use std::collections::HashMap;

use oas3::spec::{Parameter, ParameterStyle};
use percent_encoding::{AsciiSet, NON_ALPHANUMERIC, utf8_percent_encode};

use crate::generator::ast::{
  FieldNameToken, MethodNameToken, PathSegment, QueryParameter, StructMethod, StructMethodKind,
};

const QUERY_ENCODE_SET: &AsciiSet = &NON_ALPHANUMERIC.remove(b'-').remove(b'_').remove(b'.').remove(b'~');

#[derive(Debug, Clone)]
pub(crate) struct PathParamMapping {
  pub rust_field: String,
  pub original_name: String,
}

#[derive(Debug, Clone)]
pub(crate) struct QueryParamMapping {
  pub rust_field: String,
  pub original_name: String,
  pub explode: bool,
  pub style: Option<ParameterStyle>,
  pub optional: bool,
  pub is_array: bool,
}

pub(crate) fn build_render_path_method(
  path: &str,
  path_params: &[PathParamMapping],
  query_params: &[QueryParamMapping],
) -> StructMethod {
  let query_parameters = query_params
    .iter()
    .map(|m| QueryParameter {
      field: FieldNameToken::new(&m.rust_field),
      encoded_name: encode_query_name(&m.original_name),
      explode: m.explode,
      optional: m.optional,
      is_array: m.is_array,
      style: m.style,
    })
    .collect();

  StructMethod {
    name: MethodNameToken::new("render_path"),
    docs: vec!["Render the request path with parameters.".to_string()],
    kind: StructMethodKind::RenderPath {
      segments: parse_path_segments(path, path_params),
      query_params: query_parameters,
    },
    attrs: vec![],
  }
}

fn parse_path_segments(path: &str, path_params: &[PathParamMapping]) -> Vec<PathSegment> {
  if path_params.is_empty() {
    return vec![PathSegment::Literal(path.to_string())];
  }
  let param_map: HashMap<_, _> = path_params
    .iter()
    .map(|p| (&*p.original_name, &*p.rust_field))
    .collect();
  let mut segments = vec![];
  let mut current_pos = 0;
  for (brace_start, _) in path.match_indices('{') {
    if brace_start > current_pos {
      segments.push(PathSegment::Literal(path[current_pos..brace_start].to_string()));
    }
    let brace_end = path[brace_start..]
      .find('}')
      .map_or(path.len(), |offset| brace_start + offset);
    let param_name = &path[brace_start + 1..brace_end];
    if let Some(rust_field) = param_map.get(param_name) {
      segments.push(PathSegment::Parameter {
        field: FieldNameToken::new(*rust_field),
      });
    } else {
      segments.push(PathSegment::Literal(path[brace_start..=brace_end].to_string()));
    }
    current_pos = brace_end + 1;
  }
  if current_pos < path.len() {
    segments.push(PathSegment::Literal(path[current_pos..].to_string()));
  }
  segments
}

/// Percent-encode a query parameter name according to RFC 3986.
///
/// Encodes all characters except unreserved characters (A-Z, a-z, 0-9, -, _, ., ~).
pub(crate) fn encode_query_name(name: &str) -> String {
  utf8_percent_encode(name, QUERY_ENCODE_SET).to_string()
}

/// Determine if a query parameter should use exploded form.
///
/// Returns `true` if the parameter explicitly sets `explode: true`, or if
/// `explode` is unset and the style is either unspecified or `form` (the default).
pub(crate) fn query_param_explode(param: &Parameter) -> bool {
  param
    .explode
    .unwrap_or(matches!(param.style, None | Some(ParameterStyle::Form)))
}

#[cfg(test)]
mod tests {
  use std::collections::BTreeMap;

  use oas3::spec::ParameterIn;

  use super::*;
  use crate::generator::ast::PathSegment;

  #[test]
  fn test_parse_empty_path() {
    let result = parse_path_segments("", &[]);
    assert_eq!(result.len(), 1);
    assert!(matches!(&result[0], PathSegment::Literal(s) if s.is_empty()));
  }

  #[test]
  fn test_parse_path_without_parameters() {
    let result = parse_path_segments("/users/list", &[]);
    assert_eq!(result.len(), 1);
    assert!(matches!(&result[0], PathSegment::Literal(s) if s == "/users/list"));
  }

  #[test]
  fn test_parse_path_with_single_parameter() {
    let mappings = vec![PathParamMapping {
      rust_field: "user_id".to_string(),
      original_name: "userId".to_string(),
    }];
    let result = parse_path_segments("/users/{userId}", &mappings);
    assert_eq!(result.len(), 2);
    assert!(matches!(&result[0], PathSegment::Literal(s) if s == "/users/"));
    assert!(matches!(&result[1], PathSegment::Parameter { field } if field == &FieldNameToken::new("user_id")));
  }

  #[test]
  fn test_parse_path_with_multiple_parameters() {
    let mappings = vec![
      PathParamMapping {
        rust_field: "user_id".to_string(),
        original_name: "userId".to_string(),
      },
      PathParamMapping {
        rust_field: "post_id".to_string(),
        original_name: "postId".to_string(),
      },
    ];
    let result = parse_path_segments("/users/{userId}/posts/{postId}", &mappings);
    assert_eq!(result.len(), 4);
    assert!(matches!(&result[0], PathSegment::Literal(s) if s == "/users/"));
    assert!(matches!(&result[1], PathSegment::Parameter { field } if field == &FieldNameToken::new("user_id")));
    assert!(matches!(&result[2], PathSegment::Literal(s) if s == "/posts/"));
    assert!(matches!(&result[3], PathSegment::Parameter { field } if field == &FieldNameToken::new("post_id")));
  }

  #[test]
  fn test_parse_path_ending_with_parameter() {
    let mappings = vec![PathParamMapping {
      rust_field: "id".to_string(),
      original_name: "id".to_string(),
    }];
    let result = parse_path_segments("/items/{id}", &mappings);
    assert_eq!(result.len(), 2);
    assert!(matches!(&result[0], PathSegment::Literal(s) if s == "/items/"));
    assert!(matches!(&result[1], PathSegment::Parameter { field } if field == &FieldNameToken::new("id")));
  }

  #[test]
  fn test_parse_path_with_consecutive_parameters() {
    let mappings = vec![
      PathParamMapping {
        rust_field: "org".to_string(),
        original_name: "org".to_string(),
      },
      PathParamMapping {
        rust_field: "repo".to_string(),
        original_name: "repo".to_string(),
      },
    ];
    let result = parse_path_segments("/{org}/{repo}", &mappings);
    assert_eq!(result.len(), 4);
    assert!(matches!(&result[0], PathSegment::Literal(s) if s == "/"));
    assert!(matches!(&result[1], PathSegment::Parameter { field } if field == &FieldNameToken::new("org")));
    assert!(matches!(&result[2], PathSegment::Literal(s) if s == "/"));
    assert!(matches!(&result[3], PathSegment::Parameter { field } if field == &FieldNameToken::new("repo")));
  }

  #[test]
  fn test_parse_path_with_unmapped_parameter() {
    let mappings = vec![PathParamMapping {
      rust_field: "id".to_string(),
      original_name: "id".to_string(),
    }];
    let result = parse_path_segments("/items/{id}/tags/{tagId}", &mappings);
    assert_eq!(result.len(), 4);
    assert!(matches!(&result[0], PathSegment::Literal(s) if s == "/items/"));
    assert!(matches!(&result[1], PathSegment::Parameter { field } if field == &FieldNameToken::new("id")));
    assert!(matches!(&result[2], PathSegment::Literal(s) if s == "/tags/"));
    assert!(matches!(&result[3], PathSegment::Literal(s) if s == "{tagId}"));
  }

  #[test]
  fn test_encode_query_name_simple() {
    assert_eq!(encode_query_name("simple"), "simple");
  }

  #[test]
  fn test_encode_query_name_with_spaces() {
    assert_eq!(encode_query_name("has spaces"), "has%20spaces");
  }

  #[test]
  fn test_encode_query_name_with_special_chars() {
    assert_eq!(encode_query_name("foo&bar=baz"), "foo%26bar%3Dbaz");
  }

  #[test]
  fn test_encode_query_name_preserves_allowed_chars() {
    assert_eq!(encode_query_name("valid-name_1.0"), "valid-name_1.0");
  }

  #[test]
  fn test_query_param_explode_default_form() {
    let param = Parameter {
      name: "test".to_string(),
      location: ParameterIn::Query,
      required: None,
      schema: None,
      description: None,
      deprecated: None,
      allow_empty_value: None,
      allow_reserved: None,
      explode: None,
      style: None,
      content: None,
      example: None,
      examples: BTreeMap::default(),
      extensions: BTreeMap::default(),
    };
    assert!(query_param_explode(&param));
  }

  #[test]
  fn test_query_param_explode_explicit_form() {
    let param = Parameter {
      name: "test".to_string(),
      location: ParameterIn::Query,
      required: None,
      schema: None,
      description: None,
      deprecated: None,
      allow_empty_value: None,
      allow_reserved: None,
      explode: None,
      style: Some(ParameterStyle::Form),
      content: None,
      example: None,
      examples: BTreeMap::default(),
      extensions: BTreeMap::default(),
    };
    assert!(query_param_explode(&param));
  }

  #[test]
  fn test_query_param_explode_explicit_true() {
    let param = Parameter {
      name: "test".to_string(),
      location: ParameterIn::Query,
      required: None,
      schema: None,
      description: None,
      deprecated: None,
      allow_empty_value: None,
      allow_reserved: None,
      explode: Some(true),
      style: Some(ParameterStyle::SpaceDelimited),
      content: None,
      example: None,
      examples: BTreeMap::default(),
      extensions: BTreeMap::default(),
    };
    assert!(query_param_explode(&param));
  }

  #[test]
  fn test_query_param_explode_explicit_false() {
    let param = Parameter {
      name: "test".to_string(),
      location: ParameterIn::Query,
      required: None,
      schema: None,
      description: None,
      deprecated: None,
      allow_empty_value: None,
      allow_reserved: None,
      explode: Some(false),
      style: None,
      content: None,
      example: None,
      examples: BTreeMap::default(),
      extensions: BTreeMap::default(),
    };
    assert!(!query_param_explode(&param));
  }
}

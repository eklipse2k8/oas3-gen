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
  }
}

pub(crate) fn parse_path_segments(path: &str, path_params: &[PathParamMapping]) -> Vec<PathSegment> {
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

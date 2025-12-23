use std::collections::HashMap;

use crate::generator::ast::{ParsedPath, PathParseError, PathSegment, tokens::FieldNameToken};

#[test]
fn parse_literal_segment() {
  let params = HashMap::new();
  let segment = PathSegment::parse("pets", &params).unwrap();
  assert!(matches!(segment, PathSegment::Literal(s) if s == "pets"));
}

#[test]
fn parse_single_param_unknown() {
  let params = HashMap::new();
  let segment = PathSegment::parse("{id}", &params).unwrap();
  assert!(matches!(segment, PathSegment::Param(_)));
}

#[test]
fn parse_single_param_known() {
  let field = FieldNameToken::new("pet_id");
  let params: HashMap<&str, &FieldNameToken> = [("id", &field)].into_iter().collect();
  let segment = PathSegment::parse("{id}", &params).unwrap();

  let PathSegment::Param(token) = segment else {
    panic!("expected Param segment");
  };
  assert_eq!(token.to_string(), "pet_id");
}

#[test]
fn parse_mixed_segment_prefix() {
  let params = HashMap::new();
  let segment = PathSegment::parse("v{version}", &params).unwrap();

  let PathSegment::Mixed {
    format,
    params: field_params,
  } = segment
  else {
    panic!("expected Mixed segment");
  };
  assert_eq!(format, "v{}");
  assert_eq!(field_params.len(), 1);
}

#[test]
fn parse_mixed_segment_suffix() {
  let params = HashMap::new();
  let segment = PathSegment::parse("{name}.json", &params).unwrap();

  let PathSegment::Mixed {
    format,
    params: field_params,
  } = segment
  else {
    panic!("expected Mixed segment");
  };
  assert_eq!(format, "{}.json");
  assert_eq!(field_params.len(), 1);
}

#[test]
fn parse_mixed_segment_both() {
  let params = HashMap::new();
  let segment = PathSegment::parse("v{version}.json", &params).unwrap();

  let PathSegment::Mixed {
    format,
    params: field_params,
  } = segment
  else {
    panic!("expected Mixed segment");
  };
  assert_eq!(format, "v{}.json");
  assert_eq!(field_params.len(), 1);
}

#[test]
fn parse_mixed_segment_adjacent_params() {
  let params = HashMap::new();
  let segment = PathSegment::parse("{a}{b}", &params).unwrap();

  let PathSegment::Mixed {
    format,
    params: field_params,
  } = segment
  else {
    panic!("expected Mixed segment");
  };
  assert_eq!(format, "{}{}");
  assert_eq!(field_params.len(), 2);
}

#[test]
fn parse_unclosed_brace_error() {
  let params = HashMap::new();
  let result = PathSegment::parse("{unclosed", &params);
  assert!(matches!(result, Err(PathParseError::UnclosedBrace { .. })));
}

#[test]
fn parse_empty_parameter_error() {
  let params = HashMap::new();
  let result = PathSegment::parse("{}", &params);
  assert!(matches!(result, Err(PathParseError::EmptyParameter { .. })));
}

#[test]
fn parse_unmatched_closing_brace_error() {
  let params = HashMap::new();
  let result = PathSegment::parse("foo}bar", &params);
  assert!(matches!(result, Err(PathParseError::UnmatchedClosingBrace { .. })));
}

#[test]
fn parse_nested_braces_error() {
  let params = HashMap::new();
  let result = PathSegment::parse("{outer{inner}}", &params);
  assert!(matches!(result, Err(PathParseError::NestedBraces { .. })));
}

#[test]
fn parse_empty_segment() {
  let params = HashMap::new();
  let segment = PathSegment::parse("", &params).unwrap();
  assert!(matches!(segment, PathSegment::Literal(s) if s.is_empty()));
}

#[test]
fn extract_template_params_simple() {
  let params: Vec<_> = ParsedPath::extract_template_params("/projects/{projectKey}/repos/{repositorySlug}").collect();
  assert_eq!(params, vec!["projectKey", "repositorySlug"]);
}

#[test]
fn extract_template_params_none() {
  let params: Vec<_> = ParsedPath::extract_template_params("/api/v1/status").collect();
  assert!(params.is_empty());
}

#[test]
fn extract_template_params_single() {
  let params: Vec<_> = ParsedPath::extract_template_params("/users/{id}").collect();
  assert_eq!(params, vec!["id"]);
}

#[test]
fn extract_template_params_adjacent() {
  let params: Vec<_> = ParsedPath::extract_template_params("/{a}{b}/{c}").collect();
  assert_eq!(params, vec!["a", "b", "c"]);
}

#[test]
fn extract_template_params_skips_empty() {
  let params: Vec<_> = ParsedPath::extract_template_params("/foo/{}/bar/{id}").collect();
  assert_eq!(params, vec!["id"]);
}

#[test]
fn extract_template_params_handles_unclosed() {
  let params: Vec<_> = ParsedPath::extract_template_params("/foo/{unclosed").collect();
  assert!(params.is_empty());
}

#[test]
fn parsed_path_empty() {
  let path = ParsedPath::parse("/", &[]).unwrap();
  assert!(path.0.is_empty());
}

#[test]
fn parsed_path_simple_literal() {
  let path = ParsedPath::parse("/api/v1/pets", &[]).unwrap();
  assert_eq!(path.0.len(), 3);
  assert!(matches!(&path.0[0], PathSegment::Literal(s) if s == "api"));
  assert!(matches!(&path.0[1], PathSegment::Literal(s) if s == "v1"));
  assert!(matches!(&path.0[2], PathSegment::Literal(s) if s == "pets"));
}

#[test]
fn parsed_path_returns_error_on_invalid_segment() {
  let result = ParsedPath::parse("/valid/{unclosed/other", &[]);
  assert!(result.is_err());
}

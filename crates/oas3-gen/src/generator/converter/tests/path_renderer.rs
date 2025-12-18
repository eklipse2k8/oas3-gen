use std::collections::BTreeMap;

use oas3::spec::{Parameter, ParameterIn, ParameterStyle};

use crate::generator::{
  ast::{FieldNameToken, PathSegment},
  converter::path_renderer::{PathParamMapping, encode_query_name, parse_path_segments, query_param_explode},
};

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
    is_value: false,
  }];
  let result = parse_path_segments("/users/{userId}", &mappings);
  assert_eq!(result.len(), 2);
  assert!(matches!(&result[0], PathSegment::Literal(s) if s == "/users/"));
  assert!(matches!(&result[1], PathSegment::Parameter { field, .. } if field == &FieldNameToken::new("user_id")));
}

#[test]
fn test_parse_path_with_multiple_parameters() {
  let mappings = vec![
    PathParamMapping {
      rust_field: "user_id".to_string(),
      original_name: "userId".to_string(),
      is_value: false,
    },
    PathParamMapping {
      rust_field: "post_id".to_string(),
      original_name: "postId".to_string(),
      is_value: false,
    },
  ];
  let result = parse_path_segments("/users/{userId}/posts/{postId}", &mappings);
  assert_eq!(result.len(), 4);
  assert!(matches!(&result[0], PathSegment::Literal(s) if s == "/users/"));
  assert!(matches!(&result[1], PathSegment::Parameter { field, .. } if field == &FieldNameToken::new("user_id")));
  assert!(matches!(&result[2], PathSegment::Literal(s) if s == "/posts/"));
  assert!(matches!(&result[3], PathSegment::Parameter { field, .. } if field == &FieldNameToken::new("post_id")));
}

#[test]
fn test_parse_path_ending_with_parameter() {
  let mappings = vec![PathParamMapping {
    rust_field: "id".to_string(),
    original_name: "id".to_string(),
    is_value: false,
  }];
  let result = parse_path_segments("/items/{id}", &mappings);
  assert_eq!(result.len(), 2);
  assert!(matches!(&result[0], PathSegment::Literal(s) if s == "/items/"));
  assert!(matches!(&result[1], PathSegment::Parameter { field, .. } if field == &FieldNameToken::new("id")));
}

#[test]
fn test_parse_path_with_consecutive_parameters() {
  let mappings = vec![
    PathParamMapping {
      rust_field: "org".to_string(),
      original_name: "org".to_string(),
      is_value: false,
    },
    PathParamMapping {
      rust_field: "repo".to_string(),
      original_name: "repo".to_string(),
      is_value: false,
    },
  ];
  let result = parse_path_segments("/{org}/{repo}", &mappings);
  assert_eq!(result.len(), 4);
  assert!(matches!(&result[0], PathSegment::Literal(s) if s == "/"));
  assert!(matches!(&result[1], PathSegment::Parameter { field, .. } if field == &FieldNameToken::new("org")));
  assert!(matches!(&result[2], PathSegment::Literal(s) if s == "/"));
  assert!(matches!(&result[3], PathSegment::Parameter { field, .. } if field == &FieldNameToken::new("repo")));
}

#[test]
fn test_parse_path_with_unmapped_parameter() {
  let mappings = vec![PathParamMapping {
    rust_field: "id".to_string(),
    original_name: "id".to_string(),
    is_value: false,
  }];
  let result = parse_path_segments("/items/{id}/tags/{tagId}", &mappings);
  assert_eq!(result.len(), 4);
  assert!(matches!(&result[0], PathSegment::Literal(s) if s == "/items/"));
  assert!(matches!(&result[1], PathSegment::Parameter { field, .. } if field == &FieldNameToken::new("id")));
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

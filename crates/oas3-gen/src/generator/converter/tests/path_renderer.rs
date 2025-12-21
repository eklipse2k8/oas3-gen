use std::collections::BTreeMap;

use oas3::spec::{Parameter, ParameterIn, ParameterStyle};

use crate::generator::converter::path_renderer::query_param_explode;

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

use std::collections::BTreeMap;

use oas3::{
  Spec,
  spec::{MediaType, ObjectOrReference, ObjectSchema, Operation, Response, SchemaType, SchemaTypeSet},
};
use serde_json::json;

use crate::generator::naming::responses::{
  extract_all_response_types, extract_response_content_type, extract_response_type_name,
  extract_schema_name_from_response, is_error_code, is_success_code,
};

fn create_test_spec() -> Spec {
  let spec_json = json!({
    "openapi": "3.1.0",
    "info": {
      "title": "Test API",
      "version": "1.0.0"
    },
    "paths": {},
    "components": {
      "schemas": {
        "UserResponse": {
          "type": "object",
          "properties": {
            "id": {"type": "string"},
            "name": {"type": "string"}
          }
        },
        "ErrorResponse": {
          "type": "object",
          "properties": {
            "message": {"type": "string"}
          }
        }
      }
    }
  });

  serde_json::from_value(spec_json).expect("Failed to create test spec")
}

fn create_response_with_schema_ref(ref_name: &str, content_type: &str) -> Response {
  let content = BTreeMap::from([(
    content_type.to_string(),
    MediaType {
      schema: Some(ObjectOrReference::Ref {
        ref_path: format!("#/components/schemas/{ref_name}"),
        summary: None,
        description: None,
      }),
      ..Default::default()
    },
  )]);

  Response {
    content,
    ..Default::default()
  }
}

fn create_response_with_inline_schema(content_type: &str) -> Response {
  let content = BTreeMap::from([(
    content_type.to_string(),
    MediaType {
      schema: Some(ObjectOrReference::Object(ObjectSchema {
        schema_type: Some(SchemaTypeSet::Single(SchemaType::String)),
        ..Default::default()
      })),
      ..Default::default()
    },
  )]);

  Response {
    content,
    ..Default::default()
  }
}

fn create_operation_with_responses(responses: BTreeMap<String, ObjectOrReference<Response>>) -> Operation {
  Operation {
    responses: Some(responses),
    ..Default::default()
  }
}

#[test]
fn test_is_success_code() {
  assert!(is_success_code("200"));
  assert!(is_success_code("201"));
  assert!(is_success_code("204"));
  assert!(is_success_code("2XX"));
  assert!(!is_success_code("400"));
  assert!(!is_success_code("404"));
  assert!(!is_success_code("500"));
  assert!(!is_success_code("default"));
}

#[test]
fn test_is_error_code() {
  assert!(is_error_code("400"));
  assert!(is_error_code("404"));
  assert!(is_error_code("422"));
  assert!(is_error_code("4XX"));
  assert!(is_error_code("500"));
  assert!(is_error_code("503"));
  assert!(is_error_code("5XX"));
  assert!(!is_error_code("200"));
  assert!(!is_error_code("201"));
  assert!(!is_error_code("default"));
}

#[test]
fn test_extract_schema_name_from_response_with_ref() {
  let response = create_response_with_schema_ref("UserResponse", "application/json");
  let schema_name = extract_schema_name_from_response(&response);
  assert_eq!(schema_name, Some("UserResponse".to_string()));
}

#[test]
fn test_extract_schema_name_from_response_with_inline_schema() {
  let response = create_response_with_inline_schema("application/json");
  let schema_name = extract_schema_name_from_response(&response);
  assert_eq!(schema_name, None);
}

#[test]
fn test_extract_schema_name_from_response_empty_content() {
  let response = Response {
    content: BTreeMap::new(),
    ..Default::default()
  };
  let schema_name = extract_schema_name_from_response(&response);
  assert_eq!(schema_name, None);
}

#[test]
fn test_extract_response_type_name_success_response() {
  let spec = create_test_spec();
  let responses = BTreeMap::from([(
    "200".to_string(),
    ObjectOrReference::Object(create_response_with_schema_ref("UserResponse", "application/json")),
  )]);

  let operation = create_operation_with_responses(responses);
  let type_name = extract_response_type_name(&spec, &operation);
  assert_eq!(type_name, Some("UserResponse".to_string()));
}

#[test]
fn test_extract_response_type_name_fallback_to_first() {
  let spec = create_test_spec();
  let responses = BTreeMap::from([(
    "404".to_string(),
    ObjectOrReference::Object(create_response_with_schema_ref("ErrorResponse", "application/json")),
  )]);

  let operation = create_operation_with_responses(responses);
  let type_name = extract_response_type_name(&spec, &operation);
  assert_eq!(type_name, Some("ErrorResponse".to_string()));
}

#[test]
fn test_extract_response_type_name_prefers_success() {
  let spec = create_test_spec();
  let responses = BTreeMap::from([
    (
      "404".to_string(),
      ObjectOrReference::Object(create_response_with_schema_ref("ErrorResponse", "application/json")),
    ),
    (
      "200".to_string(),
      ObjectOrReference::Object(create_response_with_schema_ref("UserResponse", "application/json")),
    ),
  ]);

  let operation = create_operation_with_responses(responses);
  let type_name = extract_response_type_name(&spec, &operation);
  assert_eq!(type_name, Some("UserResponse".to_string()));
}

#[test]
fn test_extract_response_type_name_no_responses() {
  let spec = create_test_spec();
  let operation = Operation {
    responses: None,
    ..Default::default()
  };
  let type_name = extract_response_type_name(&spec, &operation);
  assert_eq!(type_name, None);
}

#[test]
fn test_extract_response_content_type_json() {
  let spec = create_test_spec();
  let responses = BTreeMap::from([(
    "200".to_string(),
    ObjectOrReference::Object(create_response_with_schema_ref("UserResponse", "application/json")),
  )]);

  let operation = create_operation_with_responses(responses);
  let content_type = extract_response_content_type(&spec, &operation);
  assert_eq!(content_type, Some("application/json".to_string()));
}

#[test]
fn test_extract_response_content_type_xml() {
  let spec = create_test_spec();
  let responses = BTreeMap::from([(
    "200".to_string(),
    ObjectOrReference::Object(create_response_with_schema_ref("UserResponse", "application/xml")),
  )]);

  let operation = create_operation_with_responses(responses);
  let content_type = extract_response_content_type(&spec, &operation);
  assert_eq!(content_type, Some("application/xml".to_string()));
}

#[test]
fn test_extract_response_content_type_no_responses() {
  let spec = create_test_spec();
  let operation = Operation {
    responses: None,
    ..Default::default()
  };
  let content_type = extract_response_content_type(&spec, &operation);
  assert_eq!(content_type, None);
}

#[test]
fn test_extract_all_response_types_success_only() {
  let spec = create_test_spec();
  let responses = BTreeMap::from([(
    "200".to_string(),
    ObjectOrReference::Object(create_response_with_schema_ref("UserResponse", "application/json")),
  )]);

  let operation = create_operation_with_responses(responses);
  let types = extract_all_response_types(&spec, &operation);
  assert_eq!(types.success.len(), 1);
  assert!(types.success.contains(&"UserResponse".to_string()));
  assert_eq!(types.error.len(), 0);
}

#[test]
fn test_extract_all_response_types_error_only() {
  let spec = create_test_spec();
  let responses = BTreeMap::from([(
    "404".to_string(),
    ObjectOrReference::Object(create_response_with_schema_ref("ErrorResponse", "application/json")),
  )]);

  let operation = create_operation_with_responses(responses);
  let types = extract_all_response_types(&spec, &operation);
  assert_eq!(types.success.len(), 0);
  assert_eq!(types.error.len(), 1);
  assert!(types.error.contains(&"ErrorResponse".to_string()));
}

#[test]
fn test_extract_all_response_types_mixed() {
  let spec = create_test_spec();
  let responses = BTreeMap::from([
    (
      "200".to_string(),
      ObjectOrReference::Object(create_response_with_schema_ref("UserResponse", "application/json")),
    ),
    (
      "404".to_string(),
      ObjectOrReference::Object(create_response_with_schema_ref("ErrorResponse", "application/json")),
    ),
  ]);

  let operation = create_operation_with_responses(responses);
  let types = extract_all_response_types(&spec, &operation);
  assert_eq!(types.success.len(), 1);
  assert!(types.success.contains(&"UserResponse".to_string()));
  assert_eq!(types.error.len(), 1);
  assert!(types.error.contains(&"ErrorResponse".to_string()));
}

#[test]
fn test_extract_all_response_types_no_responses() {
  let spec = create_test_spec();
  let operation = Operation {
    responses: None,
    ..Default::default()
  };
  let types = extract_all_response_types(&spec, &operation);
  assert_eq!(types.success.len(), 0);
  assert_eq!(types.error.len(), 0);
}

#[test]
fn test_extract_all_response_types_empty_responses() {
  let spec = create_test_spec();
  let responses = BTreeMap::new();
  let operation = create_operation_with_responses(responses);
  let types = extract_all_response_types(&spec, &operation);
  assert_eq!(types.success.len(), 0);
  assert_eq!(types.error.len(), 0);
}

#[test]
fn test_extract_all_response_types_ignores_default() {
  let spec = create_test_spec();
  let responses = BTreeMap::from([
    (
      "200".to_string(),
      ObjectOrReference::Object(create_response_with_schema_ref("UserResponse", "application/json")),
    ),
    (
      "default".to_string(),
      ObjectOrReference::Object(create_response_with_schema_ref("ErrorResponse", "application/json")),
    ),
  ]);

  let operation = create_operation_with_responses(responses);
  let types = extract_all_response_types(&spec, &operation);
  assert_eq!(types.success.len(), 1);
  assert!(types.success.contains(&"UserResponse".to_string()));
  assert_eq!(types.error.len(), 0);
}

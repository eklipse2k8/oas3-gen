use std::collections::BTreeMap;

use oas3::{
  Spec,
  spec::{MediaType, ObjectOrReference, ObjectSchema, Operation, Response, SchemaType, SchemaTypeSet},
};
use serde_json::json;

use crate::generator::naming::responses::{
  extract_all_response_types, extract_response_type_name, extract_schema_name_from_response, is_error_code,
  is_success_code,
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

fn make_empty_response() -> Response {
  Response {
    content: BTreeMap::new(),
    ..Default::default()
  }
}

#[test]
fn test_status_code_classification() {
  let success_codes = ["200", "201", "204", "2XX"];
  for code in success_codes {
    assert!(is_success_code(code), "expected {code} to be success");
    assert!(!is_error_code(code), "expected {code} not to be error");
  }

  let error_codes = ["400", "404", "422", "4XX", "500", "503", "5XX"];
  for code in error_codes {
    assert!(is_error_code(code), "expected {code} to be error");
    assert!(!is_success_code(code), "expected {code} not to be success");
  }

  assert!(!is_success_code("default"), "default should not be success");
  assert!(!is_error_code("default"), "default should not be error");
}

#[test]
fn test_extract_schema_name_from_response() {
  let cases: Vec<(Response, Option<&str>)> = vec![
    (
      create_response_with_schema_ref("UserResponse", "application/json"),
      Some("UserResponse"),
    ),
    (create_response_with_inline_schema("application/json"), None),
    (make_empty_response(), None),
  ];

  for (response, expected) in cases {
    let result = extract_schema_name_from_response(&response);
    assert_eq!(
      result.as_deref(),
      expected,
      "failed for response with content: {:?}",
      response.content.keys().collect::<Vec<_>>()
    );
  }
}

#[test]
#[allow(clippy::type_complexity)]
fn test_extract_response_type_name() {
  let spec = create_test_spec();

  let cases: Vec<(
    Option<BTreeMap<String, ObjectOrReference<Response>>>,
    Option<&str>,
    &str,
  )> = vec![
    (
      Some(BTreeMap::from([(
        "200".to_string(),
        ObjectOrReference::Object(create_response_with_schema_ref("UserResponse", "application/json")),
      )])),
      Some("UserResponse"),
      "success response",
    ),
    (
      Some(BTreeMap::from([(
        "404".to_string(),
        ObjectOrReference::Object(create_response_with_schema_ref("ErrorResponse", "application/json")),
      )])),
      Some("ErrorResponse"),
      "fallback to first when no success",
    ),
    (
      Some(BTreeMap::from([
        (
          "404".to_string(),
          ObjectOrReference::Object(create_response_with_schema_ref("ErrorResponse", "application/json")),
        ),
        (
          "200".to_string(),
          ObjectOrReference::Object(create_response_with_schema_ref("UserResponse", "application/json")),
        ),
      ])),
      Some("UserResponse"),
      "prefers success over error",
    ),
    (None, None, "no responses"),
  ];

  for (responses, expected, desc) in cases {
    let operation = Operation {
      responses,
      ..Default::default()
    };
    let result = extract_response_type_name(&spec, &operation);
    assert_eq!(result.as_deref(), expected, "failed for case: {desc}");
  }
}

struct AllResponseTypesCase {
  responses: Option<BTreeMap<String, ObjectOrReference<Response>>>,
  expected_success: Vec<&'static str>,
  expected_error: Vec<&'static str>,
  desc: &'static str,
}

#[test]
fn test_extract_all_response_types() {
  let spec = create_test_spec();

  let cases = vec![
    AllResponseTypesCase {
      responses: Some(BTreeMap::from([(
        "200".to_string(),
        ObjectOrReference::Object(create_response_with_schema_ref("UserResponse", "application/json")),
      )])),
      expected_success: vec!["UserResponse"],
      expected_error: vec![],
      desc: "success only",
    },
    AllResponseTypesCase {
      responses: Some(BTreeMap::from([(
        "404".to_string(),
        ObjectOrReference::Object(create_response_with_schema_ref("ErrorResponse", "application/json")),
      )])),
      expected_success: vec![],
      expected_error: vec!["ErrorResponse"],
      desc: "error only",
    },
    AllResponseTypesCase {
      responses: Some(BTreeMap::from([
        (
          "200".to_string(),
          ObjectOrReference::Object(create_response_with_schema_ref("UserResponse", "application/json")),
        ),
        (
          "404".to_string(),
          ObjectOrReference::Object(create_response_with_schema_ref("ErrorResponse", "application/json")),
        ),
      ])),
      expected_success: vec!["UserResponse"],
      expected_error: vec!["ErrorResponse"],
      desc: "mixed success and error",
    },
    AllResponseTypesCase {
      responses: None,
      expected_success: vec![],
      expected_error: vec![],
      desc: "no responses",
    },
    AllResponseTypesCase {
      responses: Some(BTreeMap::new()),
      expected_success: vec![],
      expected_error: vec![],
      desc: "empty responses",
    },
    AllResponseTypesCase {
      responses: Some(BTreeMap::from([
        (
          "200".to_string(),
          ObjectOrReference::Object(create_response_with_schema_ref("UserResponse", "application/json")),
        ),
        (
          "default".to_string(),
          ObjectOrReference::Object(create_response_with_schema_ref("ErrorResponse", "application/json")),
        ),
      ])),
      expected_success: vec!["UserResponse"],
      expected_error: vec![],
      desc: "ignores default response",
    },
  ];

  for case in cases {
    let operation = Operation {
      responses: case.responses,
      ..Default::default()
    };
    let types = extract_all_response_types(&spec, &operation);

    assert_eq!(
      types.success.len(),
      case.expected_success.len(),
      "success count mismatch for case: {}",
      case.desc
    );
    for expected in &case.expected_success {
      assert!(
        types.success.contains(&(*expected).to_string()),
        "missing success type {expected} for case: {}",
        case.desc
      );
    }

    assert_eq!(
      types.error.len(),
      case.expected_error.len(),
      "error count mismatch for case: {}",
      case.desc
    );
    for expected in &case.expected_error {
      assert!(
        types.error.contains(&(*expected).to_string()),
        "missing error type {expected} for case: {}",
        case.desc
      );
    }
  }
}

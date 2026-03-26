use std::{collections::BTreeMap, rc::Rc};

use http::Method;
use oas3::spec::{ObjectOrReference, ObjectSchema, Operation, Parameter};
use serde_json::json;

use crate::{
  generator::{
    ast::{ContentCategory, OperationKind, RustPrimitive, RustType, StructDef, StructToken},
    converter::{SchemaConverter, SerdeUsageRecorder, operations::OperationConverter},
    operation_registry::OperationEntry,
  },
  tests::common::{create_test_context, create_test_graph, default_config},
};

fn setup_converter(schemas: BTreeMap<String, ObjectSchema>) -> (OperationConverter, SerdeUsageRecorder) {
  let graph = create_test_graph(schemas);
  let context = create_test_context(graph, default_config());
  let schema_converter = SchemaConverter::new(&context);
  let converter = OperationConverter::new(context, schema_converter);
  let usage = SerdeUsageRecorder::new();
  (converter, usage)
}

fn extract_request_struct<'a>(types: &'a [RustType], expected_name: &str) -> &'a StructDef {
  types
    .iter()
    .find_map(|t| match t {
      RustType::Struct(s) if s.name == expected_name => Some(s),
      _ => None,
    })
    .expect("Request struct not found")
}

fn make_entry(stable_id: &str, method: Method, path: &str, operation: Operation) -> OperationEntry {
  OperationEntry {
    stable_id: stable_id.to_string(),
    method,
    path: path.to_string(),
    operation: Rc::new(operation),
    kind: OperationKind::Http,
  }
}

#[test]
fn test_basic_get_operation() -> anyhow::Result<()> {
  let (converter, _usage) = setup_converter(BTreeMap::new());
  let operation = Operation::default();

  let entry = make_entry("waddle_op", Method::GET, "/test", operation);
  let result = converter.convert(&entry)?;

  assert!(result.types.is_empty(), "Should generate no new types");
  assert_eq!(result.operation_info.operation_id, "WaddleOp");
  assert!(
    result.operation_info.request_type.is_none(),
    "Should have no request type"
  );
  assert!(
    result.operation_info.response_type.is_none(),
    "Should have no response type"
  );
  Ok(())
}

#[test]
fn test_multi_content_type_response_splits_by_category() -> anyhow::Result<()> {
  let (converter, _usage) = setup_converter(BTreeMap::new());

  let operation_json = json!({
    "operationId": "getKibbleImage",
    "responses": {
      "200": {
        "description": "The kibble image or fluff",
        "content": {
          "application/json": {
            "schema": {
              "type": "object",
              "properties": {
                "url": { "type": "string" }
              }
            }
          },
          "image/webp": {
            "schema": {
              "type": "string",
              "format": "binary"
            }
          }
        }
      }
    }
  });

  let operation = serde_json::from_value::<Operation>(operation_json)?;

  let entry = make_entry("get_kibble_image", Method::GET, "/kibble/image", operation);
  let result = converter.convert(&entry)?;

  let response_enum = result
    .types
    .iter()
    .find_map(|t| match t {
      RustType::ResponseEnum(e) if e.name == "GetKibbleImageResponse" => Some(e),
      _ => None,
    })
    .expect("Response enum not found");

  let ok_variant = response_enum.variants.iter().find(|v| v.variant_name == "Ok");
  let binary_variant = response_enum.variants.iter().find(|v| v.variant_name == "OkBinary");

  assert!(ok_variant.is_some(), "Should have Ok variant for JSON content type");
  assert!(
    binary_variant.is_some(),
    "Should have OkBinary variant for binary content types"
  );

  Ok(())
}

#[test]
fn test_operation_with_request_body_ref() -> anyhow::Result<()> {
  let corgi_schema: ObjectSchema = serde_json::from_value::<ObjectSchema>(json!({
    "type": "object"
  }))?;

  let (converter, _usage) = setup_converter(BTreeMap::from([("Corgi".to_string(), corgi_schema)]));

  let operation_json = json!({
    "requestBody": {
      "content": {
        "application/json": {
          "schema": { "$ref": "#/components/schemas/Corgi" }
        }
      }
    }
  });

  let operation = serde_json::from_value::<Operation>(operation_json)?;

  let entry = make_entry("zoom_corgi", Method::POST, "/corgis", operation);
  let result = converter.convert(&entry)?;

  assert_eq!(result.types.len(), 1, "Should generate only the Request struct");
  assert!(
    result
      .types
      .iter()
      .any(|t| matches!(t, RustType::Struct(s) if s.name == "ZoomCorgiRequest"))
  );

  let request_struct = result
    .types
    .iter()
    .find_map(|t| match t {
      RustType::Struct(s) if s.name == "ZoomCorgiRequest" => Some(s),
      _ => None,
    })
    .expect("Request struct not found");

  let body_field = request_struct
    .fields
    .iter()
    .find(|f| f.name == "body")
    .expect("Body field not found");

  assert_eq!(
    body_field.rust_type.to_rust_type(),
    "Option<Corgi>",
    "Body field should reference Corgi directly"
  );

  assert!(result.operation_info.body.is_some(), "Should have body metadata");
  Ok(())
}

#[test]
fn test_operation_with_response_type() -> anyhow::Result<()> {
  let corgi_schema: ObjectSchema = serde_json::from_value::<ObjectSchema>(json!({
    "type": "object"
  }))?;

  let (converter, _usage) = setup_converter(BTreeMap::from([("Corgi".to_string(), corgi_schema)]));

  let operation_json = json!({
    "responses": {
      "200": {
        "content": {
          "application/json": {
            "schema": { "$ref": "#/components/schemas/Corgi" }
          }
        }
      }
    }
  });

  let operation = serde_json::from_value::<Operation>(operation_json)?;

  let entry = make_entry("get_corgi", Method::GET, "/corgi", operation);
  let result = converter.convert(&entry)?;

  assert_eq!(result.operation_info.response_type.as_deref(), Some("Corgi"));
  Ok(())
}

#[test]
#[allow(clippy::type_complexity)]
fn test_path_parameter_type_mapping() -> anyhow::Result<()> {
  let cases: &[(&str, &str, &str, Option<&str>, &str, &str)] = &[
    ("id", "getById", "integer", Some("int64"), "GetByIdRequest", "i64"),
    (
      "count",
      "getByCount",
      "integer",
      Some("int32"),
      "GetByCountRequest",
      "i32",
    ),
    (
      "amount",
      "getByAmount",
      "number",
      Some("double"),
      "GetByAmountRequest",
      "f64",
    ),
    ("active", "getByActive", "boolean", None, "GetByActiveRequest", "bool"),
    (
      "uuid",
      "getByUuid",
      "string",
      Some("uuid"),
      "GetByUuidRequest",
      "uuid::Uuid",
    ),
    (
      "timestamp",
      "getByTimestamp",
      "string",
      Some("date-time"),
      "GetByTimestampRequest",
      "chrono::DateTime<chrono::Utc>",
    ),
  ];

  for (param_name, op_id, schema_type, format, expected_struct, expected_type) in cases {
    let (converter, _usage) = setup_converter(BTreeMap::new());

    let param_json = json!({
      "name": param_name,
      "in": "path",
      "required": true,
      "schema": {
        "type": schema_type,
        "format": format
      }
    });

    let mut operation: Operation = Operation::default();
    operation
      .parameters
      .push(ObjectOrReference::Object(serde_json::from_value::<Parameter>(
        param_json,
      )?));

    let path = format!("/items/{{{param_name}}}");
    let snake_op_id = inflections::case::to_snake_case(op_id);

    let entry = make_entry(&snake_op_id, Method::GET, &path, operation);
    let result = converter.convert(&entry)?;

    assert_eq!(
      result.types.len(),
      2,
      "Should generate request struct and path struct for {op_id}"
    );

    let request_type_name = result
      .operation_info
      .request_type
      .as_ref()
      .map(StructToken::as_str)
      .expect("Request type should exist");

    assert_eq!(
      request_type_name, *expected_struct,
      "Request struct name mismatch for {op_id}"
    );

    let path_struct_name = format!("{expected_struct}Path");
    let path_struct = extract_request_struct(&result.types, &path_struct_name);
    assert_eq!(
      path_struct.fields.len(),
      1,
      "Path struct should have one field for {op_id}"
    );
    assert_eq!(
      path_struct.fields[0].rust_type.to_rust_type(),
      *expected_type,
      "Type mismatch for {op_id}"
    );
  }
  Ok(())
}

#[test]
fn test_operation_with_multiple_path_parameters() -> anyhow::Result<()> {
  let (converter, _usage) = setup_converter(BTreeMap::new());

  let operation_json = json!({
    "parameters": [
      {
        "name": "userId",
        "in": "path",
        "required": true,
        "schema": {
          "type": "integer",
          "format": "int64"
        }
      },
      {
        "name": "postId",
        "in": "path",
        "required": true,
        "schema": {
          "type": "string"
        }
      }
    ]
  });

  let operation = serde_json::from_value::<Operation>(operation_json)?;

  let entry = make_entry(
    "get_user_post",
    Method::GET,
    "/users/{userId}/posts/{postId}",
    operation,
  );
  let result = converter.convert(&entry)?;

  let request_struct = extract_request_struct(&result.types, "GetUserPostRequest");
  assert_eq!(request_struct.fields.len(), 1, "Main struct should have path field");
  assert_eq!(
    request_struct.fields[0].name, "path",
    "Main struct should have path field"
  );

  let path_struct = extract_request_struct(&result.types, "GetUserPostRequestPath");
  assert_eq!(path_struct.fields.len(), 2);
  assert_eq!(path_struct.fields[0].name, "user_id");
  assert_eq!(path_struct.fields[0].rust_type.to_rust_type(), "i64");
  assert_eq!(path_struct.fields[1].name, "post_id");
  assert_eq!(path_struct.fields[1].rust_type.to_rust_type(), "String");

  Ok(())
}

#[test]
fn test_binary_response_uses_bytes_type() -> anyhow::Result<()> {
  let content_types = [
    ("application/octet-stream", ContentCategory::Binary),
    ("image/png", ContentCategory::Binary),
    ("video/mp4", ContentCategory::Binary),
    ("audio/mpeg", ContentCategory::Binary),
    ("application/pdf", ContentCategory::Binary),
  ];

  for (content_type, expected_category) in content_types {
    let (converter, _usage) = setup_converter(BTreeMap::new());

    let operation_json = json!({
      "operationId": "downloadFile",
      "responses": {
        "200": {
          "content": {
            content_type: {
              "schema": {
                "type": "string",
                "format": "binary"
              }
            }
          }
        },
        "4XX": {
          "content": {
            "application/json": {
              "schema": {
                "type": "object"
              }
            }
          }
        }
      }
    });

    let operation = serde_json::from_value::<Operation>(operation_json)?;

    let entry = make_entry("download_file", Method::GET, "/files/download", operation);
    let result = converter.convert(&entry)?;

    let response_enum = result
      .types
      .iter()
      .find_map(|t| match t {
        RustType::ResponseEnum(e) if e.name == "DownloadFileResponse" => Some(e),
        _ => None,
      })
      .unwrap_or_else(|| panic!("Response enum not found for {content_type}"));

    let ok_variant = response_enum
      .variants
      .iter()
      .find(|v| v.variant_name == "Ok")
      .unwrap_or_else(|| panic!("Ok variant not found for {content_type}"));

    assert_eq!(
      ok_variant
        .media_types
        .first()
        .map_or(ContentCategory::Json, |m| m.category),
      expected_category,
      "content_category mismatch for {content_type}"
    );

    let schema_type = ok_variant
      .schema_type
      .as_ref()
      .unwrap_or_else(|| panic!("schema_type not found for {content_type}"));

    assert_eq!(
      schema_type.base_type,
      RustPrimitive::Bytes,
      "binary response should use Vec<u8> for {content_type}, got {:?}",
      schema_type.base_type
    );

    assert!(
      result.operation_info.response_enum.is_some(),
      "response_enum should be set for {content_type}"
    );

    let error_variant = response_enum
      .variants
      .iter()
      .find(|v| v.variant_name == "ClientError")
      .unwrap_or_else(|| panic!("ClientError variant not found for {content_type}"));

    assert_eq!(
      error_variant
        .media_types
        .first()
        .map_or(ContentCategory::Json, |m| m.category),
      ContentCategory::Json,
      "error response should be Json for {content_type}"
    );

    assert!(
      error_variant.schema_type.as_ref().is_some(),
      "error response should preserve schema type for {content_type}"
    );
  }
  Ok(())
}

#[test]
fn test_event_stream_response_splits_variants() -> anyhow::Result<()> {
  let event_schema = serde_json::from_value::<ObjectSchema>(json!({
    "type": "object",
    "properties": {
      "message": { "type": "string" }
    },
    "required": ["message"]
  }))?;

  let (converter, _usage) = setup_converter(BTreeMap::from([("EventPayload".to_string(), event_schema)]));

  let operation_json = json!({
    "operationId": "getEvents",
    "responses": {
      "200": {
        "content": {
          "application/json": {
            "schema": { "$ref": "#/components/schemas/EventPayload" }
          },
          "text/event-stream": {
            "schema": { "$ref": "#/components/schemas/EventPayload" }
          }
        }
      }
    }
  });

  let operation = serde_json::from_value::<Operation>(operation_json)?;

  let entry = make_entry("get_events", Method::GET, "/events", operation);
  let result = converter.convert(&entry)?;

  let response_enum = result
    .types
    .iter()
    .find_map(|t| match t {
      RustType::ResponseEnum(e) if e.name == "GetEventsResponse" => Some(e),
      _ => None,
    })
    .expect("Response enum not found");

  let ok_variant = response_enum
    .variants
    .iter()
    .find(|v| v.variant_name == "Ok")
    .expect("Ok variant not found (JSON is the default, no suffix)");

  let stream_variant = response_enum
    .variants
    .iter()
    .find(|v| v.variant_name == "OkEventStream")
    .expect("OkEventStream variant not found");

  assert_eq!(
    ok_variant
      .schema_type
      .as_ref()
      .expect("Ok variant should have schema")
      .to_rust_type(),
    "EventPayload",
  );
  assert!(
    ok_variant
      .media_types
      .iter()
      .all(|m| m.category == ContentCategory::Json),
    "Ok variant should only contain JSON media types",
  );

  assert_eq!(
    stream_variant
      .schema_type
      .as_ref()
      .expect("Event stream variant should have schema")
      .to_rust_type(),
    "oas3_gen_support::EventStream<EventPayload>",
  );
  assert!(
    stream_variant
      .media_types
      .iter()
      .all(|m| m.category == ContentCategory::EventStream),
    "Event stream variant should only contain event stream media types",
  );
  Ok(())
}

#[test]
fn test_response_enum_adds_default_variant() -> anyhow::Result<()> {
  let (converter, _usage) = setup_converter(BTreeMap::new());

  let operation_json = json!({
    "operationId": "getItem",
    "responses": {
      "200": {
        "content": {
          "application/json": {
            "schema": {
              "type": "object"
            }
          }
        }
      }
    }
  });

  let operation = serde_json::from_value::<Operation>(operation_json)?;

  let entry = make_entry("get_item", Method::GET, "/items", operation);
  let result = converter.convert(&entry)?;

  let response_enum = result
    .types
    .iter()
    .find_map(|t| match t {
      RustType::ResponseEnum(e) if e.name == "GetItemResponse" => Some(e),
      _ => None,
    })
    .expect("Response enum not found");

  let default_variant = response_enum.variants.iter().find(|v| v.variant_name == "Unknown");

  assert!(default_variant.is_some(), "Default variant should be added");
  assert!(
    default_variant.unwrap().schema_type.as_ref().is_none(),
    "Default variant should have no schema type"
  );
  Ok(())
}

#[test]
fn test_response_enum_preserves_existing_default() -> anyhow::Result<()> {
  let error_schema = serde_json::from_value::<ObjectSchema>(json!({
    "type": "object"
  }))?;

  let (converter, _usage) = setup_converter(BTreeMap::from([("Error".to_string(), error_schema)]));

  let operation_json = json!({
    "operationId": "getItem",
    "responses": {
      "200": {
        "content": {
          "application/json": {
            "schema": {
              "type": "object"
            }
          }
        }
      },
      "default": {
        "description": "Error response",
        "content": {
          "application/json": {
            "schema": { "$ref": "#/components/schemas/Error" }
          }
        }
      }
    }
  });

  let operation = serde_json::from_value::<Operation>(operation_json)?;

  let entry = make_entry("get_item", Method::GET, "/items", operation);
  let result = converter.convert(&entry)?;

  let response_enum = result
    .types
    .iter()
    .find_map(|t| match t {
      RustType::ResponseEnum(e) if e.name == "GetItemResponse" => Some(e),
      _ => None,
    })
    .expect("Response enum not found");

  let auto_added_unknown = response_enum
    .variants
    .iter()
    .filter(|v| v.variant_name == "Unknown")
    .count();

  assert_eq!(
    auto_added_unknown, 1,
    "Should only have one Unknown variant (from the default response, not auto-added)"
  );

  let default_variant = response_enum.variants.iter().find(|v| v.status_code.is_default());

  assert!(default_variant.is_some(), "Default variant should exist");
  assert!(
    default_variant.unwrap().schema_type.as_ref().is_some(),
    "Default variant should have schema type from spec"
  );
  Ok(())
}

#[test]
fn test_response_with_primitive_type() -> anyhow::Result<()> {
  let (converter, _usage) = setup_converter(BTreeMap::new());

  let operation_json = json!({
    "operationId": "getCount",
    "responses": {
      "200": {
        "content": {
          "application/json": {
            "schema": {
              "type": "integer",
              "format": "int64"
            }
          }
        }
      }
    }
  });

  let operation = serde_json::from_value::<Operation>(operation_json)?;

  let entry = make_entry("get_count", Method::GET, "/count", operation);
  let result = converter.convert(&entry)?;

  let response_enum = result
    .types
    .iter()
    .find_map(|t| match t {
      RustType::ResponseEnum(e) if e.name == "GetCountResponse" => Some(e),
      _ => None,
    })
    .expect("Response enum not found");

  let ok_variant = response_enum
    .variants
    .iter()
    .find(|v| v.variant_name == "Ok")
    .expect("Ok variant not found");

  assert!(ok_variant.schema_type.as_ref().is_some(), "Should have schema type");
  assert_eq!(
    ok_variant.schema_type.as_ref().unwrap().base_type,
    RustPrimitive::I64,
    "Should use i64 for int64 format"
  );
  Ok(())
}

#[test]
fn test_response_with_no_content() -> anyhow::Result<()> {
  let (converter, _usage) = setup_converter(BTreeMap::new());

  let operation_json = json!({
    "operationId": "deleteItem",
    "responses": {
      "204": {
        "description": "No content"
      }
    }
  });

  let operation = serde_json::from_value::<Operation>(operation_json)?;

  let entry = make_entry("delete_item", Method::DELETE, "/items/{id}", operation);
  let result = converter.convert(&entry)?;

  let response_enum = result
    .types
    .iter()
    .find_map(|t| match t {
      RustType::ResponseEnum(e) if e.name == "DeleteItemResponse" => Some(e),
      _ => None,
    })
    .expect("Response enum not found");

  let no_content_variant = response_enum
    .variants
    .iter()
    .find(|v| v.variant_name == "NoContent")
    .expect("NoContent variant not found");

  assert!(
    no_content_variant.schema_type.as_ref().is_none(),
    "No content response should have no schema type"
  );
  Ok(())
}

#[test]
fn test_operation_with_oneof_request_body() -> anyhow::Result<()> {
  let model_params_schema = serde_json::from_value::<ObjectSchema>(json!({
    "type": "object",
    "properties": {
      "model": { "type": "string" }
    }
  }))?;

  let agent_params_schema = serde_json::from_value::<ObjectSchema>(json!({
    "type": "object",
    "properties": {
      "agent": { "type": "string" }
    }
  }))?;

  let (converter, _usage) = setup_converter(BTreeMap::from([
    ("CreateModelParams".to_string(), model_params_schema),
    ("CreateAgentParams".to_string(), agent_params_schema),
  ]));

  let operation_json = json!({
    "requestBody": {
      "required": true,
      "content": {
        "application/json": {
          "schema": {
            "oneOf": [
              { "$ref": "#/components/schemas/CreateModelParams" },
              { "$ref": "#/components/schemas/CreateAgentParams" }
            ]
          }
        }
      }
    }
  });

  let operation = serde_json::from_value::<Operation>(operation_json)?;

  let entry = make_entry("create_interaction", Method::POST, "/interactions", operation);
  let result = converter.convert(&entry)?;

  assert!(
    result.operation_info.request_type.is_some(),
    "Should have a request type"
  );
  let request_type_name = result.operation_info.request_type.as_ref().unwrap().as_str();
  assert_eq!(request_type_name, "CreateInteractionRequest");

  assert!(result.operation_info.body.is_some(), "Should have body metadata");
  let body_meta = result.operation_info.body.as_ref().unwrap();
  assert_eq!(body_meta.field_name.as_str(), "body");
  assert!(!body_meta.optional, "Body should be required");

  let request_struct = result
    .types
    .iter()
    .find_map(|t| match t {
      RustType::Struct(s) if s.name == "CreateInteractionRequest" => Some(s),
      _ => None,
    })
    .expect("Request struct not found");

  let body_field = request_struct
    .fields
    .iter()
    .find(|f| f.name == "body")
    .expect("Body field not found");

  assert!(!body_field.rust_type.nullable, "Required body should not be nullable");

  let union_enum = result.types.iter().find(|t| matches!(t, RustType::Enum(e) if e.name.as_str().contains("InteractionRequestBody") || e.name.as_str().contains("RequestBody")));
  assert!(
    union_enum.is_some(),
    "Should generate a union enum for oneOf request body. Generated types: {:?}",
    result
      .types
      .iter()
      .map(|t| match t {
        RustType::Struct(s) => format!("Struct({})", s.name),
        RustType::Enum(e) => format!("Enum({})", e.name),
        RustType::TypeAlias(a) => format!("Alias({})", a.name),
        _ => "Other".to_string(),
      })
      .collect::<Vec<_>>()
  );

  Ok(())
}

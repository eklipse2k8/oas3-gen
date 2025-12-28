use std::collections::{BTreeMap, BTreeMap as ResponseMap};

use http::Method;
use oas3::spec::{
  MediaType, ObjectOrReference, ObjectSchema, Operation, Parameter, ParameterIn, RequestBody, Response, SchemaType,
  SchemaTypeSet,
};

use crate::{
  generator::{
    ast::{ContentCategory, OperationKind, RustPrimitive, RustType, StructDef, StructToken},
    converter::{SchemaConverter, TypeUsageRecorder, cache::SharedSchemaCache, operations::OperationConverter},
  },
  tests::common::{create_test_graph, default_config},
};

fn setup_converter(
  schemas: BTreeMap<String, ObjectSchema>,
) -> (OperationConverter<'static>, TypeUsageRecorder, SharedSchemaCache) {
  let graph = create_test_graph(schemas);
  let spec = Box::leak(Box::new(graph.spec().clone()));
  let schema_converter = Box::leak(Box::new(SchemaConverter::new(&graph, &default_config())));
  let converter = OperationConverter::new(schema_converter, spec);
  let usage = TypeUsageRecorder::new();
  let cache = SharedSchemaCache::new();
  (converter, usage, cache)
}

fn create_parameter(
  name: &str,
  location: ParameterIn,
  schema_type: SchemaType,
  format: Option<&str>,
  required: bool,
) -> ObjectOrReference<Parameter> {
  ObjectOrReference::Object(Parameter {
    name: name.to_string(),
    location,
    required: Some(required),
    schema: Some(ObjectOrReference::Object(ObjectSchema {
      schema_type: Some(SchemaTypeSet::Single(schema_type)),
      format: format.map(String::from),
      ..Default::default()
    })),
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
  })
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

#[test]
fn test_basic_get_operation() -> anyhow::Result<()> {
  let (converter, mut usage, mut cache) = setup_converter(BTreeMap::new());
  let operation = Operation::default();

  let result = converter.convert(
    "my_op",
    "myOp",
    &Method::GET,
    "/test",
    OperationKind::Http,
    &operation,
    &mut usage,
    &mut cache,
  )?;

  assert!(result.types.is_empty(), "Should generate no new types");
  assert_eq!(result.operation_info.operation_id, "MyOp");
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
fn test_operation_with_path_parameter() -> anyhow::Result<()> {
  let (converter, mut usage, mut cache) = setup_converter(BTreeMap::new());
  let mut operation = Operation::default();
  operation.parameters.push(create_parameter(
    "userId",
    ParameterIn::Path,
    SchemaType::String,
    None,
    true,
  ));

  let result = converter.convert(
    "get_user",
    "getUser",
    &Method::GET,
    "/users/{userId}",
    OperationKind::Http,
    &operation,
    &mut usage,
    &mut cache,
  )?;

  assert_eq!(
    result.types.len(),
    2,
    "Should generate request struct and nested path struct"
  );
  let request_type_name = result
    .operation_info
    .request_type
    .as_ref()
    .map(StructToken::as_str)
    .expect("Request type should exist");
  assert_eq!(request_type_name, "GetUserRequest");

  let request_struct = extract_request_struct(&result.types, request_type_name);
  assert_eq!(request_struct.fields.len(), 1, "Main struct should have path field");
  assert_eq!(
    request_struct.fields[0].name, "path",
    "Main struct should have path field"
  );

  let path_struct = extract_request_struct(&result.types, "GetUserRequestPath");
  assert_eq!(path_struct.fields.len(), 1);
  assert_eq!(path_struct.fields[0].name, "user_id");

  Ok(())
}

#[test]
fn test_operation_with_request_body_ref() -> anyhow::Result<()> {
  let user_schema = ObjectSchema {
    schema_type: Some(SchemaTypeSet::Single(SchemaType::Object)),
    ..Default::default()
  };
  let (converter, mut usage, mut cache) = setup_converter(BTreeMap::from([("User".to_string(), user_schema)]));

  let operation = Operation {
    request_body: Some(ObjectOrReference::Object(RequestBody {
      content: BTreeMap::from([(
        "application/json".to_string(),
        MediaType {
          schema: Some(ObjectOrReference::Ref {
            ref_path: "#/components/schemas/User".to_string(),
            summary: None,
            description: None,
          }),
          ..Default::default()
        },
      )]),
      ..Default::default()
    })),
    ..Default::default()
  };

  let result = converter.convert(
    "create_user",
    "createUser",
    &Method::POST,
    "/users",
    OperationKind::Http,
    &operation,
    &mut usage,
    &mut cache,
  )?;

  assert_eq!(result.types.len(), 1, "Should generate only the Request struct");
  assert!(
    result
      .types
      .iter()
      .any(|t| matches!(t, RustType::Struct(s) if s.name == "CreateUserRequest"))
  );

  let request_struct = result
    .types
    .iter()
    .find_map(|t| match t {
      RustType::Struct(s) if s.name == "CreateUserRequest" => Some(s),
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
    "Option<User>",
    "Body field should reference User directly"
  );

  assert!(result.operation_info.body.is_some(), "Should have body metadata");
  Ok(())
}

#[test]
fn test_operation_with_response_type() -> anyhow::Result<()> {
  let user_schema = ObjectSchema {
    schema_type: Some(SchemaTypeSet::Single(SchemaType::Object)),
    ..Default::default()
  };
  let (converter, mut usage, mut cache) = setup_converter(BTreeMap::from([("User".to_string(), user_schema)]));

  let operation = Operation {
    responses: Some(ResponseMap::from([(
      "200".to_string(),
      ObjectOrReference::Object(Response {
        content: BTreeMap::from([(
          "application/json".to_string(),
          MediaType {
            schema: Some(ObjectOrReference::Ref {
              ref_path: "#/components/schemas/User".to_string(),
              summary: None,
              description: None,
            }),
            ..Default::default()
          },
        )]),
        ..Default::default()
      }),
    )])),
    ..Default::default()
  };

  let result = converter.convert(
    "get_user",
    "getUser",
    &Method::GET,
    "/user",
    OperationKind::Http,
    &operation,
    &mut usage,
    &mut cache,
  )?;

  assert_eq!(result.operation_info.response_type.as_deref(), Some("User"));
  Ok(())
}

#[test]
#[allow(clippy::type_complexity)]
fn test_path_parameter_type_mapping() -> anyhow::Result<()> {
  let cases: &[(&str, &str, SchemaType, Option<&str>, &str, &str)] = &[
    (
      "id",
      "getById",
      SchemaType::Integer,
      Some("int64"),
      "GetByIdRequest",
      "i64",
    ),
    (
      "count",
      "getByCount",
      SchemaType::Integer,
      Some("int32"),
      "GetByCountRequest",
      "i32",
    ),
    (
      "amount",
      "getByAmount",
      SchemaType::Number,
      Some("double"),
      "GetByAmountRequest",
      "f64",
    ),
    (
      "active",
      "getByActive",
      SchemaType::Boolean,
      None,
      "GetByActiveRequest",
      "bool",
    ),
    (
      "uuid",
      "getByUuid",
      SchemaType::String,
      Some("uuid"),
      "GetByUuidRequest",
      "uuid::Uuid",
    ),
    (
      "timestamp",
      "getByTimestamp",
      SchemaType::String,
      Some("date-time"),
      "GetByTimestampRequest",
      "chrono::DateTime<chrono::Utc>",
    ),
  ];

  for (param_name, op_id, schema_type, format, expected_struct, expected_type) in cases {
    let (converter, mut usage, mut cache) = setup_converter(BTreeMap::new());
    let mut operation = Operation::default();
    operation.parameters.push(create_parameter(
      param_name,
      ParameterIn::Path,
      *schema_type,
      *format,
      true,
    ));

    let path = format!("/items/{{{param_name}}}");
    let snake_op_id = inflections::case::to_snake_case(op_id);

    let result = converter.convert(
      &snake_op_id,
      op_id,
      &Method::GET,
      &path,
      OperationKind::Http,
      &operation,
      &mut usage,
      &mut cache,
    )?;

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
  let (converter, mut usage, mut cache) = setup_converter(BTreeMap::new());
  let mut operation = Operation::default();
  operation.parameters.push(create_parameter(
    "userId",
    ParameterIn::Path,
    SchemaType::Integer,
    Some("int64"),
    true,
  ));
  operation.parameters.push(create_parameter(
    "postId",
    ParameterIn::Path,
    SchemaType::String,
    None,
    true,
  ));

  let result = converter.convert(
    "get_user_post",
    "getUserPost",
    &Method::GET,
    "/users/{userId}/posts/{postId}",
    OperationKind::Http,
    &operation,
    &mut usage,
    &mut cache,
  )?;

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
    let (converter, mut usage, mut cache) = setup_converter(BTreeMap::new());

    let operation = Operation {
      operation_id: Some("downloadFile".to_string()),
      responses: Some(ResponseMap::from([
        (
          "200".to_string(),
          ObjectOrReference::Object(Response {
            content: BTreeMap::from([(
              content_type.to_string(),
              MediaType {
                schema: Some(ObjectOrReference::Object(ObjectSchema {
                  schema_type: Some(SchemaTypeSet::Single(SchemaType::String)),
                  format: Some("binary".to_string()),
                  ..Default::default()
                })),
                ..Default::default()
              },
            )]),
            ..Default::default()
          }),
        ),
        (
          "4XX".to_string(),
          ObjectOrReference::Object(Response {
            content: BTreeMap::from([(
              "application/json".to_string(),
              MediaType {
                schema: Some(ObjectOrReference::Object(ObjectSchema {
                  schema_type: Some(SchemaTypeSet::Single(SchemaType::Object)),
                  ..Default::default()
                })),
                ..Default::default()
              },
            )]),
            ..Default::default()
          }),
        ),
      ])),
      ..Default::default()
    };

    let result = converter.convert(
      "download_file",
      "downloadFile",
      &Method::GET,
      "/files/download",
      OperationKind::Http,
      &operation,
      &mut usage,
      &mut cache,
    )?;

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
  let event_schema = ObjectSchema {
    schema_type: Some(SchemaTypeSet::Single(SchemaType::Object)),
    properties: BTreeMap::from([(
      "message".to_string(),
      ObjectOrReference::Object(ObjectSchema {
        schema_type: Some(SchemaTypeSet::Single(SchemaType::String)),
        ..Default::default()
      }),
    )]),
    required: vec!["message".to_string()],
    ..Default::default()
  };

  let (converter, mut usage, mut cache) = setup_converter(BTreeMap::from([("EventPayload".to_string(), event_schema)]));

  let operation = Operation {
    operation_id: Some("getEvents".to_string()),
    responses: Some(ResponseMap::from([(
      "200".to_string(),
      ObjectOrReference::Object(Response {
        content: BTreeMap::from([
          (
            "application/json".to_string(),
            MediaType {
              schema: Some(ObjectOrReference::Ref {
                ref_path: "#/components/schemas/EventPayload".to_string(),
                summary: None,
                description: None,
              }),
              ..Default::default()
            },
          ),
          (
            "text/event-stream".to_string(),
            MediaType {
              schema: Some(ObjectOrReference::Ref {
                ref_path: "#/components/schemas/EventPayload".to_string(),
                summary: None,
                description: None,
              }),
              ..Default::default()
            },
          ),
        ]),
        ..Default::default()
      }),
    )])),
    ..Default::default()
  };

  let result = converter.convert(
    "get_events",
    "getEvents",
    &Method::GET,
    "/events",
    OperationKind::Http,
    &operation,
    &mut usage,
    &mut cache,
  )?;

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
    .expect("Ok variant not found");

  let stream_variant = response_enum
    .variants
    .iter()
    .find(|v| v.variant_name == "OkEventStream")
    .expect("Event stream variant not found");

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
      .all(|m| m.category != ContentCategory::EventStream),
    "Ok variant should exclude event streams",
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
  let (converter, mut usage, mut cache) = setup_converter(BTreeMap::new());

  let operation = Operation {
    operation_id: Some("getItem".to_string()),
    responses: Some(ResponseMap::from([(
      "200".to_string(),
      ObjectOrReference::Object(Response {
        content: BTreeMap::from([(
          "application/json".to_string(),
          MediaType {
            schema: Some(ObjectOrReference::Object(ObjectSchema {
              schema_type: Some(SchemaTypeSet::Single(SchemaType::Object)),
              ..Default::default()
            })),
            ..Default::default()
          },
        )]),
        ..Default::default()
      }),
    )])),
    ..Default::default()
  };

  let result = converter.convert(
    "get_item",
    "getItem",
    &Method::GET,
    "/items",
    OperationKind::Http,
    &operation,
    &mut usage,
    &mut cache,
  )?;

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
  let error_schema = ObjectSchema {
    schema_type: Some(SchemaTypeSet::Single(SchemaType::Object)),
    ..Default::default()
  };
  let (converter, mut usage, mut cache) = setup_converter(BTreeMap::from([("Error".to_string(), error_schema)]));

  let operation = Operation {
    operation_id: Some("getItem".to_string()),
    responses: Some(ResponseMap::from([
      (
        "200".to_string(),
        ObjectOrReference::Object(Response {
          content: BTreeMap::from([(
            "application/json".to_string(),
            MediaType {
              schema: Some(ObjectOrReference::Object(ObjectSchema {
                schema_type: Some(SchemaTypeSet::Single(SchemaType::Object)),
                ..Default::default()
              })),
              ..Default::default()
            },
          )]),
          ..Default::default()
        }),
      ),
      (
        "default".to_string(),
        ObjectOrReference::Object(Response {
          description: Some("Error response".to_string()),
          content: BTreeMap::from([(
            "application/json".to_string(),
            MediaType {
              schema: Some(ObjectOrReference::Ref {
                ref_path: "#/components/schemas/Error".to_string(),
                summary: None,
                description: None,
              }),
              ..Default::default()
            },
          )]),
          ..Default::default()
        }),
      ),
    ])),
    ..Default::default()
  };

  let result = converter.convert(
    "get_item",
    "getItem",
    &Method::GET,
    "/items",
    OperationKind::Http,
    &operation,
    &mut usage,
    &mut cache,
  )?;

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
  let (converter, mut usage, mut cache) = setup_converter(BTreeMap::new());

  let operation = Operation {
    operation_id: Some("getCount".to_string()),
    responses: Some(ResponseMap::from([(
      "200".to_string(),
      ObjectOrReference::Object(Response {
        content: BTreeMap::from([(
          "application/json".to_string(),
          MediaType {
            schema: Some(ObjectOrReference::Object(ObjectSchema {
              schema_type: Some(SchemaTypeSet::Single(SchemaType::Integer)),
              format: Some("int64".to_string()),
              ..Default::default()
            })),
            ..Default::default()
          },
        )]),
        ..Default::default()
      }),
    )])),
    ..Default::default()
  };

  let result = converter.convert(
    "get_count",
    "getCount",
    &Method::GET,
    "/count",
    OperationKind::Http,
    &operation,
    &mut usage,
    &mut cache,
  )?;

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
  let (converter, mut usage, mut cache) = setup_converter(BTreeMap::new());

  let operation = Operation {
    operation_id: Some("deleteItem".to_string()),
    responses: Some(ResponseMap::from([(
      "204".to_string(),
      ObjectOrReference::Object(Response {
        description: Some("No content".to_string()),
        content: BTreeMap::new(),
        ..Default::default()
      }),
    )])),
    ..Default::default()
  };

  let result = converter.convert(
    "delete_item",
    "deleteItem",
    &Method::DELETE,
    "/items/{id}",
    OperationKind::Http,
    &operation,
    &mut usage,
    &mut cache,
  )?;

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
  // Define two schema types that will be used in the oneOf
  let model_params_schema = ObjectSchema {
    schema_type: Some(SchemaTypeSet::Single(SchemaType::Object)),
    properties: BTreeMap::from([(
      "model".to_string(),
      ObjectOrReference::Object(ObjectSchema {
        schema_type: Some(SchemaTypeSet::Single(SchemaType::String)),
        ..Default::default()
      }),
    )]),
    ..Default::default()
  };
  let agent_params_schema = ObjectSchema {
    schema_type: Some(SchemaTypeSet::Single(SchemaType::Object)),
    properties: BTreeMap::from([(
      "agent".to_string(),
      ObjectOrReference::Object(ObjectSchema {
        schema_type: Some(SchemaTypeSet::Single(SchemaType::String)),
        ..Default::default()
      }),
    )]),
    ..Default::default()
  };

  let (converter, mut usage, mut cache) = setup_converter(BTreeMap::from([
    ("CreateModelParams".to_string(), model_params_schema),
    ("CreateAgentParams".to_string(), agent_params_schema),
  ]));

  // Create an operation with a oneOf request body
  let operation = Operation {
    request_body: Some(ObjectOrReference::Object(RequestBody {
      required: Some(true),
      content: BTreeMap::from([(
        "application/json".to_string(),
        MediaType {
          schema: Some(ObjectOrReference::Object(ObjectSchema {
            one_of: vec![
              ObjectOrReference::Ref {
                ref_path: "#/components/schemas/CreateModelParams".to_string(),
                summary: None,
                description: None,
              },
              ObjectOrReference::Ref {
                ref_path: "#/components/schemas/CreateAgentParams".to_string(),
                summary: None,
                description: None,
              },
            ],
            ..Default::default()
          })),
          ..Default::default()
        },
      )]),
      ..Default::default()
    })),
    ..Default::default()
  };

  let result = converter.convert(
    "create_interaction",
    "createInteraction",
    &Method::POST,
    "/interactions",
    OperationKind::Http,
    &operation,
    &mut usage,
    &mut cache,
  )?;

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

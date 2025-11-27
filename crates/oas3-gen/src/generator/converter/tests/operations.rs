use std::collections::{BTreeMap, BTreeMap as ResponseMap};

use http::Method;
use oas3::spec::{
  MediaType, ObjectOrReference, ObjectSchema, Operation, Parameter, ParameterIn, RequestBody, Response, SchemaType,
  SchemaTypeSet,
};

use crate::{
  generator::{
    ast::{
      ContentCategory, FieldNameToken, PathSegment, RustPrimitive, RustType, StructDef, StructMethodKind, StructToken,
    },
    converter::{
      FieldOptionalityPolicy, SchemaConverter, TypeUsageRecorder, cache::SharedSchemaCache,
      operations::OperationConverter,
    },
  },
  tests::common::{create_test_graph, default_config},
};

fn setup_converter(
  schemas: BTreeMap<String, ObjectSchema>,
) -> (OperationConverter<'static>, TypeUsageRecorder, SharedSchemaCache) {
  let graph = Box::leak(Box::new(create_test_graph(schemas)));
  let spec = Box::leak(Box::new(graph.spec().clone()));
  let schema_converter = Box::leak(Box::new(SchemaConverter::new(
    graph,
    FieldOptionalityPolicy::standard(),
    default_config(),
  )));
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

  let (types, info) = converter.convert(
    "my_op",
    "myOp",
    &Method::GET,
    "/test",
    &operation,
    &mut usage,
    &mut cache,
  )?;

  assert!(types.is_empty(), "Should generate no new types");
  assert_eq!(info.operation_id, "MyOp");
  assert!(info.request_type.is_none(), "Should have no request type");
  assert!(info.response_type.is_none(), "Should have no response type");
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

  let (types, info) = converter.convert(
    "get_user",
    "getUser",
    &Method::GET,
    "/users/{userId}",
    &operation,
    &mut usage,
    &mut cache,
  )?;

  assert_eq!(types.len(), 1, "Should generate one request struct");
  let request_type_name = info
    .request_type
    .as_ref()
    .map(StructToken::as_str)
    .expect("Request type should exist");
  assert_eq!(request_type_name, "GetUserRequest");

  let request_struct = extract_request_struct(&types, request_type_name);
  assert_eq!(request_struct.fields.len(), 1);
  assert_eq!(request_struct.fields[0].name, "user_id");

  let render_method = request_struct
    .methods
    .iter()
    .find(|m| m.name == "render_path")
    .expect("render_path method not found");
  let StructMethodKind::RenderPath { segments, .. } = &render_method.kind else {
    panic!("Expected RenderPath method kind");
  };
  assert_eq!(segments.len(), 2);
  assert!(matches!(&segments[0], PathSegment::Literal(s) if s == "/users/"));
  assert!(matches!(&segments[1], PathSegment::Parameter { field } if field == &FieldNameToken::new("user_id")));
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

  let (types, _) = converter.convert(
    "create_user",
    "createUser",
    &Method::POST,
    "/users",
    &operation,
    &mut usage,
    &mut cache,
  )?;

  assert_eq!(types.len(), 2, "Should generate Request struct and RequestBody alias");
  assert!(
    types
      .iter()
      .any(|t| matches!(t, RustType::TypeAlias(a) if a.name == "CreateUserRequestBody"))
  );
  assert!(
    types
      .iter()
      .any(|t| matches!(t, RustType::Struct(s) if s.name == "CreateUserRequest"))
  );
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

  let (_, info) = converter.convert(
    "get_user",
    "getUser",
    &Method::GET,
    "/user",
    &operation,
    &mut usage,
    &mut cache,
  )?;

  assert_eq!(info.response_type.as_deref(), Some("User"));
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

    let (types, info) = converter.convert(
      &snake_op_id,
      op_id,
      &Method::GET,
      &path,
      &operation,
      &mut usage,
      &mut cache,
    )?;

    assert_eq!(types.len(), 1, "Should generate one request struct for {op_id}");
    let request_type_name = info
      .request_type
      .as_ref()
      .map(StructToken::as_str)
      .expect("Request type should exist");
    assert_eq!(
      request_type_name, *expected_struct,
      "Request struct name mismatch for {op_id}"
    );

    let request_struct = extract_request_struct(&types, request_type_name);
    assert_eq!(request_struct.fields.len(), 1, "Should have one field for {op_id}");
    assert_eq!(
      request_struct.fields[0].rust_type.to_rust_type(),
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

  let (types, _) = converter.convert(
    "get_user_post",
    "getUserPost",
    &Method::GET,
    "/users/{userId}/posts/{postId}",
    &operation,
    &mut usage,
    &mut cache,
  )?;

  let request_struct = extract_request_struct(&types, "GetUserPostRequest");
  assert_eq!(request_struct.fields.len(), 2);
  assert_eq!(request_struct.fields[0].name, "user_id");
  assert_eq!(request_struct.fields[0].rust_type.to_rust_type(), "i64");
  assert_eq!(request_struct.fields[1].name, "post_id");
  assert_eq!(request_struct.fields[1].rust_type.to_rust_type(), "String");

  let render_method = request_struct
    .methods
    .iter()
    .find(|m| m.name == "render_path")
    .expect("render_path method not found");
  let StructMethodKind::RenderPath { segments, .. } = &render_method.kind else {
    panic!("Expected RenderPath method kind");
  };
  assert_eq!(segments.len(), 4);
  assert!(matches!(&segments[0], PathSegment::Literal(s) if s == "/users/"));
  assert!(matches!(&segments[1], PathSegment::Parameter { field } if field == &FieldNameToken::new("user_id")));
  assert!(matches!(&segments[2], PathSegment::Literal(s) if s == "/posts/"));
  assert!(matches!(&segments[3], PathSegment::Parameter { field } if field == &FieldNameToken::new("post_id")));
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

    let (types, info) = converter.convert(
      "download_file",
      "downloadFile",
      &Method::GET,
      "/files/download",
      &operation,
      &mut usage,
      &mut cache,
    )?;

    let response_enum = types
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
      ok_variant.content_category, expected_category,
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
      info.response_enum.is_some(),
      "response_enum should be set for {content_type}"
    );

    let error_variant = response_enum
      .variants
      .iter()
      .find(|v| v.variant_name == "ClientError")
      .unwrap_or_else(|| panic!("ClientError variant not found for {content_type}"));

    assert_eq!(
      error_variant.content_category,
      ContentCategory::Json,
      "error response should be Json for {content_type}"
    );

    assert!(
      error_variant.schema_type.is_some(),
      "error response should preserve schema type for {content_type}"
    );
  }
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

  let (types, _) = converter.convert(
    "get_item",
    "getItem",
    &Method::GET,
    "/items",
    &operation,
    &mut usage,
    &mut cache,
  )?;

  let response_enum = types
    .iter()
    .find_map(|t| match t {
      RustType::ResponseEnum(e) if e.name == "GetItemResponse" => Some(e),
      _ => None,
    })
    .expect("Response enum not found");

  let default_variant = response_enum.variants.iter().find(|v| v.variant_name == "Unknown");

  assert!(default_variant.is_some(), "Default variant should be added");
  assert!(
    default_variant.unwrap().schema_type.is_none(),
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

  let (types, _) = converter.convert(
    "get_item",
    "getItem",
    &Method::GET,
    "/items",
    &operation,
    &mut usage,
    &mut cache,
  )?;

  let response_enum = types
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
    default_variant.unwrap().schema_type.is_some(),
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

  let (types, _) = converter.convert(
    "get_count",
    "getCount",
    &Method::GET,
    "/count",
    &operation,
    &mut usage,
    &mut cache,
  )?;

  let response_enum = types
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

  assert!(ok_variant.schema_type.is_some(), "Should have schema type");
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

  let (types, _) = converter.convert(
    "delete_item",
    "deleteItem",
    &Method::DELETE,
    "/items/{id}",
    &operation,
    &mut usage,
    &mut cache,
  )?;

  let response_enum = types
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
    no_content_variant.schema_type.is_none(),
    "No content response should have no schema type"
  );
  Ok(())
}

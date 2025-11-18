use std::collections::{BTreeMap, BTreeMap as ResponseMap};

use http::Method;
use oas3::spec::{
  MediaType, ObjectOrReference, ObjectSchema, Operation, Parameter, ParameterIn, RequestBody, Response, SchemaType,
  SchemaTypeSet,
};

use super::common::create_test_graph;
use crate::generator::{
  ast::{PathSegment, RustType},
  converter::{
    ConversionResult, FieldOptionalityPolicy, SchemaConverter, TypeUsageRecorder, cache::SharedSchemaCache,
    operations::OperationConverter,
  },
};

#[test]
fn test_basic_get_operation() -> ConversionResult<()> {
  let graph = create_test_graph(BTreeMap::new());
  let spec = graph.spec().clone();
  let schema_converter = SchemaConverter::new(&graph, FieldOptionalityPolicy::standard(), false);
  let converter = OperationConverter::new(&schema_converter, &spec);

  let operation = Operation::default();
  let mut usage = TypeUsageRecorder::new();
  let mut cache = SharedSchemaCache::new();
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
fn test_operation_with_path_parameter() -> ConversionResult<()> {
  let graph = create_test_graph(BTreeMap::new());
  let spec = graph.spec().clone();
  let schema_converter = SchemaConverter::new(&graph, FieldOptionalityPolicy::standard(), false);
  let converter = OperationConverter::new(&schema_converter, &spec);
  let mut operation = Operation::default();
  operation.parameters.push(ObjectOrReference::Object(Parameter {
    name: "userId".to_string(),
    location: ParameterIn::Path,
    required: Some(true),
    schema: Some(ObjectOrReference::Object(ObjectSchema {
      schema_type: Some(SchemaTypeSet::Single(SchemaType::String)),
      ..Default::default()
    })),
    description: None,
    deprecated: None,
    allow_empty_value: None,
    allow_reserved: None,
    explode: None,
    style: None,
    content: Option::default(),
    example: None,
    examples: BTreeMap::default(),
    extensions: BTreeMap::default(),
  }));

  let mut usage = TypeUsageRecorder::new();
  let mut cache = SharedSchemaCache::new();
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
  let request_type_name = info.request_type.as_deref().expect("Request type should exist");
  assert_eq!(request_type_name, "GetUserRequest");

  let request_struct = types
    .iter()
    .find_map(|t| match t {
      RustType::Struct(s) if s.name == request_type_name => Some(s),
      _ => None,
    })
    .expect("Request struct not found");

  assert_eq!(request_struct.fields.len(), 1);
  assert_eq!(request_struct.fields[0].name, "user_id");

  let render_method = request_struct
    .methods
    .iter()
    .find(|m| m.name == "render_path")
    .expect("render_path method not found");
  let crate::generator::ast::StructMethodKind::RenderPath { segments, .. } = &render_method.kind else {
    panic!("Expected RenderPath method kind");
  };
  assert_eq!(segments.len(), 2);
  assert!(matches!(&segments[0], PathSegment::Literal(s) if s == "/users/"));
  assert!(matches!(&segments[1], PathSegment::Parameter { field } if field == "user_id"));
  Ok(())
}

#[test]
fn test_operation_with_request_body_ref() -> ConversionResult<()> {
  let user_schema = ObjectSchema {
    schema_type: Some(SchemaTypeSet::Single(SchemaType::Object)),
    ..Default::default()
  };
  let graph = create_test_graph(BTreeMap::from([("User".to_string(), user_schema)]));
  let spec = graph.spec().clone();
  let schema_converter = SchemaConverter::new(&graph, FieldOptionalityPolicy::standard(), false);
  let converter = OperationConverter::new(&schema_converter, &spec);

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

  let mut usage = TypeUsageRecorder::new();
  let mut cache = SharedSchemaCache::new();
  let (types, info) = converter.convert(
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
  assert_eq!(info.request_body_types, vec!["User", "CreateUserRequestBody"]);
  Ok(())
}

#[test]
fn test_operation_with_response_type() -> ConversionResult<()> {
  let user_schema = ObjectSchema {
    schema_type: Some(SchemaTypeSet::Single(SchemaType::Object)),
    ..Default::default()
  };
  let graph = create_test_graph(BTreeMap::from([("User".to_string(), user_schema)]));
  let spec = graph.spec().clone();
  let schema_converter = SchemaConverter::new(&graph, FieldOptionalityPolicy::standard(), false);
  let converter = OperationConverter::new(&schema_converter, &spec);

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

  let mut usage = TypeUsageRecorder::new();
  let mut cache = SharedSchemaCache::new();
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
fn test_operation_with_integer_path_parameter() -> ConversionResult<()> {
  let graph = create_test_graph(BTreeMap::new());
  let spec = graph.spec().clone();
  let schema_converter = SchemaConverter::new(&graph, FieldOptionalityPolicy::standard(), false);
  let converter = OperationConverter::new(&schema_converter, &spec);
  let mut operation = Operation::default();
  operation.parameters.push(ObjectOrReference::Object(Parameter {
    name: "id".to_string(),
    location: ParameterIn::Path,
    required: Some(true),
    schema: Some(ObjectOrReference::Object(ObjectSchema {
      schema_type: Some(SchemaTypeSet::Single(SchemaType::Integer)),
      format: Some("int64".to_string()),
      ..Default::default()
    })),
    description: None,
    deprecated: None,
    allow_empty_value: None,
    allow_reserved: None,
    explode: None,
    style: None,
    content: Option::default(),
    example: None,
    examples: BTreeMap::default(),
    extensions: BTreeMap::default(),
  }));

  let mut usage = TypeUsageRecorder::new();
  let mut cache = SharedSchemaCache::new();
  let (types, info) = converter.convert(
    "get_by_id",
    "getById",
    &Method::GET,
    "/items/{id}",
    &operation,
    &mut usage,
    &mut cache,
  )?;

  assert_eq!(types.len(), 1);
  let request_type_name = info.request_type.as_deref().expect("Request type should exist");

  let request_struct = types
    .iter()
    .find_map(|t| match t {
      RustType::Struct(s) if s.name == request_type_name => Some(s),
      _ => None,
    })
    .expect("Request struct not found");

  assert_eq!(request_struct.fields.len(), 1);
  assert_eq!(request_struct.fields[0].name, "id");
  assert_eq!(request_struct.fields[0].rust_type.to_rust_type(), "i64");
  Ok(())
}

#[test]
fn test_operation_with_int32_path_parameter() -> ConversionResult<()> {
  let graph = create_test_graph(BTreeMap::new());
  let spec = graph.spec().clone();
  let schema_converter = SchemaConverter::new(&graph, FieldOptionalityPolicy::standard(), false);
  let converter = OperationConverter::new(&schema_converter, &spec);
  let mut operation = Operation::default();
  operation.parameters.push(ObjectOrReference::Object(Parameter {
    name: "count".to_string(),
    location: ParameterIn::Path,
    required: Some(true),
    schema: Some(ObjectOrReference::Object(ObjectSchema {
      schema_type: Some(SchemaTypeSet::Single(SchemaType::Integer)),
      format: Some("int32".to_string()),
      ..Default::default()
    })),
    description: None,
    deprecated: None,
    allow_empty_value: None,
    allow_reserved: None,
    explode: None,
    style: None,
    content: Option::default(),
    example: None,
    examples: BTreeMap::default(),
    extensions: BTreeMap::default(),
  }));

  let mut usage = TypeUsageRecorder::new();
  let mut cache = SharedSchemaCache::new();
  let (types, _) = converter.convert(
    "get_by_count",
    "getByCount",
    &Method::GET,
    "/items/{count}",
    &operation,
    &mut usage,
    &mut cache,
  )?;

  let request_struct = types
    .iter()
    .find_map(|t| match t {
      RustType::Struct(s) => Some(s),
      _ => None,
    })
    .expect("Request struct not found");

  assert_eq!(request_struct.fields[0].name, "count");
  assert_eq!(request_struct.fields[0].rust_type.to_rust_type(), "i32");
  Ok(())
}

#[test]
fn test_operation_with_number_path_parameter() -> ConversionResult<()> {
  let graph = create_test_graph(BTreeMap::new());
  let spec = graph.spec().clone();
  let schema_converter = SchemaConverter::new(&graph, FieldOptionalityPolicy::standard(), false);
  let converter = OperationConverter::new(&schema_converter, &spec);
  let mut operation = Operation::default();
  operation.parameters.push(ObjectOrReference::Object(Parameter {
    name: "amount".to_string(),
    location: ParameterIn::Path,
    required: Some(true),
    schema: Some(ObjectOrReference::Object(ObjectSchema {
      schema_type: Some(SchemaTypeSet::Single(SchemaType::Number)),
      format: Some("double".to_string()),
      ..Default::default()
    })),
    description: None,
    deprecated: None,
    allow_empty_value: None,
    allow_reserved: None,
    explode: None,
    style: None,
    content: Option::default(),
    example: None,
    examples: BTreeMap::default(),
    extensions: BTreeMap::default(),
  }));

  let mut usage = TypeUsageRecorder::new();
  let mut cache = SharedSchemaCache::new();
  let (types, _) = converter.convert(
    "get_by_amount",
    "getByAmount",
    &Method::GET,
    "/items/{amount}",
    &operation,
    &mut usage,
    &mut cache,
  )?;

  let request_struct = types
    .iter()
    .find_map(|t| match t {
      RustType::Struct(s) => Some(s),
      _ => None,
    })
    .expect("Request struct not found");

  assert_eq!(request_struct.fields[0].name, "amount");
  assert_eq!(request_struct.fields[0].rust_type.to_rust_type(), "f64");
  Ok(())
}

#[test]
fn test_operation_with_boolean_path_parameter() -> ConversionResult<()> {
  let graph = create_test_graph(BTreeMap::new());
  let spec = graph.spec().clone();
  let schema_converter = SchemaConverter::new(&graph, FieldOptionalityPolicy::standard(), false);
  let converter = OperationConverter::new(&schema_converter, &spec);
  let mut operation = Operation::default();
  operation.parameters.push(ObjectOrReference::Object(Parameter {
    name: "active".to_string(),
    location: ParameterIn::Path,
    required: Some(true),
    schema: Some(ObjectOrReference::Object(ObjectSchema {
      schema_type: Some(SchemaTypeSet::Single(SchemaType::Boolean)),
      ..Default::default()
    })),
    description: None,
    deprecated: None,
    allow_empty_value: None,
    allow_reserved: None,
    explode: None,
    style: None,
    content: Option::default(),
    example: None,
    examples: BTreeMap::default(),
    extensions: BTreeMap::default(),
  }));

  let mut usage = TypeUsageRecorder::new();
  let mut cache = SharedSchemaCache::new();
  let (types, _) = converter.convert(
    "get_by_active",
    "getByActive",
    &Method::GET,
    "/items/{active}",
    &operation,
    &mut usage,
    &mut cache,
  )?;

  let request_struct = types
    .iter()
    .find_map(|t| match t {
      RustType::Struct(s) => Some(s),
      _ => None,
    })
    .expect("Request struct not found");

  assert_eq!(request_struct.fields[0].name, "active");
  assert_eq!(request_struct.fields[0].rust_type.to_rust_type(), "bool");
  Ok(())
}

#[test]
fn test_operation_with_uuid_path_parameter() -> ConversionResult<()> {
  let graph = create_test_graph(BTreeMap::new());
  let spec = graph.spec().clone();
  let schema_converter = SchemaConverter::new(&graph, FieldOptionalityPolicy::standard(), false);
  let converter = OperationConverter::new(&schema_converter, &spec);
  let mut operation = Operation::default();
  operation.parameters.push(ObjectOrReference::Object(Parameter {
    name: "uuid".to_string(),
    location: ParameterIn::Path,
    required: Some(true),
    schema: Some(ObjectOrReference::Object(ObjectSchema {
      schema_type: Some(SchemaTypeSet::Single(SchemaType::String)),
      format: Some("uuid".to_string()),
      ..Default::default()
    })),
    description: None,
    deprecated: None,
    allow_empty_value: None,
    allow_reserved: None,
    explode: None,
    style: None,
    content: Option::default(),
    example: None,
    examples: BTreeMap::default(),
    extensions: BTreeMap::default(),
  }));

  let mut usage = TypeUsageRecorder::new();
  let mut cache = SharedSchemaCache::new();
  let (types, _) = converter.convert(
    "get_by_uuid",
    "getByUuid",
    &Method::GET,
    "/items/{uuid}",
    &operation,
    &mut usage,
    &mut cache,
  )?;

  let request_struct = types
    .iter()
    .find_map(|t| match t {
      RustType::Struct(s) => Some(s),
      _ => None,
    })
    .expect("Request struct not found");

  assert_eq!(request_struct.fields[0].name, "uuid");
  assert_eq!(request_struct.fields[0].rust_type.to_rust_type(), "uuid::Uuid");
  Ok(())
}

#[test]
fn test_operation_with_date_time_path_parameter() -> ConversionResult<()> {
  let graph = create_test_graph(BTreeMap::new());
  let spec = graph.spec().clone();
  let schema_converter = SchemaConverter::new(&graph, FieldOptionalityPolicy::standard(), false);
  let converter = OperationConverter::new(&schema_converter, &spec);
  let mut operation = Operation::default();
  operation.parameters.push(ObjectOrReference::Object(Parameter {
    name: "timestamp".to_string(),
    location: ParameterIn::Path,
    required: Some(true),
    schema: Some(ObjectOrReference::Object(ObjectSchema {
      schema_type: Some(SchemaTypeSet::Single(SchemaType::String)),
      format: Some("date-time".to_string()),
      ..Default::default()
    })),
    description: None,
    deprecated: None,
    allow_empty_value: None,
    allow_reserved: None,
    explode: None,
    style: None,
    content: Option::default(),
    example: None,
    examples: BTreeMap::default(),
    extensions: BTreeMap::default(),
  }));

  let mut usage = TypeUsageRecorder::new();
  let mut cache = SharedSchemaCache::new();
  let (types, _) = converter.convert(
    "get_by_timestamp",
    "getByTimestamp",
    &Method::GET,
    "/items/{timestamp}",
    &operation,
    &mut usage,
    &mut cache,
  )?;

  let request_struct = types
    .iter()
    .find_map(|t| match t {
      RustType::Struct(s) => Some(s),
      _ => None,
    })
    .expect("Request struct not found");

  assert_eq!(request_struct.fields[0].name, "timestamp");
  assert_eq!(
    request_struct.fields[0].rust_type.to_rust_type(),
    "chrono::DateTime<chrono::Utc>"
  );
  Ok(())
}

#[test]
fn test_operation_with_multiple_path_parameters() -> ConversionResult<()> {
  let graph = create_test_graph(BTreeMap::new());
  let spec = graph.spec().clone();
  let schema_converter = SchemaConverter::new(&graph, FieldOptionalityPolicy::standard(), false);
  let converter = OperationConverter::new(&schema_converter, &spec);
  let mut operation = Operation::default();
  operation.parameters.push(ObjectOrReference::Object(Parameter {
    name: "userId".to_string(),
    location: ParameterIn::Path,
    required: Some(true),
    schema: Some(ObjectOrReference::Object(ObjectSchema {
      schema_type: Some(SchemaTypeSet::Single(SchemaType::Integer)),
      format: Some("int64".to_string()),
      ..Default::default()
    })),
    description: None,
    deprecated: None,
    allow_empty_value: None,
    allow_reserved: None,
    explode: None,
    style: None,
    content: Option::default(),
    example: None,
    examples: BTreeMap::default(),
    extensions: BTreeMap::default(),
  }));
  operation.parameters.push(ObjectOrReference::Object(Parameter {
    name: "postId".to_string(),
    location: ParameterIn::Path,
    required: Some(true),
    schema: Some(ObjectOrReference::Object(ObjectSchema {
      schema_type: Some(SchemaTypeSet::Single(SchemaType::String)),
      ..Default::default()
    })),
    description: None,
    deprecated: None,
    allow_empty_value: None,
    allow_reserved: None,
    explode: None,
    style: None,
    content: Option::default(),
    example: None,
    examples: BTreeMap::default(),
    extensions: BTreeMap::default(),
  }));

  let mut usage = TypeUsageRecorder::new();
  let mut cache = SharedSchemaCache::new();
  let (types, _info) = converter.convert(
    "get_user_post",
    "getUserPost",
    &Method::GET,
    "/users/{userId}/posts/{postId}",
    &operation,
    &mut usage,
    &mut cache,
  )?;

  let request_struct = types
    .iter()
    .find_map(|t| match t {
      RustType::Struct(s) => Some(s),
      _ => None,
    })
    .expect("Request struct not found");

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
  let crate::generator::ast::StructMethodKind::RenderPath { segments, .. } = &render_method.kind else {
    panic!("Expected RenderPath method kind");
  };
  assert_eq!(segments.len(), 4);
  assert!(matches!(&segments[0], PathSegment::Literal(s) if s == "/users/"));
  assert!(matches!(&segments[1], PathSegment::Parameter { field } if field == "user_id"));
  assert!(matches!(&segments[2], PathSegment::Literal(s) if s == "/posts/"));
  assert!(matches!(&segments[3], PathSegment::Parameter { field } if field == "post_id"));
  Ok(())
}

use std::collections::{BTreeMap, BTreeMap as ResponseMap};

use oas3::spec::{
  MediaType, ObjectOrReference, ObjectSchema, Operation, Parameter, ParameterIn, RequestBody, Response, SchemaType,
  SchemaTypeSet,
};

use super::common::create_test_graph;
use crate::generator::{
  ast::{PathSegment, RustType},
  converter::{SchemaConverter, error::ConversionResult, operations::OperationConverter},
};

#[test]
fn test_basic_get_operation() -> ConversionResult<()> {
  let graph = create_test_graph(BTreeMap::new());
  let spec = graph.spec().clone();
  let schema_converter = SchemaConverter::new(&graph);
  let converter = OperationConverter::new(&schema_converter, &spec);

  let operation = Operation::default();
  let (types, info) = converter.convert("myOp", "GET", "/test", &operation)?;

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
  let schema_converter = SchemaConverter::new(&graph);
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

  let (types, info) = converter.convert("getUser", "GET", "/users/{userId}", &operation)?;

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
  let schema_converter = SchemaConverter::new(&graph);
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

  let (types, info) = converter.convert("createUser", "POST", "/users", &operation)?;

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
  let schema_converter = SchemaConverter::new(&graph);
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

  let (_, info) = converter.convert("getUser", "GET", "/user", &operation)?;

  assert_eq!(info.response_type.as_deref(), Some("User"));
  Ok(())
}

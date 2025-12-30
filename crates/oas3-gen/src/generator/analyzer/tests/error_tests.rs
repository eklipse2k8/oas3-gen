use std::collections::BTreeMap;

use http::Method;

use crate::generator::{
  analyzer::{AnalysisResult, TypeAnalyzer},
  ast::{
    EnumDef, EnumToken, FieldDef, OperationInfo, OperationKind, ParsedPath, PathSegment, ResponseMediaType,
    RustPrimitive, RustType, StructDef, StructKind, StructToken, TypeRef, VariantContent, VariantDef,
    tokens::FieldNameToken,
  },
};

fn create_test_struct(name: &str, field_type: RustPrimitive) -> RustType {
  RustType::Struct(StructDef {
    name: StructToken::new(name),
    fields: vec![
      FieldDef::builder()
        .name(FieldNameToken::new("field"))
        .rust_type(TypeRef::new(field_type))
        .build(),
    ],
    kind: StructKind::Schema,
    ..Default::default()
  })
}

fn create_test_enum(name: &str, has_tuple_variant: bool) -> RustType {
  let variants = if has_tuple_variant {
    vec![VariantDef {
      name: "Error".into(),
      content: VariantContent::Tuple(vec![TypeRef::new(RustPrimitive::String)]),
      ..Default::default()
    }]
  } else {
    vec![VariantDef {
      name: "Unit".into(),
      content: VariantContent::Unit,
      ..Default::default()
    }]
  };

  RustType::Enum(EnumDef {
    name: EnumToken::new(name),
    variants,
    ..Default::default()
  })
}

fn create_operation_info(id: &str, success_types: Vec<String>, error_types: Vec<String>) -> OperationInfo {
  OperationInfo {
    stable_id: id.to_string(),
    operation_id: id.to_string(),
    method: Method::GET,
    path: ParsedPath(vec![PathSegment::Literal("test".to_string())]),
    path_template: "/test".to_string(),
    kind: OperationKind::Http,
    summary: None,
    description: None,
    request_type: None,
    response_type: None,
    response_enum: None,
    response_media_types: vec![ResponseMediaType::new("application/json")],
    success_response_types: success_types,
    error_response_types: error_types,
    warnings: vec![],
    parameters: vec![],
    body: None,
  }
}

fn analyze_errors(mut types: Vec<RustType>, mut operations: Vec<OperationInfo>) -> AnalysisResult {
  let analyzer = TypeAnalyzer::new(&mut types, &mut operations, BTreeMap::new());
  analyzer.analyze()
}

#[test]
fn test_build_error_schema_set_empty_operations() {
  let result = analyze_errors(vec![], vec![]);
  assert!(result.error_schemas.is_empty());
}

#[test]
fn test_build_error_schema_set_empty_types() {
  let operations = vec![create_operation_info("test", vec![], vec!["ErrorType".to_string()])];
  let result = analyze_errors(vec![], operations);

  assert_eq!(result.error_schemas.len(), 1);
  assert!(result.error_schemas.contains(&EnumToken::new("ErrorType")));
}

#[test]
fn test_build_error_schema_set_only_error_types() {
  let operations = vec![
    create_operation_info("op1", vec![], vec!["Error1".to_string()]),
    create_operation_info("op2", vec![], vec!["Error2".to_string()]),
  ];
  let types = vec![
    create_test_struct("Error1", RustPrimitive::String),
    create_test_struct("Error2", RustPrimitive::String),
  ];

  let result = analyze_errors(types, operations);

  assert_eq!(result.error_schemas.len(), 2);
  assert!(result.error_schemas.contains(&EnumToken::new("Error1")));
  assert!(result.error_schemas.contains(&EnumToken::new("Error2")));
}

#[test]
fn test_build_error_schema_set_excludes_success_types() {
  let operations = vec![
    create_operation_info("op1", vec!["SharedType".to_string()], vec![]),
    create_operation_info("op2", vec![], vec!["SharedType".to_string()]),
  ];
  let types = vec![create_test_struct("SharedType", RustPrimitive::String)];

  let result = analyze_errors(types, operations);

  assert!(
    result.error_schemas.is_empty(),
    "Types used in success responses should be excluded"
  );
}

#[test]
fn test_build_error_schema_set_expands_nested_struct_fields() {
  let operations = vec![create_operation_info("op1", vec![], vec!["RootError".to_string()])];
  let types = vec![
    RustType::Struct(StructDef {
      name: StructToken::new("RootError"),
      fields: vec![
        FieldDef::builder()
          .name(FieldNameToken::new("nested"))
          .rust_type(TypeRef::new(RustPrimitive::Custom("NestedError".into())))
          .build(),
      ],
      kind: StructKind::Schema,
      ..Default::default()
    }),
    create_test_struct("NestedError", RustPrimitive::String),
  ];

  let result = analyze_errors(types, operations);

  assert_eq!(result.error_schemas.len(), 2);
  assert!(result.error_schemas.contains(&EnumToken::new("RootError")));
  assert!(result.error_schemas.contains(&EnumToken::new("NestedError")));
}

#[test]
fn test_build_error_schema_set_expands_enum_tuple_variants() {
  let operations = vec![create_operation_info("op1", vec![], vec!["ErrorEnum".to_string()])];
  let types = vec![
    RustType::Enum(EnumDef {
      name: "ErrorEnum".into(),
      variants: vec![VariantDef {
        name: "Variant".into(),
        content: VariantContent::Tuple(vec![TypeRef::new(RustPrimitive::Custom("InnerError".into()))]),
        ..Default::default()
      }],
      ..Default::default()
    }),
    create_test_struct("InnerError", RustPrimitive::String),
  ];

  let result = analyze_errors(types, operations);

  assert_eq!(result.error_schemas.len(), 2);
  assert!(result.error_schemas.contains(&EnumToken::new("ErrorEnum")));
  assert!(result.error_schemas.contains(&EnumToken::new("InnerError")));
}

#[test]
fn test_build_error_schema_set_skips_unit_enum_variants() {
  let operations = vec![create_operation_info("op1", vec![], vec!["ErrorEnum".to_string()])];
  let types = vec![create_test_enum("ErrorEnum", false)];

  let result = analyze_errors(types, operations);

  assert_eq!(result.error_schemas.len(), 1);
  assert!(result.error_schemas.contains(&EnumToken::new("ErrorEnum")));
}

#[test]
fn test_build_error_schema_set_handles_deep_nesting() {
  let operations = vec![create_operation_info("op1", vec![], vec!["Level1".to_string()])];
  let types = vec![
    RustType::Struct(StructDef {
      name: StructToken::new("Level1"),
      fields: vec![
        FieldDef::builder()
          .name(FieldNameToken::new("nested"))
          .rust_type(TypeRef::new(RustPrimitive::Custom("Level2".into())))
          .build(),
      ],
      kind: StructKind::Schema,
      ..Default::default()
    }),
    RustType::Struct(StructDef {
      name: StructToken::new("Level2"),
      fields: vec![
        FieldDef::builder()
          .name(FieldNameToken::new("nested"))
          .rust_type(TypeRef::new(RustPrimitive::Custom("Level3".into())))
          .build(),
      ],
      kind: StructKind::Schema,
      ..Default::default()
    }),
    create_test_struct("Level3", RustPrimitive::String),
  ];

  let result = analyze_errors(types, operations);

  assert_eq!(result.error_schemas.len(), 3);
  assert!(result.error_schemas.contains(&EnumToken::new("Level1")));
  assert!(result.error_schemas.contains(&EnumToken::new("Level2")));
  assert!(result.error_schemas.contains(&EnumToken::new("Level3")));
}

#[test]
fn test_build_error_schema_set_stops_at_success_types() {
  let operations = vec![
    create_operation_info("op1", vec!["SuccessType".to_string()], vec![]),
    create_operation_info("op2", vec![], vec!["ErrorType".to_string()]),
  ];
  let types = vec![
    RustType::Struct(StructDef {
      name: StructToken::new("ErrorType"),
      fields: vec![
        FieldDef::builder()
          .name(FieldNameToken::new("nested"))
          .rust_type(TypeRef::new(RustPrimitive::Custom("SuccessType".into())))
          .build(),
      ],
      kind: StructKind::Schema,
      ..Default::default()
    }),
    create_test_struct("SuccessType", RustPrimitive::String),
  ];

  let result = analyze_errors(types, operations);

  assert_eq!(result.error_schemas.len(), 1);
  assert!(result.error_schemas.contains(&EnumToken::new("ErrorType")));
  assert!(
    !result.error_schemas.contains(&EnumToken::new("SuccessType")),
    "Should not expand into success types"
  );
}

#[test]
fn test_build_error_schema_set_handles_missing_types() {
  let operations = vec![create_operation_info("op1", vec![], vec!["MissingType".to_string()])];
  let result = analyze_errors(vec![], operations);

  assert_eq!(result.error_schemas.len(), 1);
  assert!(result.error_schemas.contains(&EnumToken::new("MissingType")));
}

#[test]
fn test_build_error_schema_set_handles_circular_references() {
  let operations = vec![create_operation_info("op1", vec![], vec!["CircularA".to_string()])];
  let types = vec![
    RustType::Struct(StructDef {
      name: StructToken::new("CircularA"),
      fields: vec![
        FieldDef::builder()
          .name(FieldNameToken::new("b"))
          .rust_type(TypeRef::new(RustPrimitive::Custom("CircularB".into())))
          .build(),
      ],
      kind: StructKind::Schema,
      ..Default::default()
    }),
    RustType::Struct(StructDef {
      name: StructToken::new("CircularB"),
      fields: vec![
        FieldDef::builder()
          .name(FieldNameToken::new("a"))
          .rust_type(TypeRef::new(RustPrimitive::Custom("CircularA".into())))
          .build(),
      ],
      kind: StructKind::Schema,
      ..Default::default()
    }),
  ];

  let result = analyze_errors(types, operations);

  assert_eq!(result.error_schemas.len(), 2);
  assert!(result.error_schemas.contains(&EnumToken::new("CircularA")));
  assert!(result.error_schemas.contains(&EnumToken::new("CircularB")));
}

#[test]
fn test_build_error_schema_set_ignores_primitive_fields() {
  let operations = vec![create_operation_info(
    "op1",
    vec![],
    vec!["ErrorWithPrimitives".to_string()],
  )];
  let types = vec![RustType::Struct(StructDef {
    name: StructToken::new("ErrorWithPrimitives"),
    fields: vec![
      FieldDef::builder()
        .name(FieldNameToken::new("string_field"))
        .rust_type(TypeRef::new(RustPrimitive::String))
        .build(),
      FieldDef::builder()
        .name(FieldNameToken::new("int_field"))
        .rust_type(TypeRef::new(RustPrimitive::I64))
        .build(),
    ],
    kind: StructKind::Schema,
    ..Default::default()
  })];

  let result = analyze_errors(types, operations);

  assert_eq!(result.error_schemas.len(), 1);
  assert!(result.error_schemas.contains(&EnumToken::new("ErrorWithPrimitives")));
}

#[test]
fn test_build_error_schema_set_multiple_operations_same_error() {
  let operations = vec![
    create_operation_info("op1", vec![], vec!["CommonError".to_string()]),
    create_operation_info("op2", vec![], vec!["CommonError".to_string()]),
    create_operation_info("op3", vec![], vec!["CommonError".to_string()]),
  ];
  let types = vec![create_test_struct("CommonError", RustPrimitive::String)];

  let result = analyze_errors(types, operations);

  assert_eq!(result.error_schemas.len(), 1, "Should deduplicate common errors");
  assert!(result.error_schemas.contains(&EnumToken::new("CommonError")));
}

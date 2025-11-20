use std::collections::BTreeSet;

use http::Method;

use crate::generator::{
  analyzer::ErrorAnalyzer,
  ast::{
    EnumDef, FieldDef, OperationInfo, RustPrimitive, RustType, StructDef, StructKind, TypeRef, VariantContent,
    VariantDef,
  },
};

fn create_test_struct(name: &str, field_type: RustPrimitive) -> RustType {
  RustType::Struct(StructDef {
    name: name.to_string(),
    docs: vec![],
    fields: vec![FieldDef {
      name: "field".to_string(),
      docs: vec![],
      rust_type: TypeRef::new(field_type),
      ..Default::default()
    }],
    derives: BTreeSet::new(),
    serde_attrs: vec![],
    outer_attrs: vec![],
    methods: vec![],
    kind: StructKind::Schema,
  })
}

fn create_test_enum(name: &str, has_tuple_variant: bool) -> RustType {
  let variants = if has_tuple_variant {
    vec![VariantDef {
      name: "Error".to_string(),
      docs: vec![],
      content: VariantContent::Tuple(vec![TypeRef::new(RustPrimitive::String)]),
      serde_attrs: vec![],
      deprecated: false,
    }]
  } else {
    vec![VariantDef {
      name: "Unit".to_string(),
      docs: vec![],
      content: VariantContent::Unit,
      serde_attrs: vec![],
      deprecated: false,
    }]
  };

  RustType::Enum(EnumDef {
    name: name.to_string(),
    docs: vec![],
    variants,
    derives: BTreeSet::new(),
    serde_attrs: vec![],
    outer_attrs: vec![],
    discriminator: None,
    case_insensitive: false,
  })
}

fn create_operation_info(id: &str, success_types: Vec<String>, error_types: Vec<String>) -> OperationInfo {
  OperationInfo {
    stable_id: id.to_string(),
    operation_id: id.to_string(),
    method: Method::GET,
    path: "/test".to_string(),
    summary: None,
    description: None,
    request_type: None,
    response_type: None,
    response_enum: None,
    response_content_type: None,
    request_body_types: vec![],
    success_response_types: success_types,
    error_response_types: error_types,
    warnings: vec![],
    parameters: vec![],
    body: None,
  }
}

#[test]
fn test_build_error_schema_set_empty_operations() {
  let operations_info = vec![];
  let rust_types = vec![];

  let result = ErrorAnalyzer::build_error_schema_set(&operations_info, &rust_types);

  assert!(result.is_empty());
}

#[test]
fn test_build_error_schema_set_empty_types() {
  let operations_info = vec![create_operation_info("test", vec![], vec!["ErrorType".to_string()])];
  let rust_types = vec![];

  let result = ErrorAnalyzer::build_error_schema_set(&operations_info, &rust_types);

  assert_eq!(result.len(), 1);
  assert!(result.contains("ErrorType"));
}

#[test]
fn test_build_error_schema_set_only_error_types() {
  let operations_info = vec![
    create_operation_info("op1", vec![], vec!["Error1".to_string()]),
    create_operation_info("op2", vec![], vec!["Error2".to_string()]),
  ];
  let rust_types = vec![
    create_test_struct("Error1", RustPrimitive::String),
    create_test_struct("Error2", RustPrimitive::String),
  ];

  let result = ErrorAnalyzer::build_error_schema_set(&operations_info, &rust_types);

  assert_eq!(result.len(), 2);
  assert!(result.contains("Error1"));
  assert!(result.contains("Error2"));
}

#[test]
fn test_build_error_schema_set_excludes_success_types() {
  let operations_info = vec![
    create_operation_info("op1", vec!["SharedType".to_string()], vec![]),
    create_operation_info("op2", vec![], vec!["SharedType".to_string()]),
  ];
  let rust_types = vec![create_test_struct("SharedType", RustPrimitive::String)];

  let result = ErrorAnalyzer::build_error_schema_set(&operations_info, &rust_types);

  assert!(result.is_empty(), "Types used in success responses should be excluded");
}

#[test]
fn test_build_error_schema_set_expands_nested_struct_fields() {
  let operations_info = vec![create_operation_info("op1", vec![], vec!["RootError".to_string()])];
  let rust_types = vec![
    RustType::Struct(StructDef {
      name: "RootError".to_string(),
      docs: vec![],
      fields: vec![FieldDef {
        name: "nested".to_string(),
        docs: vec![],
        rust_type: TypeRef::new(RustPrimitive::Custom("NestedError".to_string())),
        ..Default::default()
      }],
      derives: BTreeSet::new(),
      serde_attrs: vec![],
      outer_attrs: vec![],
      methods: vec![],
      kind: StructKind::Schema,
    }),
    create_test_struct("NestedError", RustPrimitive::String),
  ];

  let result = ErrorAnalyzer::build_error_schema_set(&operations_info, &rust_types);

  assert_eq!(result.len(), 2);
  assert!(result.contains("RootError"));
  assert!(result.contains("NestedError"));
}

#[test]
fn test_build_error_schema_set_expands_enum_tuple_variants() {
  let operations_info = vec![create_operation_info("op1", vec![], vec!["ErrorEnum".to_string()])];
  let rust_types = vec![
    RustType::Enum(EnumDef {
      name: "ErrorEnum".to_string(),
      docs: vec![],
      variants: vec![VariantDef {
        name: "Variant".to_string(),
        docs: vec![],
        content: VariantContent::Tuple(vec![TypeRef::new(RustPrimitive::Custom("InnerError".to_string()))]),
        serde_attrs: vec![],
        deprecated: false,
      }],
      derives: BTreeSet::new(),
      serde_attrs: vec![],
      outer_attrs: vec![],
      discriminator: None,
      case_insensitive: false,
    }),
    create_test_struct("InnerError", RustPrimitive::String),
  ];

  let result = ErrorAnalyzer::build_error_schema_set(&operations_info, &rust_types);

  assert_eq!(result.len(), 2);
  assert!(result.contains("ErrorEnum"));
  assert!(result.contains("InnerError"));
}

#[test]
fn test_build_error_schema_set_skips_unit_enum_variants() {
  let operations_info = vec![create_operation_info("op1", vec![], vec!["ErrorEnum".to_string()])];
  let rust_types = vec![create_test_enum("ErrorEnum", false)];

  let result = ErrorAnalyzer::build_error_schema_set(&operations_info, &rust_types);

  assert_eq!(result.len(), 1);
  assert!(result.contains("ErrorEnum"));
}

#[test]
fn test_build_error_schema_set_handles_deep_nesting() {
  let operations_info = vec![create_operation_info("op1", vec![], vec!["Level1".to_string()])];
  let rust_types = vec![
    RustType::Struct(StructDef {
      name: "Level1".to_string(),
      docs: vec![],
      fields: vec![FieldDef {
        name: "nested".to_string(),
        docs: vec![],
        rust_type: TypeRef::new(RustPrimitive::Custom("Level2".to_string())),
        ..Default::default()
      }],
      derives: BTreeSet::new(),
      serde_attrs: vec![],
      outer_attrs: vec![],
      methods: vec![],
      kind: StructKind::Schema,
    }),
    RustType::Struct(StructDef {
      name: "Level2".to_string(),
      docs: vec![],
      fields: vec![FieldDef {
        name: "nested".to_string(),
        docs: vec![],
        rust_type: TypeRef::new(RustPrimitive::Custom("Level3".to_string())),
        ..Default::default()
      }],
      derives: BTreeSet::new(),
      serde_attrs: vec![],
      outer_attrs: vec![],
      methods: vec![],
      kind: StructKind::Schema,
    }),
    create_test_struct("Level3", RustPrimitive::String),
  ];

  let result = ErrorAnalyzer::build_error_schema_set(&operations_info, &rust_types);

  assert_eq!(result.len(), 3);
  assert!(result.contains("Level1"));
  assert!(result.contains("Level2"));
  assert!(result.contains("Level3"));
}

#[test]
fn test_build_error_schema_set_stops_at_success_types() {
  let operations_info = vec![
    create_operation_info("op1", vec!["SuccessType".to_string()], vec![]),
    create_operation_info("op2", vec![], vec!["ErrorType".to_string()]),
  ];
  let rust_types = vec![
    RustType::Struct(StructDef {
      name: "ErrorType".to_string(),
      docs: vec![],
      fields: vec![FieldDef {
        name: "nested".to_string(),
        docs: vec![],
        rust_type: TypeRef::new(RustPrimitive::Custom("SuccessType".to_string())),
        ..Default::default()
      }],
      derives: BTreeSet::new(),
      serde_attrs: vec![],
      outer_attrs: vec![],
      methods: vec![],
      kind: StructKind::Schema,
    }),
    create_test_struct("SuccessType", RustPrimitive::String),
  ];

  let result = ErrorAnalyzer::build_error_schema_set(&operations_info, &rust_types);

  assert_eq!(result.len(), 1);
  assert!(result.contains("ErrorType"));
  assert!(!result.contains("SuccessType"), "Should not expand into success types");
}

#[test]
fn test_build_error_schema_set_handles_missing_types() {
  let operations_info = vec![create_operation_info("op1", vec![], vec!["MissingType".to_string()])];
  let rust_types = vec![];

  let result = ErrorAnalyzer::build_error_schema_set(&operations_info, &rust_types);

  assert_eq!(result.len(), 1);
  assert!(result.contains("MissingType"));
}

#[test]
fn test_build_error_schema_set_handles_circular_references() {
  let operations_info = vec![create_operation_info("op1", vec![], vec!["CircularA".to_string()])];
  let rust_types = vec![
    RustType::Struct(StructDef {
      name: "CircularA".to_string(),
      docs: vec![],
      fields: vec![FieldDef {
        name: "b".to_string(),
        docs: vec![],
        rust_type: TypeRef::new(RustPrimitive::Custom("CircularB".to_string())),
        ..Default::default()
      }],
      derives: BTreeSet::new(),
      serde_attrs: vec![],
      outer_attrs: vec![],
      methods: vec![],
      kind: StructKind::Schema,
    }),
    RustType::Struct(StructDef {
      name: "CircularB".to_string(),
      docs: vec![],
      fields: vec![FieldDef {
        name: "a".to_string(),
        docs: vec![],
        rust_type: TypeRef::new(RustPrimitive::Custom("CircularA".to_string())),
        ..Default::default()
      }],
      derives: BTreeSet::new(),
      serde_attrs: vec![],
      outer_attrs: vec![],
      methods: vec![],
      kind: StructKind::Schema,
    }),
  ];

  let result = ErrorAnalyzer::build_error_schema_set(&operations_info, &rust_types);

  assert_eq!(result.len(), 2);
  assert!(result.contains("CircularA"));
  assert!(result.contains("CircularB"));
}

#[test]
fn test_build_error_schema_set_ignores_primitive_fields() {
  let operations_info = vec![create_operation_info(
    "op1",
    vec![],
    vec!["ErrorWithPrimitives".to_string()],
  )];
  let rust_types = vec![RustType::Struct(StructDef {
    name: "ErrorWithPrimitives".to_string(),
    docs: vec![],
    fields: vec![
      FieldDef {
        name: "string_field".to_string(),
        docs: vec![],
        rust_type: TypeRef::new(RustPrimitive::String),
        ..Default::default()
      },
      FieldDef {
        name: "int_field".to_string(),
        docs: vec![],
        rust_type: TypeRef::new(RustPrimitive::I64),
        ..Default::default()
      },
    ],
    derives: BTreeSet::new(),
    serde_attrs: vec![],
    outer_attrs: vec![],
    methods: vec![],
    kind: StructKind::Schema,
  })];

  let result = ErrorAnalyzer::build_error_schema_set(&operations_info, &rust_types);

  assert_eq!(result.len(), 1);
  assert!(result.contains("ErrorWithPrimitives"));
}

#[test]
fn test_build_error_schema_set_multiple_operations_same_error() {
  let operations_info = vec![
    create_operation_info("op1", vec![], vec!["CommonError".to_string()]),
    create_operation_info("op2", vec![], vec!["CommonError".to_string()]),
    create_operation_info("op3", vec![], vec!["CommonError".to_string()]),
  ];
  let rust_types = vec![create_test_struct("CommonError", RustPrimitive::String)];

  let result = ErrorAnalyzer::build_error_schema_set(&operations_info, &rust_types);

  assert_eq!(result.len(), 1, "Should deduplicate common errors");
  assert!(result.contains("CommonError"));
}

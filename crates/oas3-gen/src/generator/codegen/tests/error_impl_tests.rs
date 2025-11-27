use std::collections::{BTreeSet, HashSet};

use crate::generator::{
  ast::{
    EnumDef, EnumToken, EnumVariantToken, FieldDef, RustPrimitive, RustType, StructDef, StructKind, TypeRef,
    VariantContent, VariantDef,
  },
  codegen::error_impls,
};

#[test]
fn test_generate_error_struct_impl_with_error_field() {
  let struct_def = StructDef {
    name: "MyError".to_string(),
    docs: vec![],
    fields: vec![FieldDef {
      name: "error".to_string(),
      rust_type: TypeRef::new(RustPrimitive::Custom("InnerError".to_string())),
      ..Default::default()
    }],
    derives: BTreeSet::new(),
    serde_attrs: vec![],
    outer_attrs: vec![],
    methods: vec![],
    kind: StructKind::Schema,
  };

  let result = error_impls::generate_error_impl(&RustType::Struct(struct_def));
  assert!(result.is_some());

  let code = result.unwrap().to_string();
  assert!(code.contains("impl std :: fmt :: Display for MyError"));
  assert!(code.contains("impl std :: error :: Error for MyError"));
  assert!(code.contains("self . error"));
}

#[test]
fn test_generate_error_struct_impl_with_message_field() {
  let struct_def = StructDef {
    name: "MyError".to_string(),
    docs: vec![],
    fields: vec![FieldDef {
      name: "message".to_string(),
      rust_type: TypeRef::new(RustPrimitive::String),
      ..Default::default()
    }],
    derives: BTreeSet::new(),
    serde_attrs: vec![],
    outer_attrs: vec![],
    methods: vec![],
    kind: StructKind::Schema,
  };

  let result = error_impls::generate_error_impl(&RustType::Struct(struct_def));
  assert!(result.is_some());

  let code = result.unwrap().to_string();
  assert!(code.contains("impl std :: fmt :: Display for MyError"));
  assert!(code.contains("impl std :: error :: Error for MyError"));
  assert!(code.contains("self . message"));
}

#[test]
fn test_generate_error_struct_impl_without_error_fields() {
  let struct_def = StructDef {
    name: "MyError".to_string(),
    docs: vec![],
    fields: vec![FieldDef {
      name: "code".to_string(),
      rust_type: TypeRef::new(RustPrimitive::I32),
      ..Default::default()
    }],
    derives: BTreeSet::new(),
    serde_attrs: vec![],
    outer_attrs: vec![],
    methods: vec![],
    kind: StructKind::Schema,
  };

  let result = error_impls::generate_error_impl(&RustType::Struct(struct_def));
  assert!(result.is_none());
}

#[test]
fn test_generate_error_enum_impl_with_tuple_variants() {
  let enum_def = EnumDef {
    name: EnumToken::new("MyError"),
    docs: vec![],
    variants: vec![VariantDef {
      name: EnumVariantToken::new("BadRequest"),
      docs: vec![],
      content: VariantContent::Tuple(vec![TypeRef::new(RustPrimitive::String)]),
      serde_attrs: vec![],
      deprecated: false,
    }],
    derives: BTreeSet::new(),
    serde_attrs: vec![],
    outer_attrs: vec![],
    discriminator: None,
    case_insensitive: false,
    methods: vec![],
  };

  let result = error_impls::generate_error_impl(&RustType::Enum(enum_def));
  assert!(result.is_some());

  let code = result.unwrap().to_string();
  assert!(code.contains("impl std :: fmt :: Display for MyError"));
  assert!(code.contains("impl std :: error :: Error for MyError"));
  assert!(code.contains("Self :: BadRequest (err)"));
}

#[test]
fn test_generate_error_enum_impl_with_unit_variants() {
  let enum_def = EnumDef {
    name: EnumToken::new("MyError"),
    docs: vec![],
    variants: vec![VariantDef {
      name: EnumVariantToken::new("NotFound"),
      docs: vec![],
      content: VariantContent::Unit,
      serde_attrs: vec![],
      deprecated: false,
    }],
    derives: BTreeSet::new(),
    serde_attrs: vec![],
    outer_attrs: vec![],
    discriminator: None,
    case_insensitive: false,
    methods: vec![],
  };

  let result = error_impls::generate_error_impl(&RustType::Enum(enum_def));
  assert!(result.is_none());
}

#[test]
fn test_try_generate_error_impl_for_error_struct() {
  let struct_def = StructDef {
    name: "ApiError".to_string(),
    docs: vec![],
    fields: vec![FieldDef {
      name: "message".to_string(),
      rust_type: TypeRef::new(RustPrimitive::String),
      ..Default::default()
    }],
    derives: BTreeSet::new(),
    serde_attrs: vec![],
    outer_attrs: vec![],
    methods: vec![],
    kind: StructKind::Schema,
  };

  let rust_type = RustType::Struct(struct_def);
  let mut error_schemas = HashSet::new();
  error_schemas.insert(EnumToken::new("ApiError"));

  let result = error_impls::generate_error_impl(&rust_type);
  assert!(result.is_some());
}

#[test]
fn test_try_generate_error_impl_for_error_enum() {
  let enum_def = EnumDef {
    name: EnumToken::new("ApiError"),
    docs: vec![],
    variants: vec![VariantDef {
      name: EnumVariantToken::new("Error"),
      docs: vec![],
      content: VariantContent::Tuple(vec![TypeRef::new(RustPrimitive::String)]),
      serde_attrs: vec![],
      deprecated: false,
    }],
    derives: BTreeSet::new(),
    serde_attrs: vec![],
    outer_attrs: vec![],
    discriminator: None,
    case_insensitive: false,
    methods: vec![],
  };

  let rust_type = RustType::Enum(enum_def);
  let mut error_schemas = HashSet::new();
  error_schemas.insert(EnumToken::new("ApiError"));

  let result = error_impls::generate_error_impl(&rust_type);
  assert!(result.is_some());
}

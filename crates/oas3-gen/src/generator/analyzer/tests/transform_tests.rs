use std::collections::BTreeMap;

use crate::generator::{
  analyzer::{TypeUsage, update_derives_from_usage},
  ast::{
    DeriveTrait, DerivesProvider, EnumDef, EnumToken, EnumVariantToken, FieldDef, RustType, StructDef, StructKind,
    StructToken, TypeRef, ValidationAttribute, VariantContent, VariantDef, tokens::FieldNameToken,
  },
};

fn create_struct(name: &str, kind: StructKind, nullable: bool) -> StructDef {
  StructDef {
    name: StructToken::new(name),
    docs: vec![],
    fields: vec![FieldDef {
      name: FieldNameToken::new("field"),
      rust_type: if nullable {
        TypeRef::new("String").with_option()
      } else {
        TypeRef::new("String")
      },
      validation_attrs: vec![
        ValidationAttribute::Length {
          min: Some(1),
          max: None,
        },
        ValidationAttribute::Regex("regex".to_string()),
      ],
      ..Default::default()
    }],
    serde_attrs: vec![],
    outer_attrs: vec![],
    methods: vec![],
    kind,
    ..Default::default()
  }
}

fn create_enum(name: &str) -> EnumDef {
  EnumDef {
    name: EnumToken::new(name),
    docs: vec![],
    variants: vec![VariantDef {
      name: EnumVariantToken::new("Variant"),
      docs: vec![],
      content: VariantContent::Unit,
      serde_attrs: vec![],
      deprecated: false,
    }],
    discriminator: None,
    serde_attrs: vec![],
    outer_attrs: vec![],
    case_insensitive: false,
    methods: vec![],
    ..Default::default()
  }
}

fn process_struct_helper(def: StructDef, usage: TypeUsage) -> StructDef {
  let mut usage_map = BTreeMap::new();
  usage_map.insert(EnumToken::from(&def.name), usage);
  let mut rust_types = vec![RustType::Struct(def)];
  update_derives_from_usage(&mut rust_types, &usage_map);
  match rust_types.into_iter().next().unwrap() {
    RustType::Struct(d) => d,
    _ => panic!("Expected Struct"),
  }
}

fn process_enum_helper(def: EnumDef, usage: TypeUsage) -> EnumDef {
  let mut usage_map = BTreeMap::new();
  usage_map.insert(def.name.clone(), usage);
  let mut rust_types = vec![RustType::Enum(def)];
  update_derives_from_usage(&mut rust_types, &usage_map);
  match rust_types.into_iter().next().unwrap() {
    RustType::Enum(d) => d,
    _ => panic!("Expected Enum"),
  }
}

const ATTR_SKIP_SERIALIZING_NONE: &str = "oas3_gen_support::skip_serializing_none";

// --- Tests ---

#[test]
fn test_schema_request_only() {
  let def = create_struct("User", StructKind::Schema, true);
  let def = process_struct_helper(def, TypeUsage::RequestOnly);

  assert!(def.derives().contains(&DeriveTrait::Serialize));
  assert!(def.derives().contains(&DeriveTrait::Validate));
  assert!(!def.derives().contains(&DeriveTrait::Deserialize));
  assert!(def.derives().contains(&DeriveTrait::Debug));

  // Check Attributes (Request needs skip_serializing_none if nullable)
  assert!(def.outer_attrs.contains(&ATTR_SKIP_SERIALIZING_NONE.to_string()));

  // Check Validation (Should remain)
  assert!(!def.fields[0].validation_attrs.is_empty());
}

#[test]
fn test_schema_response_only() {
  let def = create_struct("UserResponse", StructKind::Schema, true);
  let def = process_struct_helper(def, TypeUsage::ResponseOnly);

  assert!(def.derives().contains(&DeriveTrait::Deserialize));
  assert!(!def.derives().contains(&DeriveTrait::Serialize));
  assert!(!def.derives().contains(&DeriveTrait::Validate));

  // Check Attributes (Response does NOT need skip_serializing_none)
  assert!(!def.outer_attrs.contains(&ATTR_SKIP_SERIALIZING_NONE.to_string()));

  // Check Validation (Should be stripped)
  assert!(def.fields[0].validation_attrs.is_empty());
}

#[test]
fn test_schema_bidirectional() {
  let def = create_struct("UserDto", StructKind::Schema, true);
  let def = process_struct_helper(def, TypeUsage::Bidirectional);

  assert!(def.derives().contains(&DeriveTrait::Serialize));
  assert!(def.derives().contains(&DeriveTrait::Deserialize));
  assert!(def.derives().contains(&DeriveTrait::Validate));

  // Should have skip attribute
  assert!(def.outer_attrs.contains(&ATTR_SKIP_SERIALIZING_NONE.to_string()));
}

#[test]
fn test_operation_request_special_handling() {
  let def = create_struct("OpReq", StructKind::OperationRequest, true);
  // Even if we say RequestOnly, OperationRequest has specific base derives
  let def = process_struct_helper(def, TypeUsage::RequestOnly);

  // OpRequest always has Validate
  assert!(def.derives().contains(&DeriveTrait::Validate));
  // OpRequest does NOT get Serialize/Deserialize from the standard flow for Schema
  assert!(!def.derives().contains(&DeriveTrait::Serialize));

  // OpRequest explicitly excludes skip_serializing_none
  assert!(!def.outer_attrs.contains(&ATTR_SKIP_SERIALIZING_NONE.to_string()));
}

#[test]
fn test_enum_processing_request_only() {
  let def = create_enum("Status");
  let def = process_enum_helper(def, TypeUsage::RequestOnly);

  assert!(def.derives().contains(&DeriveTrait::Debug)); // Preserved
  assert!(def.derives().contains(&DeriveTrait::Serialize)); // Added
  assert!(!def.derives().contains(&DeriveTrait::Validate)); // Enums don't get Validate
  assert!(!def.derives().contains(&DeriveTrait::Deserialize));
}

#[test]
fn test_enum_processing_response_only() {
  let def = create_enum("StatusResp");
  let def = process_enum_helper(def, TypeUsage::ResponseOnly);

  assert!(def.derives().contains(&DeriveTrait::Debug)); // Preserved
  assert!(def.derives().contains(&DeriveTrait::Deserialize)); // Added
  assert!(!def.derives().contains(&DeriveTrait::Serialize));
}

#[test]
fn test_skip_serializing_none_logic() {
  // Case 1: Not nullable -> No attribute
  let def = create_struct("Strict", StructKind::Schema, false);
  let def = process_struct_helper(def, TypeUsage::RequestOnly);
  assert!(def.outer_attrs.is_empty());

  // Case 2: Nullable + Request -> Attribute
  let def2 = create_struct("Loose", StructKind::Schema, true);
  let def2 = process_struct_helper(def2, TypeUsage::RequestOnly);
  assert!(def2.outer_attrs.contains(&ATTR_SKIP_SERIALIZING_NONE.to_string()));
}

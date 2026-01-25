use std::collections::BTreeMap;

use crate::generator::{
  ast::{
    DeriveTrait, DerivesProvider, EnumDef, EnumToken, EnumVariantToken, FieldDef, OuterAttr, RustType, StructDef,
    StructKind, StructToken, TypeRef, ValidationAttribute, VariantContent, VariantDef, tokens::FieldNameToken,
  },
  postprocess::{
    serde_usage::TypeUsage,
    tests::{postprocess_types_for_server, postprocess_types_with_usage},
  },
};

fn create_struct(name: &str, kind: StructKind, nullable: bool) -> StructDef {
  StructDef {
    name: StructToken::new(name),
    fields: vec![
      FieldDef::builder()
        .name(FieldNameToken::new("field"))
        .rust_type(if nullable {
          TypeRef::new("String").with_option()
        } else {
          TypeRef::new("String")
        })
        .validation_attrs(vec![
          ValidationAttribute::Length {
            min: Some(1),
            max: None,
          },
          ValidationAttribute::Regex("regex".to_string()),
        ])
        .build(),
    ],
    kind,
    ..Default::default()
  }
}

fn create_enum(name: &str) -> EnumDef {
  EnumDef {
    name: EnumToken::new(name),
    variants: vec![
      VariantDef::builder()
        .name(EnumVariantToken::new("Variant"))
        .content(VariantContent::Unit)
        .build(),
    ],
    ..Default::default()
  }
}

fn usage_flags(usage: TypeUsage) -> (bool, bool) {
  match usage {
    TypeUsage::RequestOnly => (true, false),
    TypeUsage::ResponseOnly => (false, true),
    TypeUsage::Bidirectional => (true, true),
  }
}

fn process_struct_helper(def: StructDef, usage: TypeUsage) -> StructDef {
  let name = EnumToken::from(&def.name);
  let seeds = BTreeMap::from([(name, usage_flags(usage))]);
  let types = postprocess_types_with_usage(vec![RustType::Struct(def)], seeds);
  match types.into_iter().next().unwrap() {
    RustType::Struct(d) => d,
    _ => panic!("Expected Struct"),
  }
}

fn process_enum_helper(def: EnumDef, usage: TypeUsage) -> EnumDef {
  let name = def.name.clone();
  let seeds = BTreeMap::from([(name, usage_flags(usage))]);
  let types = postprocess_types_with_usage(vec![RustType::Enum(def)], seeds);
  match types.into_iter().next().unwrap() {
    RustType::Enum(d) => d,
    _ => panic!("Expected Enum"),
  }
}

fn process_struct_for_server(def: StructDef, usage: TypeUsage) -> StructDef {
  let name = EnumToken::from(&def.name);
  let seeds = BTreeMap::from([(name, usage_flags(usage))]);
  let types = postprocess_types_for_server(vec![RustType::Struct(def)], seeds);
  match types.into_iter().next().unwrap() {
    RustType::Struct(d) => d,
    _ => panic!("Expected Struct"),
  }
}

fn process_enum_for_server(def: EnumDef, usage: TypeUsage) -> EnumDef {
  let name = def.name.clone();
  let seeds = BTreeMap::from([(name, usage_flags(usage))]);
  let types = postprocess_types_for_server(vec![RustType::Enum(def)], seeds);
  match types.into_iter().next().unwrap() {
    RustType::Enum(d) => d,
    _ => panic!("Expected Enum"),
  }
}

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
  assert!(def.outer_attrs.contains(&OuterAttr::SkipSerializingNone));

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
  assert!(!def.outer_attrs.contains(&OuterAttr::SkipSerializingNone));

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
  assert!(def.outer_attrs.contains(&OuterAttr::SkipSerializingNone));
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
  assert!(!def.outer_attrs.contains(&OuterAttr::SkipSerializingNone));
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
  assert!(def2.outer_attrs.contains(&OuterAttr::SkipSerializingNone));
}

#[test]
fn test_adds_nested_validation_attrs_transitively() {
  let validated_inner = create_struct("Inner", StructKind::Schema, false);

  let middle = StructDef {
    name: StructToken::new("Middle"),
    fields: vec![
      FieldDef::builder()
        .name(FieldNameToken::new("inner"))
        .rust_type(TypeRef::new("Inner"))
        .build(),
    ],
    kind: StructKind::Schema,
    ..Default::default()
  };

  let outer = StructDef {
    name: StructToken::new("Outer"),
    fields: vec![
      FieldDef::builder()
        .name(FieldNameToken::new("middle"))
        .rust_type(TypeRef::new("Middle"))
        .build(),
    ],
    kind: StructKind::Schema,
    ..Default::default()
  };

  let rust_types = vec![
    RustType::Struct(validated_inner),
    RustType::Struct(middle),
    RustType::Struct(outer),
  ];

  let rust_types = postprocess_types_with_usage(rust_types, BTreeMap::new());

  let middle = rust_types
    .iter()
    .find_map(|t| match t {
      RustType::Struct(def) if def.name.as_str() == "Middle" => Some(def),
      _ => None,
    })
    .expect("Middle struct not found");
  assert!(
    middle.fields[0].validation_attrs.contains(&ValidationAttribute::Nested),
    "missing nested validation on middle.inner"
  );

  let outer = rust_types
    .iter()
    .find_map(|t| match t {
      RustType::Struct(def) if def.name.as_str() == "Outer" => Some(def),
      _ => None,
    })
    .expect("Outer struct not found");
  assert!(
    outer.fields[0].validation_attrs.contains(&ValidationAttribute::Nested),
    "missing nested validation on outer.middle"
  );
}

#[test]
fn test_does_not_add_nested_validation_for_unvalidated_structs() {
  let unvalidated = StructDef {
    name: StructToken::new("Plain"),
    fields: vec![
      FieldDef::builder()
        .name(FieldNameToken::new("field"))
        .rust_type(TypeRef::new("String"))
        .build(),
    ],
    kind: StructKind::Schema,
    ..Default::default()
  };

  let outer = StructDef {
    name: StructToken::new("Outer"),
    fields: vec![
      FieldDef::builder()
        .name(FieldNameToken::new("plain"))
        .rust_type(TypeRef::new("Plain"))
        .build(),
    ],
    kind: StructKind::Schema,
    ..Default::default()
  };

  let rust_types = vec![RustType::Struct(unvalidated), RustType::Struct(outer)];
  let rust_types = postprocess_types_with_usage(rust_types, BTreeMap::new());

  let outer = rust_types
    .iter()
    .find_map(|t| match t {
      RustType::Struct(def) if def.name.as_str() == "Outer" => Some(def),
      _ => None,
    })
    .expect("Outer struct not found");
  assert!(
    outer.fields[0].validation_attrs.is_empty(),
    "unexpected nested validation for Outer.plain"
  );
}

#[test]
fn test_server_struct_request_only_gets_deserialize() {
  let def = create_struct("ServerRequest", StructKind::Schema, true);
  let def = process_struct_for_server(def, TypeUsage::RequestOnly);

  assert!(
    def.derives().contains(&DeriveTrait::Deserialize),
    "Server request-only struct should have Deserialize"
  );
  assert!(
    !def.derives().contains(&DeriveTrait::Serialize),
    "Server request-only struct should NOT have Serialize"
  );
  assert!(
    def.derives().contains(&DeriveTrait::Validate),
    "Server request struct should have Validate for input validation"
  );
  assert!(
    !def.fields[0].validation_attrs.is_empty(),
    "Server request struct should retain validation attrs"
  );
  assert!(
    !def.outer_attrs.contains(&OuterAttr::SkipSerializingNone),
    "Server request-only struct should NOT have SkipSerializingNone"
  );
}

#[test]
fn test_server_struct_response_only_gets_serialize() {
  let def = create_struct("ServerResponse", StructKind::Schema, true);
  let def = process_struct_for_server(def, TypeUsage::ResponseOnly);

  assert!(
    def.derives().contains(&DeriveTrait::Serialize),
    "Server response-only struct should have Serialize"
  );
  assert!(
    !def.derives().contains(&DeriveTrait::Deserialize),
    "Server response-only struct should NOT have Deserialize"
  );
  assert!(
    !def.derives().contains(&DeriveTrait::Validate),
    "Server response struct should NOT have Validate"
  );
  assert!(
    def.fields[0].validation_attrs.is_empty(),
    "Server response struct should have validation attrs cleared"
  );
  assert!(
    def.outer_attrs.contains(&OuterAttr::SkipSerializingNone),
    "Server response struct with nullable should have SkipSerializingNone"
  );
}

#[test]
fn test_server_struct_bidirectional_gets_both() {
  let def = create_struct("ServerBidirectional", StructKind::Schema, true);
  let def = process_struct_for_server(def, TypeUsage::Bidirectional);

  assert!(
    def.derives().contains(&DeriveTrait::Serialize),
    "Server bidirectional struct should have Serialize"
  );
  assert!(
    def.derives().contains(&DeriveTrait::Deserialize),
    "Server bidirectional struct should have Deserialize"
  );
  assert!(
    def.derives().contains(&DeriveTrait::Validate),
    "Server bidirectional struct should have Validate"
  );
}

#[test]
fn test_server_enum_request_only_gets_deserialize() {
  let def = create_enum("ServerEnumReq");
  let def = process_enum_for_server(def, TypeUsage::RequestOnly);

  assert!(
    def.derives().contains(&DeriveTrait::Deserialize),
    "Server request-only enum should have Deserialize"
  );
  assert!(
    !def.derives().contains(&DeriveTrait::Serialize),
    "Server request-only enum should NOT have Serialize"
  );
}

#[test]
fn test_server_enum_response_only_gets_serialize() {
  let def = create_enum("ServerEnumResp");
  let def = process_enum_for_server(def, TypeUsage::ResponseOnly);

  assert!(
    def.derives().contains(&DeriveTrait::Serialize),
    "Server response-only enum should have Serialize"
  );
  assert!(
    !def.derives().contains(&DeriveTrait::Deserialize),
    "Server response-only enum should NOT have Deserialize"
  );
}

#[test]
fn test_server_enum_bidirectional_gets_both() {
  let def = create_enum("ServerEnumBoth");
  let def = process_enum_for_server(def, TypeUsage::Bidirectional);

  assert!(
    def.derives().contains(&DeriveTrait::Serialize),
    "Server bidirectional enum should have Serialize"
  );
  assert!(
    def.derives().contains(&DeriveTrait::Deserialize),
    "Server bidirectional enum should have Deserialize"
  );
}

#[test]
fn test_client_vs_server_serde_inversion() {
  let client_req = process_struct_helper(
    create_struct("ClientReq", StructKind::Schema, false),
    TypeUsage::RequestOnly,
  );
  let server_req = process_struct_for_server(
    create_struct("ServerReq", StructKind::Schema, false),
    TypeUsage::RequestOnly,
  );

  assert!(client_req.derives().contains(&DeriveTrait::Serialize));
  assert!(!client_req.derives().contains(&DeriveTrait::Deserialize));
  assert!(!server_req.derives().contains(&DeriveTrait::Serialize));
  assert!(server_req.derives().contains(&DeriveTrait::Deserialize));

  let client_resp = process_struct_helper(
    create_struct("ClientResp", StructKind::Schema, false),
    TypeUsage::ResponseOnly,
  );
  let server_resp = process_struct_for_server(
    create_struct("ServerResp", StructKind::Schema, false),
    TypeUsage::ResponseOnly,
  );

  assert!(!client_resp.derives().contains(&DeriveTrait::Serialize));
  assert!(client_resp.derives().contains(&DeriveTrait::Deserialize));
  assert!(server_resp.derives().contains(&DeriveTrait::Serialize));
  assert!(!server_resp.derives().contains(&DeriveTrait::Deserialize));
}

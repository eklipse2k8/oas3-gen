use std::collections::BTreeMap;

use crate::generator::{
  analyzer::{TypeUsage, update_derives_from_usage},
  ast::{EnumDef, FieldDef, RustType, StructDef, StructKind, TypeRef, VariantContent, VariantDef},
};

fn run_transform(mut rust_types: Vec<RustType>, usage: &[(&str, TypeUsage)]) -> StructDef {
  let mut usage_map = BTreeMap::new();
  for (name, typ) in usage {
    usage_map.insert((*name).to_string(), *typ);
  }
  update_derives_from_usage(&mut rust_types, &usage_map);
  match rust_types.into_iter().next().unwrap() {
    RustType::Struct(def) => def,
    _ => panic!("expected struct type"),
  }
}

fn schema_struct(name: &str, field: FieldDef) -> StructDef {
  StructDef {
    name: name.to_string(),
    docs: vec![],
    fields: vec![field],
    derives: vec![],
    serde_attrs: vec![],
    outer_attrs: vec![],
    methods: vec![],
    kind: StructKind::Schema,
  }
}

fn operation_request_struct(name: &str) -> StructDef {
  StructDef {
    name: name.to_string(),
    docs: vec![],
    fields: vec![FieldDef {
      name: "id".to_string(),
      rust_type: TypeRef::new("String"),
      validation_attrs: vec!["length(min = 1)".to_string()],
      ..Default::default()
    }],
    derives: vec![],
    serde_attrs: vec![],
    outer_attrs: vec![],
    methods: vec![],
    kind: StructKind::OperationRequest,
  }
}

#[test]
fn response_only_removes_validation_and_skip_attr() {
  let field = FieldDef {
    name: "name".to_string(),
    rust_type: TypeRef::new("String"),
    validation_attrs: vec!["length(min = 1)".to_string()],
    regex_validation: Some("NAME_REGEX".to_string()),
    ..Default::default()
  };
  let mut def = schema_struct("ResponseType", field);
  def
    .outer_attrs
    .push("oas3_gen_support::skip_serializing_none".to_string());

  let def = run_transform(
    vec![RustType::Struct(def)],
    &[("ResponseType", TypeUsage::ResponseOnly)],
  );

  assert!(def.derives.contains(&"Deserialize".to_string()));
  assert!(!def.derives.iter().any(|d| d == "Serialize"));
  assert!(!def.derives.iter().any(|d| d == "validator::Validate"));
  assert!(def.outer_attrs.is_empty(), "skip_serializing_none should be removed");
  let field = &def.fields[0];
  assert!(field.validation_attrs.is_empty());
  assert!(field.regex_validation.is_none());
}

#[test]
fn request_only_adds_serialize_and_validate() {
  let field = FieldDef {
    name: "value".to_string(),
    rust_type: TypeRef::new("String"),
    validation_attrs: vec!["length(min = 1)".to_string()],
    ..Default::default()
  };
  let def = schema_struct("RequestType", field);

  let def = run_transform(vec![RustType::Struct(def)], &[("RequestType", TypeUsage::RequestOnly)]);

  assert!(def.derives.iter().any(|d| d == "Serialize"));
  assert!(def.derives.iter().any(|d| d == "validator::Validate"));
  assert!(
    !def
      .outer_attrs
      .iter()
      .any(|attr| attr.contains("skip_serializing_none"))
  );
}

#[test]
fn bidirectional_adds_serialization_and_validation() {
  let field = FieldDef {
    name: "optional".to_string(),
    rust_type: TypeRef::new("String").with_option(),
    validation_attrs: vec!["length(min = 1)".to_string()],
    ..Default::default()
  };
  let def = schema_struct("BothWays", field);

  let def = run_transform(vec![RustType::Struct(def)], &[("BothWays", TypeUsage::Bidirectional)]);

  assert!(def.derives.iter().any(|d| d == "Serialize"));
  assert!(def.derives.iter().any(|d| d == "Deserialize"));
  assert!(def.derives.iter().any(|d| d == "validator::Validate"));
  assert!(
    def
      .outer_attrs
      .iter()
      .any(|attr| attr == "oas3_gen_support::skip_serializing_none")
  );
}

#[test]
fn operation_request_always_validates_without_serialization() {
  let def = operation_request_struct("GetItemRequest");
  let def = run_transform(
    vec![RustType::Struct(def)],
    &[("GetItemRequest", TypeUsage::RequestOnly)],
  );

  assert!(def.derives.iter().any(|d| d == "validator::Validate"));
  assert!(!def.derives.iter().any(|d| d == "Serialize"));
  assert!(!def.derives.iter().any(|d| d == "Deserialize"));
  assert!(!def.fields[0].validation_attrs.is_empty());
}

fn enum_type(name: &str) -> EnumDef {
  EnumDef {
    name: name.to_string(),
    docs: vec![],
    variants: vec![VariantDef {
      name: "Variant".to_string(),
      docs: vec![],
      content: VariantContent::Unit,
      serde_attrs: vec![],
      deprecated: false,
    }],
    discriminator: None,
    derives: vec![
      "Debug".to_string(),
      "Clone".to_string(),
      "PartialEq".to_string(),
      "Serialize".to_string(),
      "Deserialize".to_string(),
      "oas3_gen_support::Default".to_string(),
    ],
    serde_attrs: vec![],
    outer_attrs: vec![],
    case_insensitive: false,
  }
}

fn run_enum_transform(name: &str, usage: TypeUsage) -> EnumDef {
  let enum_def = enum_type(name);
  let usage_map = BTreeMap::from([(name.to_string(), usage)]);
  let mut rust_types = vec![RustType::Enum(enum_def)];
  update_derives_from_usage(&mut rust_types, &usage_map);
  match rust_types.into_iter().next().unwrap() {
    RustType::Enum(def) => def,
    _ => panic!("expected enum"),
  }
}

#[test]
fn response_only_enum_drops_serialize() {
  let def = run_enum_transform("ResponseEnum", TypeUsage::ResponseOnly);
  assert!(def.derives.iter().any(|d| d == "Deserialize"));
  assert!(!def.derives.iter().any(|d| d == "Serialize"));
}

#[test]
fn request_only_enum_drops_deserialize() {
  let def = run_enum_transform("RequestEnum", TypeUsage::RequestOnly);
  assert!(def.derives.iter().any(|d| d == "Serialize"));
  assert!(!def.derives.iter().any(|d| d == "Deserialize"));
}

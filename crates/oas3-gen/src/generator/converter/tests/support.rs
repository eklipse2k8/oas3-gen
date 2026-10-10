use oas3::spec::{ObjectOrReference, ObjectSchema, Schema, SchemaType, SchemaTypeSet};

use crate::generator::ast::{
  EnumDef, EnumVariantToken, RustPrimitive, RustType, SerdeAttribute, TypeRef, VariantContent,
};

const SCHEMA_REF_PREFIX: &str = "#/components/schemas/";

pub(super) fn make_schema_ref(name: &str) -> Schema {
  Schema::Object(Box::new(ObjectOrReference::Ref {
    ref_path: format!("{SCHEMA_REF_PREFIX}{name}"),
    summary: None,
    description: None,
  }))
}

pub(super) fn make_schema_object(schema: ObjectSchema) -> Schema {
  Schema::Object(Box::new(ObjectOrReference::Object(schema)))
}

pub(super) fn make_integer_schema() -> ObjectSchema {
  ObjectSchema {
    schema_type: Some(SchemaTypeSet::Single(SchemaType::Integer)),
    ..Default::default()
  }
}

pub(super) fn make_null_schema() -> ObjectSchema {
  ObjectSchema {
    schema_type: Some(SchemaTypeSet::Single(SchemaType::Null)),
    ..Default::default()
  }
}

pub(super) fn assert_single_type_alias(types: &[RustType], expected_name: &str, expected_target: &str) {
  assert_eq!(types.len(), 1, "expected single type for {expected_name}");
  let RustType::TypeAlias(alias) = &types[0] else {
    panic!("expected type alias for {expected_name}")
  };
  assert_eq!(alias.name, expected_name, "type alias name mismatch");
  assert_eq!(
    alias.target.to_rust_type(),
    expected_target,
    "type alias target mismatch"
  );
}

/// Asserts `types` is exactly one non-exhaustive enum named `expected_name`: the `listed` unit
/// variants followed by an untagged `other` catch-all holding a `String`.
pub(super) fn assert_single_non_exhaustive_enum<'a>(
  types: &'a [RustType],
  expected_name: &str,
  listed: &[&str],
  other: &str,
) -> &'a EnumDef {
  let [RustType::Enum(enum_def)] = types else {
    panic!("expected only the {expected_name} enum, got {types:?}");
  };
  assert_eq!(enum_def.name, expected_name, "enum name");
  assert!(enum_def.non_exhaustive, "{expected_name} should be non-exhaustive");

  let listed_names = enum_def
    .listed_variants()
    .iter()
    .map(|v| v.name.as_str())
    .collect::<Vec<_>>();
  assert_eq!(listed_names, listed, "listed variants of {expected_name}");
  assert!(
    enum_def
      .listed_variants()
      .iter()
      .all(|v| v.content == VariantContent::Unit),
    "listed variants of {expected_name} should be unit variants"
  );

  let catch_all = enum_def
    .other_variant()
    .expect("non-exhaustive enum should have a catch-all");
  assert_eq!(
    catch_all.name,
    EnumVariantToken::new(other),
    "catch-all of {expected_name}"
  );
  assert_eq!(
    catch_all.content,
    VariantContent::Tuple(vec![TypeRef::new(RustPrimitive::String)]),
    "catch-all of {expected_name} should hold a String"
  );
  assert_eq!(
    catch_all.serde_attrs,
    vec![SerdeAttribute::Untagged],
    "catch-all of {expected_name} should be untagged"
  );
  enum_def
}

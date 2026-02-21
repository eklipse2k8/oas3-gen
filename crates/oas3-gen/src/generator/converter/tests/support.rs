use oas3::spec::{ObjectOrReference, ObjectSchema, SchemaType, SchemaTypeSet};

use crate::generator::{
  ast::{EnumDef, EnumVariantToken, RustType},
  naming::constants::KNOWN_ENUM_VARIANT,
};

const SCHEMA_REF_PREFIX: &str = "#/components/schemas/";

pub(super) fn make_schema_ref(name: &str) -> ObjectOrReference<ObjectSchema> {
  ObjectOrReference::Ref {
    ref_path: format!("{SCHEMA_REF_PREFIX}{name}"),
    summary: None,
    description: None,
  }
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

pub(super) fn assert_has_known_variant(enum_def: &EnumDef) {
  assert!(
    enum_def
      .variants
      .iter()
      .any(|variant| variant.name == EnumVariantToken::new(KNOWN_ENUM_VARIANT)),
    "expected enum to contain Known variant"
  );
}

use oas3_gen::generator::ast::{RustPrimitive, SerdeAsFieldAttr, TypeRef, ValidationAttribute};
use quote::ToTokens as _;
use rstest::rstest;
use serde_json::{Value, json};

use crate::parse_schema;

fn range_tokens(schema: Value, primitive: RustPrimitive) -> Option<String> {
  ValidationAttribute::range(&parse_schema(schema), &TypeRef::new(primitive))
    .map(|attribute| attribute.to_token_stream().to_string())
}

fn custom_override_tokens(optional: bool, is_array: bool) -> String {
  SerdeAsFieldAttr::CustomOverride {
    custom_type: "crate::MyDateTime".to_string(),
    optional,
    is_array,
  }
  .to_token_stream()
  .to_string()
}

#[rstest]
fn field_attributes_render_bounds_and_custom_serializers() {
  for (schema, primitive, expected) in [
    (
      json!({ "minimum": 1, "maximum": 1000 }),
      RustPrimitive::I32,
      Some("range (min = 1i32 , max = 1_000i32)"),
    ),
    (
      json!({ "exclusiveMinimum": 0, "exclusiveMaximum": 100 }),
      RustPrimitive::I64,
      Some("range (exclusive_min = 0i64 , exclusive_max = 100i64)"),
    ),
    (
      json!({ "minimum": 0.5, "maximum": 1.0 }),
      RustPrimitive::F64,
      Some("range (min = 0.5 , max = 1.0)"),
    ),
    (
      json!({ "minimum": 1e-6 }),
      RustPrimitive::F64,
      Some("range (min = 1e-6)"),
    ),
    (
      json!({ "minimum": -1.0, "maximum": 1.0 }),
      RustPrimitive::I64,
      Some("range (min = - 1i64 , max = 1i64)"),
    ),
    (
      json!({ "minimum": 1.5, "maximum": 2.5 }),
      RustPrimitive::I64,
      Some("range (min = 2i64 , max = 2i64)"),
    ),
    (
      json!({ "exclusiveMinimum": 1.5, "exclusiveMaximum": 2.5 }),
      RustPrimitive::I64,
      Some("range (exclusive_min = 1i64 , exclusive_max = 3i64)"),
    ),
    (json!({ "type": "integer" }), RustPrimitive::I64, None),
  ] {
    assert_eq!(
      range_tokens(schema.clone(), primitive),
      expected.map(ToString::to_string),
      "range for {schema}"
    );
  }

  for (min, max, expected) in [
    (Some(1), Some(1_000), "length (min = 1u64 , max = 1_000u64)"),
    (Some(10_000), None, "length (min = 10_000u64)"),
    (None, Some(1_000_000), "length (max = 1_000_000u64)"),
  ] {
    assert_eq!(
      ValidationAttribute::Length { min, max }.to_token_stream().to_string(),
      expected,
      "length for {min:?}..{max:?}"
    );
  }

  for (optional, is_array, expected) in [
    (false, false, "# [serde_as (as = \"crate::MyDateTime\")]"),
    (true, false, "# [serde_as (as = \"Option<crate::MyDateTime>\")]"),
    (false, true, "# [serde_as (as = \"Vec<crate::MyDateTime>\")]"),
    (true, true, "# [serde_as (as = \"Option<Vec<crate::MyDateTime>>\")]"),
  ] {
    assert_eq!(
      custom_override_tokens(optional, is_array),
      expected,
      "custom override optional={optional} array={is_array}"
    );
  }
}

use crate::generator::ast::{RustPrimitive, ValidationAttribute};

#[test]
fn test_validation_attribute_length_display() {
  let attr = ValidationAttribute::Length {
    min: Some(1),
    max: Some(1_000),
  };
  assert_eq!(attr.to_string(), "length(min = 1u64, max = 1_000u64)");

  let attr_min_only = ValidationAttribute::Length {
    min: Some(10_000),
    max: None,
  };
  assert_eq!(attr_min_only.to_string(), "length(min = 10_000u64)");

  let attr_max_only = ValidationAttribute::Length {
    min: None,
    max: Some(1_000_000),
  };
  assert_eq!(attr_max_only.to_string(), "length(max = 1_000_000u64)");
}

#[test]
fn test_validation_attribute_range_display() {
  let attr = ValidationAttribute::Range {
    primitive: RustPrimitive::I32,
    min: Some(serde_json::json!(1).as_number().unwrap().clone()),
    max: Some(serde_json::json!(1000).as_number().unwrap().clone()),
    exclusive_min: None,
    exclusive_max: None,
  };
  assert_eq!(attr.to_string(), "range(min = 1i32, max = 1_000i32)");

  let attr_exclusive = ValidationAttribute::Range {
    primitive: RustPrimitive::I64,
    min: None,
    max: None,
    exclusive_min: Some(serde_json::json!(0).as_number().unwrap().clone()),
    exclusive_max: Some(serde_json::json!(100).as_number().unwrap().clone()),
  };
  assert_eq!(
    attr_exclusive.to_string(),
    "range(exclusive_min = 0i64, exclusive_max = 100i64)"
  );

  let attr_float = ValidationAttribute::Range {
    primitive: RustPrimitive::F64,
    min: Some(serde_json::json!(0.5).as_number().unwrap().clone()),
    max: Some(serde_json::json!(1.0).as_number().unwrap().clone()),
    exclusive_min: None,
    exclusive_max: None,
  };
  assert_eq!(attr_float.to_string(), "range(min = 0.5, max = 1.0)");
}

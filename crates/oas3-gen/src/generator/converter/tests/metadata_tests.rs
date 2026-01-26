use oas3::spec::{ObjectSchema, SchemaType, SchemaTypeSet};
use serde_json::json;

use crate::generator::{
  ast::{RustPrimitive, TypeRef, ValidationAttribute},
  converter::fields::FieldConverter,
};

fn string_schema() -> ObjectSchema {
  ObjectSchema {
    schema_type: Some(SchemaTypeSet::Single(SchemaType::String)),
    ..Default::default()
  }
}

fn nullable_string_schema() -> ObjectSchema {
  ObjectSchema {
    schema_type: Some(SchemaTypeSet::Multiple(vec![SchemaType::String, SchemaType::Null])),
    ..Default::default()
  }
}

fn number_schema() -> ObjectSchema {
  ObjectSchema {
    schema_type: Some(SchemaTypeSet::Single(SchemaType::Number)),
    ..Default::default()
  }
}

fn integer_schema() -> ObjectSchema {
  ObjectSchema {
    schema_type: Some(SchemaTypeSet::Single(SchemaType::Integer)),
    ..Default::default()
  }
}

fn array_schema() -> ObjectSchema {
  ObjectSchema {
    schema_type: Some(SchemaTypeSet::Single(SchemaType::Array)),
    ..Default::default()
  }
}

fn num(n: i64) -> serde_json::Number {
  json!(n).as_number().unwrap().clone()
}

#[test]
fn extract_default_value_priority() {
  let cases = [
    (
      Some(json!("default")),
      None,
      vec![],
      Some(json!("default")),
      "default value",
    ),
    (None, Some(json!("const")), vec![], Some(json!("const")), "const value"),
    (
      None,
      None,
      vec![json!("only")],
      Some(json!("only")),
      "single enum value",
    ),
    (
      Some(json!("default")),
      Some(json!("const")),
      vec![json!("enum")],
      Some(json!("default")),
      "default takes priority over const and enum",
    ),
    (None, None, vec![], None, "no default when all empty"),
    (
      None,
      None,
      vec![json!("a"), json!("b")],
      None,
      "no default for multi-value enum",
    ),
  ];

  for (default, const_value, enum_values, expected, desc) in cases {
    let schema = ObjectSchema {
      default,
      const_value,
      enum_values,
      ..Default::default()
    };
    assert_eq!(FieldConverter::extract_default_value(&schema), expected, "{desc}");
  }
}

#[test]
fn extract_parameter_metadata_returns_validation_and_default() {
  let mut schema = string_schema();
  schema.min_length = Some(5);
  schema.max_length = Some(50);
  schema.default = Some(json!("default_value"));

  let type_ref = TypeRef::new(RustPrimitive::String);
  let (attrs, default) = FieldConverter::extract_parameter_metadata("param", true, &schema, &type_ref);

  assert_eq!(
    attrs,
    vec![ValidationAttribute::Length {
      min: Some(5),
      max: Some(50)
    }]
  );
  assert_eq!(default, Some(json!("default_value")));
}

#[test]
fn validation_format_to_attribute() {
  let cases = [
    ("email", ValidationAttribute::Email, "email format"),
    ("url", ValidationAttribute::Url, "url format"),
    ("uri", ValidationAttribute::Url, "uri format maps to Url"),
  ];

  for (format, expected_attr, desc) in cases {
    let mut schema = string_schema();
    schema.format = Some(format.to_string());
    let attrs = FieldConverter::extract_all_validation("test", false, &schema, &TypeRef::new(RustPrimitive::String));
    assert!(attrs.contains(&expected_attr), "{desc}");
  }
}

#[test]
fn validation_numeric_range() {
  let mut schema = integer_schema();
  schema.minimum = Some(num(0));
  schema.maximum = Some(num(100));

  let attrs = FieldConverter::extract_all_validation("test", false, &schema, &TypeRef::new(RustPrimitive::I32));

  assert_eq!(
    attrs,
    vec![ValidationAttribute::Range {
      primitive: RustPrimitive::I32,
      min: Some(num(0)),
      max: Some(num(100)),
      exclusive_min: None,
      exclusive_max: None,
    }]
  );
}

#[test]
fn validation_exclusive_numeric_range() {
  let mut schema = number_schema();
  schema.exclusive_minimum = Some(num(0));
  schema.exclusive_maximum = Some(num(100));

  let attrs = FieldConverter::extract_all_validation("test", false, &schema, &TypeRef::new(RustPrimitive::I32));

  assert_eq!(
    attrs,
    vec![ValidationAttribute::Range {
      primitive: RustPrimitive::I32,
      min: None,
      max: None,
      exclusive_min: Some(num(0)),
      exclusive_max: Some(num(100)),
    }]
  );
}

#[test]
fn validation_no_range_when_bounds_empty() {
  let schema = number_schema();
  let attrs = FieldConverter::extract_all_validation("test", false, &schema, &TypeRef::new(RustPrimitive::I32));
  assert!(attrs.is_empty());
}

#[test]
fn validation_string_length() {
  let cases = [
    (Some(1), Some(100), "min and max"),
    (Some(5), None, "min only"),
    (None, Some(20), "max only"),
  ];

  for (min, max, desc) in cases {
    let mut schema = string_schema();
    schema.min_length = min;
    schema.max_length = max;
    let attrs = FieldConverter::extract_all_validation("test", false, &schema, &TypeRef::new(RustPrimitive::String));
    assert_eq!(attrs, vec![ValidationAttribute::Length { min, max }], "{desc}");
  }
}

#[test]
fn validation_length_skipped_for_non_string_types() {
  let mut schema = string_schema();
  schema.min_length = Some(1);
  schema.format = Some("date".to_string());

  let attrs = FieldConverter::extract_all_validation("test", false, &schema, &TypeRef::new(RustPrimitive::Date));
  assert!(attrs.is_empty());
}

#[test]
fn validation_required_string_implies_min_length() {
  let schema = string_schema();
  let attrs = FieldConverter::extract_all_validation("test", true, &schema, &TypeRef::new(RustPrimitive::String));
  assert_eq!(
    attrs,
    vec![ValidationAttribute::Length {
      min: Some(1),
      max: None
    }]
  );
}

#[test]
fn validation_nullable_required_string_skips_implicit_min_length() {
  let schema = nullable_string_schema();
  let type_ref = TypeRef::new(RustPrimitive::String).with_option();
  let attrs = FieldConverter::extract_all_validation("test", true, &schema, &type_ref);
  assert!(
    attrs.is_empty(),
    "nullable required strings should not get implicit min_length=1"
  );
}

#[test]
fn validation_nullable_string_with_explicit_min_length() {
  let mut schema = nullable_string_schema();
  schema.min_length = Some(5);
  let type_ref = TypeRef::new(RustPrimitive::String).with_option();
  let attrs = FieldConverter::extract_all_validation("test", true, &schema, &type_ref);
  assert_eq!(
    attrs,
    vec![ValidationAttribute::Length {
      min: Some(5),
      max: None
    }],
    "explicit min_length should still be applied to nullable strings"
  );
}

#[test]
fn validation_regex_pattern() {
  let mut schema = string_schema();
  schema.pattern = Some("^[a-z]+$".to_string());

  let attrs = FieldConverter::extract_all_validation("test", false, &schema, &TypeRef::new(RustPrimitive::String));
  assert!(attrs.contains(&ValidationAttribute::Regex("^[a-z]+$".to_string())));
}

#[test]
fn validation_regex_skipped_for_special_types() {
  let skip_cases = [
    (vec![json!("a"), json!("b")], RustPrimitive::String, "enum values"),
    (vec![], RustPrimitive::DateTime, "datetime type"),
    (vec![], RustPrimitive::Date, "date type"),
    (vec![], RustPrimitive::Uuid, "uuid type"),
  ];

  for (enum_values, primitive, desc) in skip_cases {
    let mut schema = string_schema();
    schema.pattern = Some("^[a-z]+$".to_string());
    schema.enum_values = enum_values;

    let attrs = FieldConverter::extract_all_validation("test", false, &schema, &TypeRef::new(primitive));
    assert!(
      !attrs.iter().any(|a| matches!(a, ValidationAttribute::Regex(_))),
      "regex should be skipped for {desc}"
    );
  }
}

#[test]
fn validation_invalid_regex_skipped() {
  let mut schema = string_schema();
  schema.pattern = Some("^[a-z+$".to_string());

  let attrs = FieldConverter::extract_all_validation("test", false, &schema, &TypeRef::new(RustPrimitive::String));
  assert!(!attrs.iter().any(|a| matches!(a, ValidationAttribute::Regex(_))));
}

#[test]
fn validation_array_length() {
  let mut schema = array_schema();
  schema.min_items = Some(1);
  schema.max_items = Some(10);

  let attrs =
    FieldConverter::extract_all_validation("test", false, &schema, &TypeRef::new(RustPrimitive::String).with_vec());
  assert_eq!(
    attrs,
    vec![ValidationAttribute::Length {
      min: Some(1),
      max: Some(10)
    }]
  );
}

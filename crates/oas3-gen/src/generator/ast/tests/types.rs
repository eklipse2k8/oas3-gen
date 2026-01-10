use std::collections::BTreeSet;

use quote::quote;

use crate::generator::ast::{
  Documentation, FieldDef, FieldNameToken, RustPrimitive, SerdeAttribute, TypeRef, ValidationAttribute,
  types::{parse_date_parts, parse_time_parts},
};

fn base_field(type_ref: TypeRef) -> FieldDef {
  FieldDef::builder()
    .name(FieldNameToken::from_raw("test_field"))
    .docs(Documentation::from_lines(["Some docs"]))
    .rust_type(type_ref)
    .serde_attrs(BTreeSet::from([SerdeAttribute::Rename("original".to_string())]))
    .validation_attrs(vec![ValidationAttribute::Email])
    .build()
}

#[test]
fn test_rust_primitive_from_str() {
  let cases: Vec<(&str, RustPrimitive)> = vec![
    ("i8", RustPrimitive::I8),
    ("i16", RustPrimitive::I16),
    ("i32", RustPrimitive::I32),
    ("i64", RustPrimitive::I64),
    ("i128", RustPrimitive::I128),
    ("isize", RustPrimitive::Isize),
    ("u8", RustPrimitive::U8),
    ("u16", RustPrimitive::U16),
    ("u32", RustPrimitive::U32),
    ("u64", RustPrimitive::U64),
    ("u128", RustPrimitive::U128),
    ("usize", RustPrimitive::Usize),
    ("f32", RustPrimitive::F32),
    ("f64", RustPrimitive::F64),
    ("bool", RustPrimitive::Bool),
    ("String", RustPrimitive::String),
    ("()", RustPrimitive::Unit),
    ("Vec<u8>", RustPrimitive::Bytes),
    ("chrono::NaiveDate", RustPrimitive::Date),
    ("chrono::DateTime<chrono::Utc>", RustPrimitive::DateTime),
    ("chrono::NaiveTime", RustPrimitive::Time),
    ("uuid::Uuid", RustPrimitive::Uuid),
    ("serde_json::Value", RustPrimitive::Value),
    ("MyCustomType", RustPrimitive::Custom("MyCustomType".into())),
    ("Vec<MyType>", RustPrimitive::Custom("Vec<MyType>".into())),
  ];

  for (input, expected) in cases {
    let parsed: RustPrimitive = input.parse().unwrap();
    assert_eq!(parsed, expected, "parsing '{input}' failed");
  }
}

#[test]
fn test_rust_primitive_display_and_default() {
  assert_eq!(RustPrimitive::default(), RustPrimitive::String);

  let primitives = vec![
    RustPrimitive::I8,
    RustPrimitive::I32,
    RustPrimitive::I64,
    RustPrimitive::U32,
    RustPrimitive::U64,
    RustPrimitive::F32,
    RustPrimitive::F64,
    RustPrimitive::Bool,
    RustPrimitive::String,
    RustPrimitive::Bytes,
    RustPrimitive::Date,
    RustPrimitive::DateTime,
    RustPrimitive::Uuid,
    RustPrimitive::Value,
    RustPrimitive::Unit,
    RustPrimitive::Custom("MyType".into()),
  ];

  for primitive in primitives {
    let string = primitive.to_string();
    let parsed: RustPrimitive = string.parse().unwrap();
    assert_eq!(parsed, primitive, "round-trip failed for {primitive:?}");
  }
}

#[test]
fn test_type_ref_construction() {
  let from_str = TypeRef::new("i32");
  assert_eq!(from_str.base_type, RustPrimitive::I32);
  assert!(!from_str.nullable);

  let from_primitive = TypeRef::new(RustPrimitive::F64);
  assert_eq!(from_primitive.base_type, RustPrimitive::F64);

  let default_ref = TypeRef::default();
  assert_eq!(default_ref.base_type, RustPrimitive::String);
  assert!(!default_ref.nullable);
  assert!(!default_ref.is_array);
  assert!(!default_ref.boxed);
}

#[test]
fn test_type_ref_wrappers() {
  let cases = [
    (TypeRef::new("String").with_vec().with_option(), "Option<Vec<String>>"),
    (TypeRef::new("MyType").with_boxed().with_option(), "Option<Box<MyType>>"),
    (TypeRef::new("i32").with_vec(), "Vec<i32>"),
    (TypeRef::new("bool").with_option(), "Option<bool>"),
  ];

  for (type_ref, expected) in cases {
    assert_eq!(
      type_ref.to_rust_type(),
      expected,
      "wrapper failed for expected {expected}"
    );
  }
}

#[allow(clippy::approx_constant)]
#[test]
fn test_format_example_primitive_types() {
  let cases: Vec<(TypeRef, serde_json::Value, &str)> = vec![
    (
      TypeRef::new(RustPrimitive::String).with_option(),
      serde_json::Value::Null,
      "None",
    ),
    (TypeRef::new(RustPrimitive::String), serde_json::Value::Null, ""),
    (
      TypeRef::new(RustPrimitive::String),
      serde_json::json!("hello"),
      "\"hello\"",
    ),
    (TypeRef::new(RustPrimitive::I32), serde_json::json!(42), "42i32"),
    (TypeRef::new(RustPrimitive::F64), serde_json::json!(3.14), "3.14"),
    (TypeRef::new(RustPrimitive::Bool), serde_json::json!(true), "true"),
    (TypeRef::new(RustPrimitive::Bool), serde_json::json!(false), "false"),
  ];

  for (type_ref, example, expected) in cases {
    let result = type_ref.format_example(&example);
    assert_eq!(
      result, expected,
      "format_example failed for {example:?} with type {:?}",
      type_ref.base_type
    );
  }
}

#[test]
fn test_format_example_arrays() {
  let cases: Vec<(TypeRef, serde_json::Value, &str)> = vec![
    (
      TypeRef::new(RustPrimitive::String).with_vec(),
      serde_json::json!(["foo", "bar", "baz"]),
      "vec![\"foo\", \"bar\", \"baz\"]",
    ),
    (
      TypeRef::new(RustPrimitive::I32).with_vec(),
      serde_json::json!([1, 2, 3]),
      "vec![1i32, 2i32, 3i32]",
    ),
    (
      TypeRef::new(RustPrimitive::String).with_vec(),
      serde_json::json!([]),
      "vec![]",
    ),
    (
      TypeRef::new(RustPrimitive::Date).with_vec(),
      serde_json::json!(["2024-01-15", "2024-02-20"]),
      "vec![chrono::NaiveDate::from_ymd_opt(2024, 1, 15)?, chrono::NaiveDate::from_ymd_opt(2024, 2, 20)?]",
    ),
    (
      TypeRef::new(RustPrimitive::I32).with_vec(),
      serde_json::json!([[1, 2], [3, 4]]),
      "vec![vec![1i32, 2i32], vec![3i32, 4i32]]",
    ),
  ];

  for (type_ref, example, expected) in cases {
    let result = type_ref.format_example(&example);
    assert_eq!(result, expected, "array format failed for {example:?}");
  }
}

#[test]
fn test_format_example_special_types() {
  let cases: Vec<(TypeRef, &str, &str)> = vec![
    (
      TypeRef::new(RustPrimitive::Date),
      "2024-01-15",
      "chrono::NaiveDate::from_ymd_opt(2024, 1, 15)?",
    ),
    (
      TypeRef::new(RustPrimitive::DateTime),
      "2024-01-15T10:30:00Z",
      "chrono::DateTime::parse_from_rfc3339(\"2024-01-15T10:30:00Z\")?.with_timezone(&chrono::Utc)",
    ),
    (
      TypeRef::new(RustPrimitive::Time),
      "14:30:00",
      "chrono::NaiveTime::from_hms_opt(14, 30, 0)?",
    ),
    (
      TypeRef::new(RustPrimitive::Uuid),
      "550e8400-e29b-41d4-a716-446655440000",
      "uuid::Uuid::parse_str(\"550e8400-e29b-41d4-a716-446655440000\")?",
    ),
  ];

  for (type_ref, input, expected) in cases {
    let example = serde_json::Value::String(input.to_string());
    let result = type_ref.format_example(&example);
    assert_eq!(result, expected, "special type format failed for {input}");
  }
}

#[test]
fn test_format_example_boxed() {
  let type_ref = TypeRef::new(RustPrimitive::String).with_boxed();
  let example = serde_json::Value::String("boxed".to_string());
  assert_eq!(type_ref.format_example(&example), "Box::new(\"boxed\")");
}

#[test]
fn test_parse_date_parts() {
  let valid_cases = [("2024-01-15", Some((2024, 1, 15)))];
  let invalid_cases = [("not-a-date", None), ("2024-01", None)];

  for (input, expected) in valid_cases.iter().chain(invalid_cases.iter()) {
    assert_eq!(
      parse_date_parts(input),
      *expected,
      "parse_date_parts failed for '{input}'"
    );
  }
}

#[test]
fn test_parse_time_parts() {
  let cases = [
    ("14:30:45", Some((14, 30, 45))),
    ("14:30", Some((14, 30, 0))),
    ("14:30:45.123", Some((14, 30, 45))),
    ("not-a-time", None),
    ("25:00:00", None),
  ];

  for (input, expected) in cases {
    assert_eq!(
      parse_time_parts(input),
      expected,
      "parse_time_parts failed for '{input}'"
    );
  }
}

#[test]
fn test_escape_string_literal() {
  let type_ref = TypeRef::new(RustPrimitive::String);
  let cases = [
    ("page_to_fetch : \"001e0010\"", "\"page_to_fetch : \\\"001e0010\\\"\""),
    ("path\\to\\file", "\"path\\\\to\\\\file\""),
    ("line1\nline2", "\"line1\\nline2\""),
    ("col1\tcol2", "\"col1\\tcol2\""),
    ("\"quoted\"\n\\backslash\\", "\"\\\"quoted\\\"\\n\\\\backslash\\\\\""),
  ];

  for (input, expected) in cases {
    let example = serde_json::Value::String(input.to_string());
    assert_eq!(
      type_ref.format_example(&example),
      expected,
      "escape failed for '{input}'"
    );
  }

  let uuid_ref = TypeRef::new(RustPrimitive::Uuid);
  let uuid_example = serde_json::Value::String("\"550e8400-e29b-41d4-a716-446655440000\"".to_string());
  assert_eq!(
    uuid_ref.format_example(&uuid_example),
    "uuid::Uuid::parse_str(\"\\\"550e8400-e29b-41d4-a716-446655440000\\\"\")?"
  );
}

#[test]
fn test_type_coercion() {
  let string_type = TypeRef::new(RustPrimitive::String);
  let bool_type = TypeRef::new(RustPrimitive::Bool);
  let i32_type = TypeRef::new(RustPrimitive::I32);
  let optional_string = TypeRef::new(RustPrimitive::String).with_option();

  let cases: Vec<(&TypeRef, serde_json::Value, &str)> = vec![
    (&string_type, serde_json::json!(true), "\"true\""),
    (&string_type, serde_json::json!(false), "\"false\""),
    (&bool_type, serde_json::json!(true), "true"),
    (&string_type, serde_json::json!(2.2), "\"2.2\""),
    (&string_type, serde_json::json!(42), "\"42\""),
    (&i32_type, serde_json::json!(42), "42i32"),
    (&optional_string, serde_json::json!(true), "\"true\""),
    (&optional_string, serde_json::json!(2.2), "\"2.2\""),
  ];

  for (type_ref, example, expected) in cases {
    let result = type_ref.format_example(&example);
    assert_eq!(
      result, expected,
      "coercion failed for {example:?} -> {:?}",
      type_ref.base_type
    );
  }
}

#[test]
fn test_complete_header_example_flow() {
  let type_ref = TypeRef::new(RustPrimitive::String).with_option();

  let test_cases = [
    (serde_json::json!(true), "\"true\""),
    (serde_json::json!(2.2), "\"2.2\""),
  ];

  for (example, expected_formatted) in test_cases {
    let formatted = type_ref.format_example(&example);
    assert_eq!(formatted, expected_formatted, "format failed for {example:?}");

    let with_to_string = format!("{formatted}.to_string()");
    let with_some = format!("Some({with_to_string})");
    assert!(with_some.starts_with("Some("), "wrapping failed for {example:?}");
  }
}

#[test]
fn discriminator_behavior() {
  struct Case {
    name: &'static str,
    type_ref: TypeRef,
    discriminator_value: Option<&'static str>,
    is_base: bool,
    expect_doc_hidden: bool,
    expect_skip_deserializing: bool,
    expect_skip: bool,
    expect_default: bool,
    expected_default_value: Option<serde_json::Value>,
  }

  let cases = [
    Case {
      name: "child discriminator hides and sets value",
      type_ref: TypeRef::new(RustPrimitive::String),
      discriminator_value: Some("child_type"),
      is_base: false,
      expect_doc_hidden: true,
      expect_skip_deserializing: true,
      expect_skip: false,
      expect_default: true,
      expected_default_value: Some(serde_json::Value::String("child_type".to_string())),
    },
    Case {
      name: "base hides and skips string",
      type_ref: TypeRef::new(RustPrimitive::String),
      discriminator_value: None,
      is_base: true,
      expect_doc_hidden: true,
      expect_skip_deserializing: false,
      expect_skip: true,
      expect_default: false,
      expected_default_value: Some(serde_json::Value::String(String::new())),
    },
    Case {
      name: "base non-string no default",
      type_ref: TypeRef::new(RustPrimitive::I64),
      discriminator_value: None,
      is_base: true,
      expect_doc_hidden: true,
      expect_skip_deserializing: false,
      expect_skip: true,
      expect_default: false,
      expected_default_value: None,
    },
  ];

  for case in cases {
    let field = base_field(case.type_ref);
    let result = field.with_discriminator_behavior(case.discriminator_value, case.is_base);

    let docs = &result.docs;
    assert!(quote! { #docs }.is_empty(), "{}: docs should be cleared", case.name);
    assert!(
      result.validation_attrs.is_empty(),
      "{}: validation should be cleared",
      case.name
    );
    assert_eq!(
      result.doc_hidden, case.expect_doc_hidden,
      "{}: doc_hidden mismatch",
      case.name
    );

    assert_eq!(
      result.serde_attrs.contains(&SerdeAttribute::SkipDeserializing),
      case.expect_skip_deserializing,
      "{}: SkipDeserializing mismatch",
      case.name
    );
    assert_eq!(
      result.serde_attrs.contains(&SerdeAttribute::Skip),
      case.expect_skip,
      "{}: Skip mismatch",
      case.name
    );
    assert_eq!(
      result.serde_attrs.contains(&SerdeAttribute::Default),
      case.expect_default,
      "{}: Default mismatch",
      case.name
    );
    assert_eq!(
      result.default_value, case.expected_default_value,
      "{}: default_value mismatch",
      case.name
    );
  }
}

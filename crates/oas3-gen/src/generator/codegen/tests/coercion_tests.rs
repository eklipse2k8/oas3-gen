use serde_json::json;

use crate::generator::{ast::TypeRef, codegen::coercion};

fn assert_conversion(value: &serde_json::Value, rust_type: &TypeRef, expected: &str) {
  let result = coercion::json_to_rust_literal(value, rust_type);
  let code = result.to_string();
  assert_eq!(
    code.trim(),
    expected.trim(),
    "Conversion mismatch for {value:?} -> {}",
    rust_type.base_type
  );
}

fn nullable_type(name: &str) -> TypeRef {
  let mut t = TypeRef::new(name);
  t.nullable = true;
  t
}

#[test]
fn test_string_type_conversions() {
  let cases = [
    (json!(""), "String :: new ()"),
    (json!("hello"), r#""hello" . to_string ()"#),
    (json!("hello world"), r#""hello world" . to_string ()"#),
    (json!(42), r#""42" . to_string ()"#),
    (json!(2.5), r#""2.5" . to_string ()"#),
    (json!(true), r#""true" . to_string ()"#),
  ];
  let rust_type = TypeRef::new("String");
  for (value, expected) in cases {
    assert_conversion(&value, &rust_type, expected);
  }
}

#[test]
fn test_signed_integer_from_values() {
  let cases: Vec<(serde_json::Value, &str, &str)> = vec![
    (json!(42), "i64", "42i64"),
    (json!("123"), "i64", "123i64"),
    (json!("not_a_number"), "i64", "Default :: default ()"),
    (json!(100), "i32", "100i32"),
    (json!(50), "i16", "50i16"),
  ];
  for (value, type_name, expected) in cases {
    assert_conversion(&value, &TypeRef::new(type_name), expected);
  }
}

#[test]
fn test_int_type_suffixes() {
  let value = json!(42);
  let cases = [
    ("i8", "42i8"),
    ("i16", "42i16"),
    ("i32", "42i32"),
    ("i64", "42i64"),
    ("i128", "42i128"),
    ("isize", "42isize"),
  ];
  for (type_name, expected) in cases {
    assert_conversion(&value, &TypeRef::new(type_name), expected);
  }
}

#[test]
fn test_unsigned_integer_conversions() {
  let cases: Vec<(serde_json::Value, &str, &str)> = vec![
    (json!(42), "u32", "42u32"),
    (json!(255), "u8", "255u8"),
    (json!("100"), "u64", "100u64"),
    (json!(-1), "u32", "Default :: default ()"),
  ];
  for (value, type_name, expected) in cases {
    assert_conversion(&value, &TypeRef::new(type_name), expected);
  }
}

#[test]
fn test_uint_type_suffixes() {
  let value = json!(42);
  let cases = [
    ("u8", "42u8"),
    ("u16", "42u16"),
    ("u32", "42u32"),
    ("u64", "42u64"),
    ("u128", "42u128"),
    ("usize", "42usize"),
  ];
  for (type_name, expected) in cases {
    assert_conversion(&value, &TypeRef::new(type_name), expected);
  }
}

#[test]
fn test_float_from_values() {
  let cases: Vec<(serde_json::Value, &str, &str)> = vec![
    (json!(2.5), "f64", "2.5f64"),
    (json!(10), "f64", "10f64"),
    (json!("2.718"), "f64", "2.718f64"),
    (json!("not_a_float"), "f64", "Default :: default ()"),
    (json!(1.5), "f32", "1.5f32"),
  ];
  for (value, type_name, expected) in cases {
    assert_conversion(&value, &TypeRef::new(type_name), expected);
  }
}

#[allow(clippy::approx_constant)]
#[test]
fn test_float_type_suffixes() {
  let value = json!(3.14);
  let cases = [("f32", "3.14f32"), ("f64", "3.14f64")];
  for (type_name, expected) in cases {
    assert_conversion(&value, &TypeRef::new(type_name), expected);
  }
}

#[test]
fn test_bool_from_primitives() {
  let rust_type = TypeRef::new("bool");
  let cases = [
    (json!(true), "true"),
    (json!(false), "false"),
    (json!(1), "true"),
    (json!(42), "true"),
    (json!(0), "false"),
  ];
  for (value, expected) in cases {
    assert_conversion(&value, &rust_type, expected);
  }
}

#[test]
fn test_bool_from_strings() {
  let rust_type = TypeRef::new("bool");
  let true_cases = [
    json!("true"),
    json!("True"),
    json!("TRUE"),
    json!("1"),
    json!("yes"),
    json!("Yes"),
  ];
  for value in true_cases {
    assert_conversion(&value, &rust_type, "true");
  }

  let false_cases = [json!("false"), json!("no"), json!("0"), json!("anything")];
  for value in false_cases {
    assert_conversion(&value, &rust_type, "false");
  }
}

#[test]
fn test_null_value() {
  let value = json!(null);
  let rust_type = TypeRef::new("String");
  assert_conversion(&value, &rust_type, "None");
}

#[test]
fn test_nullable_with_values() {
  let cases: Vec<(serde_json::Value, &str, &str)> = vec![
    (json!("hello"), "String", r#"Some ("hello" . to_string ())"#),
    (json!(42), "i64", "Some (42i64)"),
    (json!(2.5), "f64", "Some (2.5f64)"),
    (json!(true), "bool", "Some (true)"),
  ];
  for (value, type_name, expected) in cases {
    assert_conversion(&value, &nullable_type(type_name), expected);
  }
}

#[test]
fn test_nullable_with_null() {
  let value = json!(null);
  assert_conversion(&value, &nullable_type("String"), "None");
}

#[test]
fn test_nullable_type_suffixes() {
  let cases: Vec<(serde_json::Value, &str, &str)> = vec![
    (json!(25), "i32", "Some (25i32)"),
    (json!(25), "u32", "Some (25u32)"),
    (json!(1.5), "f32", "Some (1.5f32)"),
  ];
  for (value, type_name, expected) in cases {
    assert_conversion(&value, &nullable_type(type_name), expected);
  }
}

#[test]
fn test_complex_type_defaults() {
  let cases = [(json!([1, 2, 3]), "Vec<i64>"), (json!({"key": "value"}), "CustomType")];
  for (value, type_name) in cases {
    assert_conversion(&value, &TypeRef::new(type_name), "Default :: default ()");
  }
}

#[test]
fn test_negative_numbers() {
  assert_conversion(&json!(-42), &TypeRef::new("i64"), "- 42i64");
  assert_conversion(&json!(-2.5), &TypeRef::new("f64"), "- 2.5f64");
}

#[test]
fn test_zero_values() {
  assert_conversion(&json!(0), &TypeRef::new("i64"), "0i64");
  assert_conversion(&json!(0.0), &TypeRef::new("f64"), "0f64");
}

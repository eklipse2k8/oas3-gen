use serde_json::json;

use crate::generator::{ast::TypeRef, codegen::coercion};

fn assert_conversion(value: &serde_json::Value, rust_type: &TypeRef, expected: &str) {
  let result = coercion::json_to_rust_literal(value, rust_type);
  let code = result.to_string();
  assert_eq!(code.trim(), expected.trim(), "Conversion mismatch");
}

#[test]
fn test_string_empty() {
  let value = json!("");
  let rust_type = TypeRef::new("String");
  assert_conversion(&value, &rust_type, "String :: new ()");
}

#[test]
fn test_string_regular() {
  let value = json!("hello");
  let rust_type = TypeRef::new("String");
  assert_conversion(&value, &rust_type, r#""hello" . to_string ()"#);
}

#[test]
fn test_string_with_spaces() {
  let value = json!("hello world");
  let rust_type = TypeRef::new("String");
  assert_conversion(&value, &rust_type, r#""hello world" . to_string ()"#);
}

#[test]
fn test_number_to_string() {
  let value = json!(42);
  let rust_type = TypeRef::new("String");
  assert_conversion(&value, &rust_type, r#""42" . to_string ()"#);
}

#[test]
fn test_float_to_string() {
  let value = json!(2.5);
  let rust_type = TypeRef::new("String");
  assert_conversion(&value, &rust_type, r#""2.5" . to_string ()"#);
}

#[test]
fn test_bool_to_string() {
  let value = json!(true);
  let rust_type = TypeRef::new("String");
  assert_conversion(&value, &rust_type, r#""true" . to_string ()"#);
}

#[test]
fn test_int_from_number() {
  let value = json!(42);
  let rust_type = TypeRef::new("i64");
  assert_conversion(&value, &rust_type, "42i64");
}

#[test]
fn test_int_from_string() {
  let value = json!("123");
  let rust_type = TypeRef::new("i64");
  assert_conversion(&value, &rust_type, "123i64");
}

#[test]
fn test_int_invalid_string() {
  let value = json!("not_a_number");
  let rust_type = TypeRef::new("i64");
  assert_conversion(&value, &rust_type, "Default :: default ()");
}

#[test]
fn test_float_from_float() {
  let value = json!(2.5);
  let rust_type = TypeRef::new("f64");
  assert_conversion(&value, &rust_type, "2.5f64");
}

#[test]
fn test_float_from_integer_coercion() {
  let value = json!(10);
  let rust_type = TypeRef::new("f64");
  assert_conversion(&value, &rust_type, "10f64");
}

#[test]
fn test_float_from_string() {
  let value = json!("2.718");
  let rust_type = TypeRef::new("f64");
  assert_conversion(&value, &rust_type, "2.718f64");
}

#[test]
fn test_float_invalid_string() {
  let value = json!("not_a_float");
  let rust_type = TypeRef::new("f64");
  assert_conversion(&value, &rust_type, "Default :: default ()");
}

#[test]
fn test_bool_from_bool() {
  let value = json!(true);
  let rust_type = TypeRef::new("bool");
  assert_conversion(&value, &rust_type, "true");

  let value = json!(false);
  assert_conversion(&value, &rust_type, "false");
}

#[test]
fn test_bool_from_number_nonzero() {
  let value = json!(1);
  let rust_type = TypeRef::new("bool");
  assert_conversion(&value, &rust_type, "true");

  let value = json!(42);
  assert_conversion(&value, &rust_type, "true");
}

#[test]
fn test_bool_from_number_zero() {
  let value = json!(0);
  let rust_type = TypeRef::new("bool");
  assert_conversion(&value, &rust_type, "false");
}

#[test]
fn test_bool_from_string_true() {
  let value = json!("true");
  let rust_type = TypeRef::new("bool");
  assert_conversion(&value, &rust_type, "true");

  let value = json!("True");
  assert_conversion(&value, &rust_type, "true");

  let value = json!("TRUE");
  assert_conversion(&value, &rust_type, "true");

  let value = json!("1");
  assert_conversion(&value, &rust_type, "true");

  let value = json!("yes");
  assert_conversion(&value, &rust_type, "true");

  let value = json!("Yes");
  assert_conversion(&value, &rust_type, "true");
}

#[test]
fn test_bool_from_string_false() {
  let value = json!("false");
  let rust_type = TypeRef::new("bool");
  assert_conversion(&value, &rust_type, "false");

  let value = json!("no");
  assert_conversion(&value, &rust_type, "false");

  let value = json!("0");
  assert_conversion(&value, &rust_type, "false");

  let value = json!("anything");
  assert_conversion(&value, &rust_type, "false");
}

#[test]
fn test_null_value() {
  let value = json!(null);
  let rust_type = TypeRef::new("String");
  assert_conversion(&value, &rust_type, "None");
}

#[test]
fn test_nullable_string_with_value() {
  let value = json!("hello");
  let mut rust_type = TypeRef::new("String");
  rust_type.nullable = true;
  assert_conversion(&value, &rust_type, r#"Some ("hello" . to_string ())"#);
}

#[test]
fn test_nullable_int_with_value() {
  let value = json!(42);
  let mut rust_type = TypeRef::new("i64");
  rust_type.nullable = true;
  assert_conversion(&value, &rust_type, "Some (42i64)");
}

#[test]
fn test_nullable_float_with_value() {
  let value = json!(2.5);
  let mut rust_type = TypeRef::new("f64");
  rust_type.nullable = true;
  assert_conversion(&value, &rust_type, "Some (2.5f64)");
}

#[test]
fn test_nullable_bool_with_value() {
  let value = json!(true);
  let mut rust_type = TypeRef::new("bool");
  rust_type.nullable = true;
  assert_conversion(&value, &rust_type, "Some (true)");
}

#[test]
fn test_nullable_with_null() {
  let value = json!(null);
  let mut rust_type = TypeRef::new("String");
  rust_type.nullable = true;
  assert_conversion(&value, &rust_type, "None");
}

#[test]
fn test_array_defaults() {
  let value = json!([1, 2, 3]);
  let rust_type = TypeRef::new("Vec<i64>");
  assert_conversion(&value, &rust_type, "Default :: default ()");
}

#[test]
fn test_object_defaults() {
  let value = json!({"key": "value"});
  let rust_type = TypeRef::new("CustomType");
  assert_conversion(&value, &rust_type, "Default :: default ()");
}

#[test]
fn test_int_types_i32() {
  let value = json!(100);
  let rust_type = TypeRef::new("i32");
  assert_conversion(&value, &rust_type, "100i32");
}

#[test]
fn test_int_types_i16() {
  let value = json!(50);
  let rust_type = TypeRef::new("i16");
  assert_conversion(&value, &rust_type, "50i16");
}

#[test]
fn test_float_types_f32() {
  let value = json!(1.5);
  let rust_type = TypeRef::new("f32");
  assert_conversion(&value, &rust_type, "1.5f32");
}

#[test]
fn test_negative_numbers() {
  let value = json!(-42);
  let rust_type = TypeRef::new("i64");
  assert_conversion(&value, &rust_type, "- 42i64");

  let value = json!(-2.5);
  let rust_type = TypeRef::new("f64");
  assert_conversion(&value, &rust_type, "- 2.5f64");
}

#[test]
fn test_zero_values() {
  let value = json!(0);
  let rust_type = TypeRef::new("i64");
  assert_conversion(&value, &rust_type, "0i64");

  let value = json!(0.0);
  let rust_type = TypeRef::new("f64");
  assert_conversion(&value, &rust_type, "0f64");
}

#[test]
fn test_unsigned_from_number() {
  let value = json!(42);
  let rust_type = TypeRef::new("u32");
  assert_conversion(&value, &rust_type, "42u32");

  let value = json!(255);
  let rust_type = TypeRef::new("u8");
  assert_conversion(&value, &rust_type, "255u8");
}

#[test]
fn test_unsigned_from_string() {
  let value = json!("100");
  let rust_type = TypeRef::new("u64");
  assert_conversion(&value, &rust_type, "100u64");
}

#[test]
fn test_unsigned_invalid_negative() {
  let value = json!(-1);
  let rust_type = TypeRef::new("u32");
  assert_conversion(&value, &rust_type, "Default :: default ()");
}

#[test]
fn test_format_based_type_coercion() {
  let value = json!(100);
  let rust_type = TypeRef::new("i32");
  assert_conversion(&value, &rust_type, "100i32");

  let value = json!(1.5);
  let rust_type = TypeRef::new("f32");
  assert_conversion(&value, &rust_type, "1.5f32");
}

#[test]
fn test_int_type_suffixes() {
  let value = json!(42);
  assert_conversion(&value, &TypeRef::new("i8"), "42i8");
  assert_conversion(&value, &TypeRef::new("i16"), "42i16");
  assert_conversion(&value, &TypeRef::new("i32"), "42i32");
  assert_conversion(&value, &TypeRef::new("i64"), "42i64");
  assert_conversion(&value, &TypeRef::new("i128"), "42i128");
  assert_conversion(&value, &TypeRef::new("isize"), "42isize");
}

#[test]
fn test_uint_type_suffixes() {
  let value = json!(42);
  assert_conversion(&value, &TypeRef::new("u8"), "42u8");
  assert_conversion(&value, &TypeRef::new("u16"), "42u16");
  assert_conversion(&value, &TypeRef::new("u32"), "42u32");
  assert_conversion(&value, &TypeRef::new("u64"), "42u64");
  assert_conversion(&value, &TypeRef::new("u128"), "42u128");
  assert_conversion(&value, &TypeRef::new("usize"), "42usize");
}

#[allow(clippy::approx_constant)]
#[test]
fn test_float_type_suffixes() {
  let value = json!(3.14);
  assert_conversion(&value, &TypeRef::new("f32"), "3.14f32");
  assert_conversion(&value, &TypeRef::new("f64"), "3.14f64");
}

#[test]
fn test_nullable_with_correct_type_suffixes() {
  let value = json!(25);
  let mut rust_type = TypeRef::new("i32");
  rust_type.nullable = true;
  assert_conversion(&value, &rust_type, "Some (25i32)");

  let mut rust_type = TypeRef::new("u32");
  rust_type.nullable = true;
  assert_conversion(&value, &rust_type, "Some (25u32)");

  let value = json!(1.5);
  let mut rust_type = TypeRef::new("f32");
  rust_type.nullable = true;
  assert_conversion(&value, &rust_type, "Some (1.5f32)");
}

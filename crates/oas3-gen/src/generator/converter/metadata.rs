use std::sync::LazyLock;

use num_format::{CustomFormat, Grouping, ToFormattedString};
use oas3::spec::{ObjectSchema, SchemaType, SchemaTypeSet};
use regex::Regex;
use serde_json::Number;

use crate::generator::{
  ast::{RustPrimitive, TypeRef},
  utils::doc_comment_lines,
};

static UNDERSCORE_FORMAT: LazyLock<CustomFormat> = LazyLock::new(|| {
  CustomFormat::builder()
    .grouping(Grouping::Standard)
    .separator("_")
    .build()
    .expect("formatter failed to build.")
});

#[derive(Clone, Default)]
pub(crate) struct FieldMetadata {
  pub docs: Vec<String>,
  pub validation_attrs: Vec<String>,
  pub regex_validation: Option<String>,
  pub default_value: Option<serde_json::Value>,
  pub read_only: bool,
  pub write_only: bool,
  pub deprecated: bool,
  pub multiple_of: Option<serde_json::Number>,
}

impl FieldMetadata {
  pub(crate) fn from_schema(prop_name: &str, is_required: bool, schema: &ObjectSchema) -> Self {
    Self {
      docs: extract_docs(schema.description.as_ref()),
      validation_attrs: extract_validation_attrs(is_required, schema),
      regex_validation: extract_validation_pattern(prop_name, schema).cloned(),
      default_value: extract_default_value(schema),
      read_only: schema.read_only.unwrap_or(false),
      write_only: schema.write_only.unwrap_or(false),
      deprecated: schema.deprecated.unwrap_or(false),
      multiple_of: schema.multiple_of.clone(),
    }
  }
}

pub(crate) fn extract_docs(desc: Option<&String>) -> Vec<String> {
  desc.map_or_else(Vec::new, |d| doc_comment_lines(d))
}

pub fn format_example_value(example: &serde_json::Value, type_ref: &TypeRef) -> String {
  if matches!(example, serde_json::Value::Null) {
    return if type_ref.nullable {
      "None".to_string()
    } else {
      String::new()
    };
  }

  if type_ref.is_array {
    return format_array_example(example, type_ref);
  }

  let inner_formatted = format_base_value(example, type_ref);

  if type_ref.boxed {
    return format!("Box::new({inner_formatted})");
  }

  inner_formatted
}

fn format_array_example(example: &serde_json::Value, type_ref: &TypeRef) -> String {
  let serde_json::Value::Array(items) = example else {
    return "vec![]".to_string();
  };

  if items.is_empty() {
    return "vec![]".to_string();
  }

  let element_type = TypeRef {
    base_type: type_ref.base_type.clone(),
    boxed: type_ref.boxed,
    nullable: false,
    is_array: false,
    unique_items: false,
  };

  let formatted_items: Vec<String> = items
    .iter()
    .map(|item| format_base_value(item, &element_type))
    .collect();

  format!("vec![{}]", formatted_items.join(", "))
}

fn format_base_value(example: &serde_json::Value, type_ref: &TypeRef) -> String {
  match example {
    serde_json::Value::String(s) => format_string_value(s, &type_ref.base_type),
    serde_json::Value::Number(n) => {
      if matches!(type_ref.base_type, RustPrimitive::String) {
        format!("\"{}\"", escape_string_literal(&n.to_string()))
      } else {
        render_number(&type_ref.base_type, n)
      }
    }
    serde_json::Value::Bool(b) => {
      if matches!(type_ref.base_type, RustPrimitive::String) {
        format!("\"{b}\"")
      } else {
        b.to_string()
      }
    }
    serde_json::Value::Null => String::new(),
    serde_json::Value::Array(items) => {
      let formatted_items: Vec<String> = items
        .iter()
        .map(|item| {
          let element_type = TypeRef::new(type_ref.base_type.clone());
          format_base_value(item, &element_type)
        })
        .collect();
      format!("vec![{}]", formatted_items.join(", "))
    }
    serde_json::Value::Object(_) => serde_json::to_string(example).unwrap_or_else(|_| "...".to_string()),
  }
}

fn format_string_value(s: &str, primitive: &RustPrimitive) -> String {
  match primitive {
    RustPrimitive::Date => format_date_constructor(s),
    RustPrimitive::DateTime => format_datetime_constructor(s),
    RustPrimitive::Time => format_time_constructor(s),
    RustPrimitive::Uuid => format!("uuid::Uuid::parse_str(\"{}\").unwrap()", escape_string_literal(s)),
    _ => format!("\"{}\"", escape_string_literal(s)),
  }
}

fn escape_string_literal(s: &str) -> String {
  s.replace('\\', "\\\\")
    .replace('"', "\\\"")
    .replace('\n', "\\n")
    .replace('\r', "\\r")
    .replace('\t', "\\t")
}

fn format_date_constructor(date_str: &str) -> String {
  if let Some((year, month, day)) = parse_date_parts(date_str) {
    format!("chrono::NaiveDate::from_ymd_opt({year}, {month}, {day}).unwrap()")
  } else {
    format!(
      "chrono::NaiveDate::parse_from_str(\"{}\", \"%Y-%m-%d\").unwrap()",
      escape_string_literal(date_str)
    )
  }
}

fn format_datetime_constructor(datetime_str: &str) -> String {
  format!(
    "chrono::DateTime::parse_from_rfc3339(\"{}\").unwrap().with_timezone(&chrono::Utc)",
    escape_string_literal(datetime_str)
  )
}

fn format_time_constructor(time_str: &str) -> String {
  if let Some((hour, minute, second)) = parse_time_parts(time_str) {
    format!("chrono::NaiveTime::from_hms_opt({hour}, {minute}, {second}).unwrap()")
  } else {
    format!(
      "chrono::NaiveTime::parse_from_str(\"{}\", \"%H:%M:%S\").unwrap()",
      escape_string_literal(time_str)
    )
  }
}

fn parse_date_parts(date_str: &str) -> Option<(i32, u32, u32)> {
  let parts: Vec<&str> = date_str.split('-').collect();
  if parts.len() == 3 {
    let year = parts[0].parse().ok()?;
    let month = parts[1].parse().ok()?;
    let day = parts[2].parse().ok()?;
    Some((year, month, day))
  } else {
    None
  }
}

fn parse_time_parts(time_str: &str) -> Option<(u32, u32, u32)> {
  let parts: Vec<&str> = time_str.split(':').collect();
  if parts.len() >= 2 {
    let hour: u32 = parts[0].parse().ok()?;
    let minute: u32 = parts[1].parse().ok()?;
    let second: u32 = if parts.len() >= 3 {
      parts[2].split('.').next()?.parse().ok()?
    } else {
      0
    };

    if hour > 23 || minute > 59 || second > 59 {
      return None;
    }

    Some((hour, minute, second))
  } else {
    None
  }
}

pub(crate) fn extract_default_value(schema: &ObjectSchema) -> Option<serde_json::Value> {
  schema
    .default
    .clone()
    .or_else(|| schema.const_value.clone())
    .or_else(|| {
      if schema.enum_values.len() == 1 {
        schema.enum_values.first().cloned()
      } else {
        None
      }
    })
}

pub(crate) fn extract_validation_pattern<'s>(prop_name: &str, schema: &'s ObjectSchema) -> Option<&'s String> {
  match (schema.schema_type.as_ref(), schema.pattern.as_ref()) {
    (Some(SchemaTypeSet::Single(SchemaType::String)), Some(pattern)) => {
      let is_non_string_format = schema.format.as_ref().is_some_and(|f| {
        matches!(
          f.as_str(),
          "date" | "date-time" | "duration" | "time" | "binary" | "byte" | "uuid"
        )
      });

      if is_non_string_format {
        return None;
      }

      if !schema.enum_values.is_empty() {
        return None;
      }

      if Regex::new(pattern).is_ok() {
        Some(pattern)
      } else {
        eprintln!("Warning: Invalid regex pattern '{pattern}' for property '{prop_name}'");
        None
      }
    }
    _ => None,
  }
}

pub(crate) fn filter_regex_validation(rust_type: &TypeRef, regex: Option<String>) -> Option<String> {
  match &rust_type.base_type {
    RustPrimitive::DateTime | RustPrimitive::Date | RustPrimitive::Time | RustPrimitive::Uuid => None,
    _ => regex,
  }
}

pub(crate) fn extract_validation_attrs(is_required: bool, schema: &ObjectSchema) -> Vec<String> {
  let mut attrs = Vec::new();

  if let Some(ref format) = schema.format {
    match format.as_str() {
      "email" => attrs.push("email".to_string()),
      "uri" | "url" => attrs.push("url".to_string()),
      _ => {}
    }
  }

  if let Some(ref schema_type) = schema.schema_type {
    if matches!(
      schema_type,
      SchemaTypeSet::Single(SchemaType::Number | SchemaType::Integer)
    ) && let Some(range_attr) = build_range_validation_attr(schema, schema_type)
    {
      attrs.push(range_attr);
    }

    if matches!(schema_type, SchemaTypeSet::Single(SchemaType::String))
      && schema.enum_values.is_empty()
      && let Some(length_attr) = build_string_length_validation_attr(is_required, schema)
    {
      attrs.push(length_attr);
    }

    if matches!(schema_type, SchemaTypeSet::Single(SchemaType::Array))
      && let Some(length_attr) = build_array_length_validation_attr(schema)
    {
      attrs.push(length_attr);
    }
  }

  attrs
}

fn build_range_validation_attr(schema: &ObjectSchema, schema_type: &SchemaTypeSet) -> Option<String> {
  let primitive = if matches!(schema_type, SchemaTypeSet::Single(SchemaType::Number)) {
    super::type_resolver::format_to_primitive(schema.format.as_ref()).unwrap_or(RustPrimitive::F64)
  } else {
    super::type_resolver::format_to_primitive(schema.format.as_ref()).unwrap_or(RustPrimitive::I64)
  };

  let mut parts = Vec::<String>::new();
  if let Some(v) = schema.exclusive_minimum.as_ref() {
    parts.push(format!("exclusive_min = {}", render_number(&primitive, v)));
  }
  if let Some(v) = schema.exclusive_maximum.as_ref() {
    parts.push(format!("exclusive_max = {}", render_number(&primitive, v)));
  }
  if let Some(v) = schema.minimum.as_ref() {
    parts.push(format!("min = {}", render_number(&primitive, v)));
  }
  if let Some(v) = schema.maximum.as_ref() {
    parts.push(format!("max = {}", render_number(&primitive, v)));
  }

  if parts.is_empty() {
    None
  } else {
    Some(format!("range({})", parts.join(", ")))
  }
}

fn build_string_length_validation_attr(is_required: bool, schema: &ObjectSchema) -> Option<String> {
  let is_non_string_format = schema.format.as_ref().is_some_and(|f| {
    matches!(
      f.as_str(),
      "date" | "date-time" | "duration" | "time" | "binary" | "byte" | "uuid"
    )
  });

  if is_non_string_format {
    return None;
  }

  let min = schema.min_length.map(|l| l.to_formatted_string(&*UNDERSCORE_FORMAT));
  let max = schema.max_length.map(|l| l.to_formatted_string(&*UNDERSCORE_FORMAT));

  build_length_attribute(min, max, is_required)
}

fn build_array_length_validation_attr(schema: &ObjectSchema) -> Option<String> {
  let min = schema.min_items.map(|l| l.to_formatted_string(&*UNDERSCORE_FORMAT));
  let max = schema.max_items.map(|l| l.to_formatted_string(&*UNDERSCORE_FORMAT));
  build_length_attribute(min, max, false)
}

fn build_length_attribute(min: Option<String>, max: Option<String>, is_required_non_empty: bool) -> Option<String> {
  match (min, max) {
    (Some(min), Some(max)) => Some(format!("length(min = {min}, max = {max})")),
    (Some(min), None) => Some(format!("length(min = {min})")),
    (None, Some(max)) => Some(format!("length(max = {max})")),
    (None, None) if is_required_non_empty => Some("length(min = 1)".to_string()),
    _ => None,
  }
}

fn render_number(primitive: &RustPrimitive, num: &Number) -> String {
  if primitive.is_float() {
    let s = num.to_string();
    if s.contains('.') { s } else { format!("{s}.0") }
  } else if let Some(value) = num.as_i64() {
    render_integer(primitive, value)
  } else if let Some(value) = num.as_u64() {
    render_unsigned_integer(primitive, value)
  } else {
    num.to_string()
  }
}

fn render_integer(primitive: &RustPrimitive, value: i64) -> String {
  match primitive {
    RustPrimitive::I8 if value <= i64::from(i8::MIN) => "i8::MIN".to_string(),
    RustPrimitive::I8 if value >= i64::from(i8::MAX) => "i8::MAX".to_string(),
    RustPrimitive::I8 => format!("{}i8", value.to_formatted_string(&*UNDERSCORE_FORMAT)),
    RustPrimitive::I16 if value <= i64::from(i16::MIN) => "i16::MIN".to_string(),
    RustPrimitive::I16 if value >= i64::from(i16::MAX) => "i16::MAX".to_string(),
    RustPrimitive::I16 => format!("{}i16", value.to_formatted_string(&*UNDERSCORE_FORMAT)),
    RustPrimitive::I32 if value <= i64::from(i32::MIN) => "i32::MIN".to_string(),
    RustPrimitive::I32 if value >= i64::from(i32::MAX) => "i32::MAX".to_string(),
    RustPrimitive::I32 => format!("{}i32", value.to_formatted_string(&*UNDERSCORE_FORMAT)),
    RustPrimitive::I64 => format!("{}i64", value.to_formatted_string(&*UNDERSCORE_FORMAT)),
    _ => value.to_string(),
  }
}

fn render_unsigned_integer(primitive: &RustPrimitive, value: u64) -> String {
  match primitive {
    RustPrimitive::U8 if value >= u64::from(u8::MAX) => "u8::MAX".to_string(),
    RustPrimitive::U8 => format!("{}u8", value.to_formatted_string(&*UNDERSCORE_FORMAT)),
    RustPrimitive::U16 if value >= u64::from(u16::MAX) => "u16::MAX".to_string(),
    RustPrimitive::U16 => format!("{}u16", value.to_formatted_string(&*UNDERSCORE_FORMAT)),
    RustPrimitive::U32 if value >= u64::from(u32::MAX) => "u32::MAX".to_string(),
    RustPrimitive::U32 => format!("{}u32", value.to_formatted_string(&*UNDERSCORE_FORMAT)),
    RustPrimitive::U64 => format!("{}u64", value.to_formatted_string(&*UNDERSCORE_FORMAT)),
    _ => value.to_string(),
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn test_format_example_null_with_option() {
    let type_ref = TypeRef::new(RustPrimitive::String).with_option();
    let example = serde_json::Value::Null;
    assert_eq!(format_example_value(&example, &type_ref), "None");
  }

  #[test]
  fn test_format_example_null_without_option() {
    let type_ref = TypeRef::new(RustPrimitive::String);
    let example = serde_json::Value::Null;
    assert_eq!(format_example_value(&example, &type_ref), "");
  }

  #[test]
  fn test_format_example_string() {
    let type_ref = TypeRef::new(RustPrimitive::String);
    let example = serde_json::Value::String("hello".to_string());
    assert_eq!(format_example_value(&example, &type_ref), "\"hello\"");
  }

  #[test]
  fn test_format_example_number_i32() {
    let type_ref = TypeRef::new(RustPrimitive::I32);
    let example = serde_json::json!(42);
    assert_eq!(format_example_value(&example, &type_ref), "42i32");
  }

  #[test]
  fn test_format_example_number_f64() {
    let type_ref = TypeRef::new(RustPrimitive::F64);
    let example = serde_json::json!(3.14);
    assert_eq!(format_example_value(&example, &type_ref), "3.14");
  }

  #[test]
  fn test_format_example_bool() {
    let type_ref = TypeRef::new(RustPrimitive::Bool);
    let example = serde_json::json!(true);
    assert_eq!(format_example_value(&example, &type_ref), "true");
  }

  #[test]
  fn test_format_example_array_strings() {
    let type_ref = TypeRef::new(RustPrimitive::String).with_vec();
    let example = serde_json::json!(["foo", "bar", "baz"]);
    assert_eq!(
      format_example_value(&example, &type_ref),
      "vec![\"foo\", \"bar\", \"baz\"]"
    );
  }

  #[test]
  fn test_format_example_array_numbers() {
    let type_ref = TypeRef::new(RustPrimitive::I32).with_vec();
    let example = serde_json::json!([1, 2, 3]);
    assert_eq!(format_example_value(&example, &type_ref), "vec![1i32, 2i32, 3i32]");
  }

  #[test]
  fn test_format_example_empty_array() {
    let type_ref = TypeRef::new(RustPrimitive::String).with_vec();
    let example = serde_json::json!([]);
    assert_eq!(format_example_value(&example, &type_ref), "vec![]");
  }

  #[test]
  fn test_format_example_date() {
    let type_ref = TypeRef::new(RustPrimitive::Date);
    let example = serde_json::Value::String("2024-01-15".to_string());
    assert_eq!(
      format_example_value(&example, &type_ref),
      "chrono::NaiveDate::from_ymd_opt(2024, 1, 15).unwrap()"
    );
  }

  #[test]
  fn test_format_example_datetime() {
    let type_ref = TypeRef::new(RustPrimitive::DateTime);
    let example = serde_json::Value::String("2024-01-15T10:30:00Z".to_string());
    assert_eq!(
      format_example_value(&example, &type_ref),
      "chrono::DateTime::parse_from_rfc3339(\"2024-01-15T10:30:00Z\").unwrap().with_timezone(&chrono::Utc)"
    );
  }

  #[test]
  fn test_format_example_time() {
    let type_ref = TypeRef::new(RustPrimitive::Time);
    let example = serde_json::Value::String("14:30:00".to_string());
    assert_eq!(
      format_example_value(&example, &type_ref),
      "chrono::NaiveTime::from_hms_opt(14, 30, 0).unwrap()"
    );
  }

  #[test]
  fn test_format_example_uuid() {
    let type_ref = TypeRef::new(RustPrimitive::Uuid);
    let example = serde_json::Value::String("550e8400-e29b-41d4-a716-446655440000".to_string());
    assert_eq!(
      format_example_value(&example, &type_ref),
      "uuid::Uuid::parse_str(\"550e8400-e29b-41d4-a716-446655440000\").unwrap()"
    );
  }

  #[test]
  fn test_format_example_boxed_string() {
    let type_ref = TypeRef::new(RustPrimitive::String).with_boxed();
    let example = serde_json::Value::String("boxed".to_string());
    assert_eq!(format_example_value(&example, &type_ref), "Box::new(\"boxed\")");
  }

  #[test]
  fn test_format_example_array_of_dates() {
    let type_ref = TypeRef::new(RustPrimitive::Date).with_vec();
    let example = serde_json::json!(["2024-01-15", "2024-02-20"]);
    assert_eq!(
      format_example_value(&example, &type_ref),
      "vec![chrono::NaiveDate::from_ymd_opt(2024, 1, 15).unwrap(), \
       chrono::NaiveDate::from_ymd_opt(2024, 2, 20).unwrap()]"
    );
  }

  #[test]
  fn test_format_example_nested_array() {
    let type_ref = TypeRef::new(RustPrimitive::I32).with_vec();
    let example = serde_json::json!([[1, 2], [3, 4]]);
    let result = format_example_value(&example, &type_ref);
    assert_eq!(result, "vec![vec![1i32, 2i32], vec![3i32, 4i32]]");
  }

  #[test]
  fn test_parse_date_parts_valid() {
    assert_eq!(parse_date_parts("2024-01-15"), Some((2024, 1, 15)));
  }

  #[test]
  fn test_parse_date_parts_invalid() {
    assert_eq!(parse_date_parts("not-a-date"), None);
    assert_eq!(parse_date_parts("2024-01"), None);
  }

  #[test]
  fn test_parse_time_parts_valid() {
    assert_eq!(parse_time_parts("14:30:45"), Some((14, 30, 45)));
    assert_eq!(parse_time_parts("14:30"), Some((14, 30, 0)));
  }

  #[test]
  fn test_parse_time_parts_with_fractional_seconds() {
    assert_eq!(parse_time_parts("14:30:45.123"), Some((14, 30, 45)));
  }

  #[test]
  fn test_parse_time_parts_invalid() {
    assert_eq!(parse_time_parts("not-a-time"), None);
    assert_eq!(parse_time_parts("25:00:00"), None);
  }

  #[test]
  fn test_escape_string_literal_quotes() {
    let type_ref = TypeRef::new(RustPrimitive::String);
    let example = serde_json::Value::String("page_to_fetch : \"001e0010\"".to_string());
    assert_eq!(
      format_example_value(&example, &type_ref),
      "\"page_to_fetch : \\\"001e0010\\\"\""
    );
  }

  #[test]
  fn test_escape_string_literal_backslash() {
    let type_ref = TypeRef::new(RustPrimitive::String);
    let example = serde_json::Value::String("path\\to\\file".to_string());
    assert_eq!(format_example_value(&example, &type_ref), "\"path\\\\to\\\\file\"");
  }

  #[test]
  fn test_escape_string_literal_newline() {
    let type_ref = TypeRef::new(RustPrimitive::String);
    let example = serde_json::Value::String("line1\nline2".to_string());
    assert_eq!(format_example_value(&example, &type_ref), "\"line1\\nline2\"");
  }

  #[test]
  fn test_escape_string_literal_tab() {
    let type_ref = TypeRef::new(RustPrimitive::String);
    let example = serde_json::Value::String("col1\tcol2".to_string());
    assert_eq!(format_example_value(&example, &type_ref), "\"col1\\tcol2\"");
  }

  #[test]
  fn test_escape_string_literal_multiple_escapes() {
    let type_ref = TypeRef::new(RustPrimitive::String);
    let example = serde_json::Value::String("\"quoted\"\n\\backslash\\".to_string());
    assert_eq!(
      format_example_value(&example, &type_ref),
      "\"\\\"quoted\\\"\\n\\\\backslash\\\\\""
    );
  }

  #[test]
  fn test_escape_in_uuid() {
    let type_ref = TypeRef::new(RustPrimitive::Uuid);
    let example = serde_json::Value::String("\"550e8400-e29b-41d4-a716-446655440000\"".to_string());
    assert_eq!(
      format_example_value(&example, &type_ref),
      "uuid::Uuid::parse_str(\"\\\"550e8400-e29b-41d4-a716-446655440000\\\"\").unwrap()"
    );
  }

  #[test]
  fn test_bool_to_string_type_coercion() {
    let type_ref = TypeRef::new(RustPrimitive::String);
    let example = serde_json::json!(true);
    assert_eq!(format_example_value(&example, &type_ref), "\"true\"");

    let example_false = serde_json::json!(false);
    assert_eq!(format_example_value(&example_false, &type_ref), "\"false\"");
  }

  #[test]
  fn test_bool_native_type() {
    let type_ref = TypeRef::new(RustPrimitive::Bool);
    let example = serde_json::json!(true);
    assert_eq!(format_example_value(&example, &type_ref), "true");
  }

  #[test]
  fn test_number_to_string_type_coercion() {
    let type_ref = TypeRef::new(RustPrimitive::String);
    let example = serde_json::json!(2.2);
    assert_eq!(format_example_value(&example, &type_ref), "\"2.2\"");

    let example_int = serde_json::json!(42);
    assert_eq!(format_example_value(&example_int, &type_ref), "\"42\"");
  }

  #[test]
  fn test_number_native_type() {
    let type_ref = TypeRef::new(RustPrimitive::I32);
    let example = serde_json::json!(42);
    assert_eq!(format_example_value(&example, &type_ref), "42i32");
  }

  #[test]
  fn test_option_string_with_bool_example() {
    let type_ref = TypeRef::new(RustPrimitive::String).with_option();
    let example = serde_json::json!(true);
    assert_eq!(format_example_value(&example, &type_ref), "\"true\"");
  }

  #[test]
  fn test_option_string_with_number_example() {
    let type_ref = TypeRef::new(RustPrimitive::String).with_option();
    let example = serde_json::json!(2.2);
    assert_eq!(format_example_value(&example, &type_ref), "\"2.2\"");
  }

  #[test]
  fn test_complete_header_example_flow() {
    let type_ref = TypeRef::new(RustPrimitive::String).with_option();

    let bool_example = serde_json::json!(true);
    let formatted = format_example_value(&bool_example, &type_ref);
    assert_eq!(formatted, "\"true\"");
    let with_to_string = format!("{formatted}.to_string()");
    assert_eq!(with_to_string, "\"true\".to_string()");
    let with_some = format!("Some({with_to_string})");
    assert_eq!(with_some, "Some(\"true\".to_string())");

    let number_example = serde_json::json!(2.2);
    let formatted = format_example_value(&number_example, &type_ref);
    assert_eq!(formatted, "\"2.2\"");
    let with_to_string = format!("{formatted}.to_string()");
    assert_eq!(with_to_string, "\"2.2\".to_string()");
    let with_some = format!("Some({with_to_string})");
    assert_eq!(with_some, "Some(\"2.2\".to_string())");
  }
}

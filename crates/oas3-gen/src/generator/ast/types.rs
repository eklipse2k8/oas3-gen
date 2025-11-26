use std::sync::LazyLock;

use num_format::{CustomFormat, Grouping, ToFormattedString};
use serde::{Deserialize, Serialize};
use serde_json::Number;

static UNDERSCORE_FORMAT: LazyLock<CustomFormat> = LazyLock::new(|| {
  CustomFormat::builder()
    .grouping(Grouping::Standard)
    .separator("_")
    .build()
    .expect("formatter failed to build.")
});

pub(crate) fn format_number_with_underscores<T: ToFormattedString>(value: &T) -> String {
  value.to_formatted_string(&*UNDERSCORE_FORMAT)
}

/// Type reference with wrapper support (Box, Option, Vec)
#[derive(Debug, Clone, Default)]
#[allow(clippy::struct_excessive_bools)]
pub struct TypeRef {
  pub base_type: RustPrimitive,
  pub boxed: bool,
  pub nullable: bool,
  pub is_array: bool,
  pub unique_items: bool,
}

impl TypeRef {
  pub fn new(base_type: impl Into<RustPrimitive>) -> Self {
    Self {
      base_type: base_type.into(),
      boxed: false,
      nullable: false,
      is_array: false,
      unique_items: false,
    }
  }

  pub fn with_option(mut self) -> Self {
    self.nullable = true;
    self
  }

  pub fn with_vec(mut self) -> Self {
    self.is_array = true;
    self
  }

  pub fn with_unique_items(mut self, unique: bool) -> Self {
    self.unique_items = unique;
    self
  }

  pub fn with_boxed(mut self) -> Self {
    self.boxed = true;
    self
  }

  pub fn is_string_like(&self) -> bool {
    matches!(self.base_type, RustPrimitive::String) && !self.is_array
  }

  pub fn is_primitive_type(&self) -> bool {
    matches!(
      self.base_type,
      RustPrimitive::I8
        | RustPrimitive::I16
        | RustPrimitive::I32
        | RustPrimitive::I64
        | RustPrimitive::I128
        | RustPrimitive::Isize
        | RustPrimitive::U8
        | RustPrimitive::U16
        | RustPrimitive::U32
        | RustPrimitive::U64
        | RustPrimitive::U128
        | RustPrimitive::Usize
        | RustPrimitive::F32
        | RustPrimitive::F64
        | RustPrimitive::Bool
    ) && !self.is_array
  }

  /// Get the full Rust type string
  pub fn to_rust_type(&self) -> String {
    let mut result = self.base_type.to_string();

    if self.boxed {
      result = format!("Box<{result}>");
    }

    if self.is_array {
      result = format!("Vec<{result}>");
    }

    if self.nullable {
      result = format!("Option<{result}>");
    }

    result
  }

  pub fn format_example(&self, example: &serde_json::Value) -> String {
    if matches!(example, serde_json::Value::Null) {
      return if self.nullable {
        "None".to_string()
      } else {
        String::new()
      };
    }

    if self.is_array {
      return self.format_array_example(example);
    }

    let inner_formatted = self.base_type.format_value(example);

    if self.boxed {
      return format!("Box::new({inner_formatted})");
    }

    inner_formatted
  }

  fn format_array_example(&self, example: &serde_json::Value) -> String {
    let serde_json::Value::Array(items) = example else {
      return "vec![]".to_string();
    };

    if items.is_empty() {
      return "vec![]".to_string();
    }

    let element_type = TypeRef {
      base_type: self.base_type.clone(),
      boxed: self.boxed,
      nullable: false,
      is_array: false,
      unique_items: false,
    };

    let formatted_items: Vec<String> = items.iter().map(|item| element_type.format_example(item)).collect();

    format!("vec![{}]", formatted_items.join(", "))
  }
}

impl From<RustPrimitive> for TypeRef {
  fn from(primitive: RustPrimitive) -> Self {
    TypeRef::new(primitive)
  }
}

/// Rust primitive and standard library types
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum RustPrimitive {
  #[serde(rename = "i8")]
  I8,
  #[serde(rename = "i16")]
  I16,
  #[serde(rename = "i32")]
  I32,
  #[serde(rename = "i64")]
  I64,
  #[serde(rename = "i128")]
  I128,
  #[serde(rename = "isize")]
  Isize,
  #[serde(rename = "u8")]
  U8,
  #[serde(rename = "u16")]
  U16,
  #[serde(rename = "u32")]
  U32,
  #[serde(rename = "u64")]
  U64,
  #[serde(rename = "u128")]
  U128,
  #[serde(rename = "usize")]
  Usize,
  #[serde(rename = "f32")]
  F32,
  #[serde(rename = "f64")]
  F64,
  #[serde(rename = "bool")]
  Bool,
  #[default]
  #[serde(rename = "String")]
  String,
  #[serde(rename = "Vec<u8>")]
  Bytes,
  #[serde(rename = "chrono::NaiveDate")]
  Date,
  #[serde(rename = "chrono::DateTime<chrono::Utc>")]
  DateTime,
  #[serde(rename = "chrono::NaiveTime")]
  Time,
  #[serde(rename = "chrono::Duration")]
  Duration,
  #[serde(rename = "uuid::Uuid")]
  Uuid,
  #[serde(rename = "serde_json::Value")]
  Value,
  #[serde(rename = "()")]
  Unit,
  Custom(String),
}

impl RustPrimitive {
  pub fn is_float(&self) -> bool {
    matches!(self, RustPrimitive::F32 | RustPrimitive::F64)
  }

  pub fn from_format(format: &str) -> Option<Self> {
    match format {
      "int8" => Some(RustPrimitive::I8),
      "int16" => Some(RustPrimitive::I16),
      "int32" => Some(RustPrimitive::I32),
      "int64" => Some(RustPrimitive::I64),
      "uint8" => Some(RustPrimitive::U8),
      "uint16" => Some(RustPrimitive::U16),
      "uint32" => Some(RustPrimitive::U32),
      "uint64" => Some(RustPrimitive::U64),
      "float" => Some(RustPrimitive::F32),
      "double" => Some(RustPrimitive::F64),
      "date" => Some(RustPrimitive::Date),
      "date-time" => Some(RustPrimitive::DateTime),
      "time" => Some(RustPrimitive::Time),
      "duration" => Some(RustPrimitive::Duration),
      "byte" | "binary" => Some(RustPrimitive::Bytes),
      "uuid" => Some(RustPrimitive::Uuid),
      _ => None,
    }
  }

  pub fn format_value(&self, value: &serde_json::Value) -> String {
    match value {
      serde_json::Value::String(s) => self.format_string_value(s),
      serde_json::Value::Number(n) => {
        if matches!(self, RustPrimitive::String) {
          format!("\"{}\"", escape_string_literal(&n.to_string()))
        } else {
          self.format_number(n)
        }
      }
      serde_json::Value::Bool(b) => {
        if matches!(self, RustPrimitive::String) {
          format!("\"{b}\"")
        } else {
          b.to_string()
        }
      }
      serde_json::Value::Null => String::new(),
      serde_json::Value::Array(items) => {
        let formatted_items: Vec<String> = items.iter().map(|item| self.format_value(item)).collect();
        format!("vec![{}]", formatted_items.join(", "))
      }
      serde_json::Value::Object(_) => serde_json::to_string(value).unwrap_or_else(|_| "...".to_string()),
    }
  }

  pub fn format_number(&self, num: &Number) -> String {
    if self.is_float() {
      let s = num.to_string();
      if s.contains('.') { s } else { format!("{s}.0") }
    } else if let Some(value) = num.as_i64() {
      render_integer(self, value)
    } else if let Some(value) = num.as_u64() {
      render_unsigned_integer(self, value)
    } else {
      num.to_string()
    }
  }

  fn format_string_value(&self, s: &str) -> String {
    match self {
      RustPrimitive::Date => format_date_constructor(s),
      RustPrimitive::DateTime => format_datetime_constructor(s),
      RustPrimitive::Time => format_time_constructor(s),
      RustPrimitive::Uuid => format!("uuid::Uuid::parse_str(\"{}\")?", escape_string_literal(s)),
      _ => format!("\"{}\"", escape_string_literal(s)),
    }
  }
}

impl std::fmt::Display for RustPrimitive {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    let s = match self {
      RustPrimitive::Custom(name) => name,
      _ => &serde_plain::to_string(self).unwrap(),
    };
    write!(f, "{s}")
  }
}

impl std::str::FromStr for RustPrimitive {
  type Err = std::convert::Infallible;

  fn from_str(s: &str) -> Result<Self, Self::Err> {
    Ok(match s {
      "i8" => RustPrimitive::I8,
      "i16" => RustPrimitive::I16,
      "i32" => RustPrimitive::I32,
      "i64" => RustPrimitive::I64,
      "i128" => RustPrimitive::I128,
      "isize" => RustPrimitive::Isize,
      "u8" => RustPrimitive::U8,
      "u16" => RustPrimitive::U16,
      "u32" => RustPrimitive::U32,
      "u64" => RustPrimitive::U64,
      "u128" => RustPrimitive::U128,
      "usize" => RustPrimitive::Usize,
      "f32" => RustPrimitive::F32,
      "f64" => RustPrimitive::F64,
      "bool" => RustPrimitive::Bool,
      "String" => RustPrimitive::String,
      "Vec<u8>" => RustPrimitive::Bytes,
      "chrono::NaiveDate" => RustPrimitive::Date,
      "chrono::DateTime<chrono::Utc>" => RustPrimitive::DateTime,
      "chrono::NaiveTime" => RustPrimitive::Time,
      "chrono::Duration" => RustPrimitive::Duration,
      "uuid::Uuid" => RustPrimitive::Uuid,
      "serde_json::Value" => RustPrimitive::Value,
      "()" => RustPrimitive::Unit,
      custom => RustPrimitive::Custom(custom.to_string()),
    })
  }
}

impl From<&str> for RustPrimitive {
  fn from(s: &str) -> Self {
    s.parse().unwrap()
  }
}

impl From<String> for RustPrimitive {
  fn from(s: String) -> Self {
    RustPrimitive::from(s.as_str())
  }
}

impl From<&String> for RustPrimitive {
  fn from(s: &String) -> Self {
    RustPrimitive::from(s.as_str())
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
    format!("chrono::NaiveDate::from_ymd_opt({year}, {month}, {day})?")
  } else {
    format!(
      "chrono::NaiveDate::parse_from_str(\"{}\", \"%Y-%m-%d\")?",
      escape_string_literal(date_str)
    )
  }
}

fn format_datetime_constructor(datetime_str: &str) -> String {
  format!(
    "chrono::DateTime::parse_from_rfc3339(\"{}\")?.with_timezone(&chrono::Utc)",
    escape_string_literal(datetime_str)
  )
}

fn format_time_constructor(time_str: &str) -> String {
  if let Some((hour, minute, second)) = parse_time_parts(time_str) {
    format!("chrono::NaiveTime::from_hms_opt({hour}, {minute}, {second})?")
  } else {
    format!(
      "chrono::NaiveTime::parse_from_str(\"{}\", \"%H:%M:%S\")?",
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

fn render_integer(primitive: &RustPrimitive, value: i64) -> String {
  match primitive {
    RustPrimitive::I8 if value <= i64::from(i8::MIN) => "i8::MIN".to_string(),
    RustPrimitive::I8 if value >= i64::from(i8::MAX) => "i8::MAX".to_string(),
    RustPrimitive::I8 => format!("{}i8", format_number_with_underscores(&value)),
    RustPrimitive::I16 if value <= i64::from(i16::MIN) => "i16::MIN".to_string(),
    RustPrimitive::I16 if value >= i64::from(i16::MAX) => "i16::MAX".to_string(),
    RustPrimitive::I16 => format!("{}i16", format_number_with_underscores(&value)),
    RustPrimitive::I32 if value <= i64::from(i32::MIN) => "i32::MIN".to_string(),
    RustPrimitive::I32 if value >= i64::from(i32::MAX) => "i32::MAX".to_string(),
    RustPrimitive::I32 => format!("{}i32", format_number_with_underscores(&value)),
    RustPrimitive::I64 => format!("{}i64", format_number_with_underscores(&value)),
    _ => value.to_string(),
  }
}

pub(crate) fn render_unsigned_integer(primitive: &RustPrimitive, value: u64) -> String {
  match primitive {
    RustPrimitive::U8 if value >= u64::from(u8::MAX) => "u8::MAX".to_string(),
    RustPrimitive::U8 => format!("{}u8", format_number_with_underscores(&value)),
    RustPrimitive::U16 if value >= u64::from(u16::MAX) => "u16::MAX".to_string(),
    RustPrimitive::U16 => format!("{}u16", format_number_with_underscores(&value)),
    RustPrimitive::U32 if value >= u64::from(u32::MAX) => "u32::MAX".to_string(),
    RustPrimitive::U32 => format!("{}u32", format_number_with_underscores(&value)),
    RustPrimitive::U64 => format!("{}u64", format_number_with_underscores(&value)),
    _ => value.to_string(),
  }
}

#[cfg(test)]
mod tests {
  use super::*;

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
      ("MyCustomType", RustPrimitive::Custom("MyCustomType".to_string())),
      ("Vec<MyType>", RustPrimitive::Custom("Vec<MyType>".to_string())),
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
      RustPrimitive::Custom("MyType".to_string()),
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
}

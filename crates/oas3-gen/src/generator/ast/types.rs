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

  #[allow(unused)]
  pub fn is_integer(&self) -> bool {
    matches!(
      self,
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
    )
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
  fn test_rust_primitive_from_str_integers() {
    assert_eq!("i8".parse::<RustPrimitive>().unwrap(), RustPrimitive::I8);
    assert_eq!("i16".parse::<RustPrimitive>().unwrap(), RustPrimitive::I16);
    assert_eq!("i32".parse::<RustPrimitive>().unwrap(), RustPrimitive::I32);
    assert_eq!("i64".parse::<RustPrimitive>().unwrap(), RustPrimitive::I64);
    assert_eq!("i128".parse::<RustPrimitive>().unwrap(), RustPrimitive::I128);
    assert_eq!("isize".parse::<RustPrimitive>().unwrap(), RustPrimitive::Isize);
  }

  #[test]
  fn test_rust_primitive_from_str_unsigned() {
    assert_eq!("u8".parse::<RustPrimitive>().unwrap(), RustPrimitive::U8);
    assert_eq!("u16".parse::<RustPrimitive>().unwrap(), RustPrimitive::U16);
    assert_eq!("u32".parse::<RustPrimitive>().unwrap(), RustPrimitive::U32);
    assert_eq!("u64".parse::<RustPrimitive>().unwrap(), RustPrimitive::U64);
    assert_eq!("u128".parse::<RustPrimitive>().unwrap(), RustPrimitive::U128);
    assert_eq!("usize".parse::<RustPrimitive>().unwrap(), RustPrimitive::Usize);
  }

  #[test]
  fn test_rust_primitive_from_str_floats() {
    assert_eq!("f32".parse::<RustPrimitive>().unwrap(), RustPrimitive::F32);
    assert_eq!("f64".parse::<RustPrimitive>().unwrap(), RustPrimitive::F64);
  }

  #[test]
  fn test_rust_primitive_from_str_others() {
    assert_eq!("bool".parse::<RustPrimitive>().unwrap(), RustPrimitive::Bool);
    assert_eq!("String".parse::<RustPrimitive>().unwrap(), RustPrimitive::String);
    assert_eq!("()".parse::<RustPrimitive>().unwrap(), RustPrimitive::Unit);
  }

  #[test]
  fn test_rust_primitive_from_str_special_types() {
    assert_eq!("Vec<u8>".parse::<RustPrimitive>().unwrap(), RustPrimitive::Bytes);
    assert_eq!(
      "chrono::NaiveDate".parse::<RustPrimitive>().unwrap(),
      RustPrimitive::Date
    );
    assert_eq!(
      "chrono::DateTime<chrono::Utc>".parse::<RustPrimitive>().unwrap(),
      RustPrimitive::DateTime
    );
    assert_eq!(
      "chrono::NaiveTime".parse::<RustPrimitive>().unwrap(),
      RustPrimitive::Time
    );
    assert_eq!("uuid::Uuid".parse::<RustPrimitive>().unwrap(), RustPrimitive::Uuid);
    assert_eq!(
      "serde_json::Value".parse::<RustPrimitive>().unwrap(),
      RustPrimitive::Value
    );
  }

  #[test]
  fn test_rust_primitive_from_str_custom() {
    assert_eq!(
      "MyCustomType".parse::<RustPrimitive>().unwrap(),
      RustPrimitive::Custom("MyCustomType".to_string())
    );
    assert_eq!(
      "Vec<MyType>".parse::<RustPrimitive>().unwrap(),
      RustPrimitive::Custom("Vec<MyType>".to_string())
    );
  }

  #[test]
  fn test_rust_primitive_display_round_trip() {
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
      assert_eq!(parsed, primitive, "Round-trip failed for {primitive:?}");
    }
  }

  #[test]
  fn test_type_ref_new_from_str() {
    let type_ref = TypeRef::new("i32");
    assert_eq!(type_ref.base_type, RustPrimitive::I32);
    assert!(!type_ref.nullable);
  }

  #[test]
  fn test_type_ref_new_from_primitive() {
    let type_ref = TypeRef::new(RustPrimitive::F64);
    assert_eq!(type_ref.base_type, RustPrimitive::F64);
  }

  #[test]
  fn test_type_ref_with_wrappers() {
    let type_ref = TypeRef::new("String").with_vec().with_option();
    assert_eq!(type_ref.base_type, RustPrimitive::String);
    assert!(type_ref.is_array);
    assert!(type_ref.nullable);
    assert_eq!(type_ref.to_rust_type(), "Option<Vec<String>>");
  }

  #[test]
  fn test_type_ref_with_box() {
    let type_ref = TypeRef::new("MyType").with_boxed().with_option();
    assert_eq!(type_ref.to_rust_type(), "Option<Box<MyType>>");
  }

  #[test]
  fn test_type_ref_default() {
    let type_ref = TypeRef::default();
    assert_eq!(type_ref.base_type, RustPrimitive::String);
    assert!(!type_ref.nullable);
    assert!(!type_ref.is_array);
    assert!(!type_ref.boxed);
  }

  #[test]
  fn test_rust_primitive_default() {
    let primitive = RustPrimitive::default();
    assert_eq!(primitive, RustPrimitive::String);
  }

  #[test]
  fn test_format_example_null_with_option() {
    let type_ref = TypeRef::new(RustPrimitive::String).with_option();
    let example = serde_json::Value::Null;
    assert_eq!(type_ref.format_example(&example), "None");
  }

  #[test]
  fn test_format_example_null_without_option() {
    let type_ref = TypeRef::new(RustPrimitive::String);
    let example = serde_json::Value::Null;
    assert_eq!(type_ref.format_example(&example), "");
  }

  #[test]
  fn test_format_example_string() {
    let type_ref = TypeRef::new(RustPrimitive::String);
    let example = serde_json::Value::String("hello".to_string());
    assert_eq!(type_ref.format_example(&example), "\"hello\"");
  }

  #[test]
  fn test_format_example_number_i32() {
    let type_ref = TypeRef::new(RustPrimitive::I32);
    let example = serde_json::json!(42);
    assert_eq!(type_ref.format_example(&example), "42i32");
  }

  #[allow(clippy::approx_constant)]
  #[test]
  fn test_format_example_number_f64() {
    let type_ref = TypeRef::new(RustPrimitive::F64);
    let example = serde_json::json!(3.14);
    assert_eq!(type_ref.format_example(&example), "3.14");
  }

  #[test]
  fn test_format_example_bool() {
    let type_ref = TypeRef::new(RustPrimitive::Bool);
    let example = serde_json::json!(true);
    assert_eq!(type_ref.format_example(&example), "true");
  }

  #[test]
  fn test_format_example_array_strings() {
    let type_ref = TypeRef::new(RustPrimitive::String).with_vec();
    let example = serde_json::json!(["foo", "bar", "baz"]);
    assert_eq!(type_ref.format_example(&example), "vec![\"foo\", \"bar\", \"baz\"]");
  }

  #[test]
  fn test_format_example_array_numbers() {
    let type_ref = TypeRef::new(RustPrimitive::I32).with_vec();
    let example = serde_json::json!([1, 2, 3]);
    assert_eq!(type_ref.format_example(&example), "vec![1i32, 2i32, 3i32]");
  }

  #[test]
  fn test_format_example_empty_array() {
    let type_ref = TypeRef::new(RustPrimitive::String).with_vec();
    let example = serde_json::json!([]);
    assert_eq!(type_ref.format_example(&example), "vec![]");
  }

  #[test]
  fn test_format_example_date() {
    let type_ref = TypeRef::new(RustPrimitive::Date);
    let example = serde_json::Value::String("2024-01-15".to_string());
    assert_eq!(
      type_ref.format_example(&example),
      "chrono::NaiveDate::from_ymd_opt(2024, 1, 15)?"
    );
  }

  #[test]
  fn test_format_example_boxed_string() {
    let type_ref = TypeRef::new(RustPrimitive::String).with_boxed();
    let example = serde_json::Value::String("boxed".to_string());
    assert_eq!(type_ref.format_example(&example), "Box::new(\"boxed\")");
  }

  #[test]
  fn test_format_example_datetime() {
    let type_ref = TypeRef::new(RustPrimitive::DateTime);
    let example = serde_json::Value::String("2024-01-15T10:30:00Z".to_string());
    assert_eq!(
      type_ref.format_example(&example),
      "chrono::DateTime::parse_from_rfc3339(\"2024-01-15T10:30:00Z\")?.with_timezone(&chrono::Utc)"
    );
  }

  #[test]
  fn test_format_example_time() {
    let type_ref = TypeRef::new(RustPrimitive::Time);
    let example = serde_json::Value::String("14:30:00".to_string());
    assert_eq!(
      type_ref.format_example(&example),
      "chrono::NaiveTime::from_hms_opt(14, 30, 0)?"
    );
  }

  #[test]
  fn test_format_example_uuid() {
    let type_ref = TypeRef::new(RustPrimitive::Uuid);
    let example = serde_json::Value::String("550e8400-e29b-41d4-a716-446655440000".to_string());
    assert_eq!(
      type_ref.format_example(&example),
      "uuid::Uuid::parse_str(\"550e8400-e29b-41d4-a716-446655440000\")?"
    );
  }

  #[test]
  fn test_format_example_array_of_dates() {
    let type_ref = TypeRef::new(RustPrimitive::Date).with_vec();
    let example = serde_json::json!(["2024-01-15", "2024-02-20"]);
    assert_eq!(
      type_ref.format_example(&example),
      "vec![chrono::NaiveDate::from_ymd_opt(2024, 1, 15)?, \
       chrono::NaiveDate::from_ymd_opt(2024, 2, 20)?]"
    );
  }

  #[test]
  fn test_format_example_nested_array() {
    let type_ref = TypeRef::new(RustPrimitive::I32).with_vec();
    let example = serde_json::json!([[1, 2], [3, 4]]);
    let result = type_ref.format_example(&example);
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
      type_ref.format_example(&example),
      "\"page_to_fetch : \\\"001e0010\\\"\""
    );
  }

  #[test]
  fn test_escape_string_literal_backslash() {
    let type_ref = TypeRef::new(RustPrimitive::String);
    let example = serde_json::Value::String("path\\to\\file".to_string());
    assert_eq!(type_ref.format_example(&example), "\"path\\\\to\\\\file\"");
  }

  #[test]
  fn test_escape_string_literal_newline() {
    let type_ref = TypeRef::new(RustPrimitive::String);
    let example = serde_json::Value::String("line1\nline2".to_string());
    assert_eq!(type_ref.format_example(&example), "\"line1\\nline2\"");
  }

  #[test]
  fn test_escape_string_literal_tab() {
    let type_ref = TypeRef::new(RustPrimitive::String);
    let example = serde_json::Value::String("col1\tcol2".to_string());
    assert_eq!(type_ref.format_example(&example), "\"col1\\tcol2\"");
  }

  #[test]
  fn test_escape_string_literal_multiple_escapes() {
    let type_ref = TypeRef::new(RustPrimitive::String);
    let example = serde_json::Value::String("\"quoted\"\n\\backslash\\".to_string());
    assert_eq!(
      type_ref.format_example(&example),
      "\"\\\"quoted\\\"\\n\\\\backslash\\\\\""
    );
  }

  #[test]
  fn test_escape_in_uuid() {
    let type_ref = TypeRef::new(RustPrimitive::Uuid);
    let example = serde_json::Value::String("\"550e8400-e29b-41d4-a716-446655440000\"".to_string());
    assert_eq!(
      type_ref.format_example(&example),
      "uuid::Uuid::parse_str(\"\\\"550e8400-e29b-41d4-a716-446655440000\\\"\")?"
    );
  }

  #[test]
  fn test_bool_to_string_type_coercion() {
    let type_ref = TypeRef::new(RustPrimitive::String);
    let example = serde_json::json!(true);
    assert_eq!(type_ref.format_example(&example), "\"true\"");

    let example_false = serde_json::json!(false);
    assert_eq!(type_ref.format_example(&example_false), "\"false\"");
  }

  #[test]
  fn test_bool_native_type() {
    let type_ref = TypeRef::new(RustPrimitive::Bool);
    let example = serde_json::json!(true);
    assert_eq!(type_ref.format_example(&example), "true");
  }

  #[test]
  fn test_number_to_string_type_coercion() {
    let type_ref = TypeRef::new(RustPrimitive::String);
    let example = serde_json::json!(2.2);
    assert_eq!(type_ref.format_example(&example), "\"2.2\"");

    let example_int = serde_json::json!(42);
    assert_eq!(type_ref.format_example(&example_int), "\"42\"");
  }

  #[test]
  fn test_number_native_type() {
    let type_ref = TypeRef::new(RustPrimitive::I32);
    let example = serde_json::json!(42);
    assert_eq!(type_ref.format_example(&example), "42i32");
  }

  #[test]
  fn test_option_string_with_bool_example() {
    let type_ref = TypeRef::new(RustPrimitive::String).with_option();
    let example = serde_json::json!(true);
    assert_eq!(type_ref.format_example(&example), "\"true\"");
  }

  #[test]
  fn test_option_string_with_number_example() {
    let type_ref = TypeRef::new(RustPrimitive::String).with_option();
    let example = serde_json::json!(2.2);
    assert_eq!(type_ref.format_example(&example), "\"2.2\"");
  }

  #[test]
  fn test_complete_header_example_flow() {
    let type_ref = TypeRef::new(RustPrimitive::String).with_option();

    let bool_example = serde_json::json!(true);
    let formatted = type_ref.format_example(&bool_example);
    assert_eq!(formatted, "\"true\"");
    let with_to_string = format!("{formatted}.to_string()");
    assert_eq!(with_to_string, "\"true\".to_string()");
    let with_some = format!("Some({with_to_string})");
    assert_eq!(with_some, "Some(\"true\".to_string())");

    let number_example = serde_json::json!(2.2);
    let formatted = type_ref.format_example(&number_example);
    assert_eq!(formatted, "\"2.2\"");
    let with_to_string = format!("{formatted}.to_string()");
    assert_eq!(with_to_string, "\"2.2\".to_string()");
    let with_some = format!("Some({with_to_string})");
    assert_eq!(with_some, "Some(\"2.2\".to_string())");
  }
}

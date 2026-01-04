use std::sync::LazyLock;

use num_format::{CustomFormat, Grouping, ToFormattedString};
use quote::{ToTokens, quote};
use serde::{Deserialize, Serialize};
use serde_json::Number;

use crate::generator::ast::{DefaultAtom, StructToken};

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
#[derive(Debug, Clone, Default, PartialEq, Eq)]
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

  pub fn unwrap_option(mut self) -> Self {
    self.nullable = false;
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

  pub fn unboxed_base_type_name(&self) -> String {
    self.base_type.to_string()
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

  pub fn requires_json_serialization(&self) -> bool {
    self.is_array || matches!(self.base_type, RustPrimitive::Custom(_) | RustPrimitive::Value)
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

impl From<&serde_json::Value> for TypeRef {
  fn from(value: &serde_json::Value) -> Self {
    match value {
      serde_json::Value::String(_) => TypeRef::new(RustPrimitive::String),
      serde_json::Value::Number(n) if n.is_i64() => TypeRef::new(RustPrimitive::I64),
      serde_json::Value::Number(_) => TypeRef::new(RustPrimitive::F64),
      serde_json::Value::Bool(_) => TypeRef::new(RustPrimitive::Bool),
      _ => TypeRef::new(RustPrimitive::Value),
    }
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
  Custom(DefaultAtom),
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
    let s: &str = match self {
      RustPrimitive::Custom(name) => name,
      _ => &serde_plain::to_string(self).unwrap(),
    };
    write!(f, "{s}")
  }
}

impl ToTokens for RustPrimitive {
  fn to_tokens(&self, tokens: &mut proc_macro2::TokenStream) {
    let s = self.to_string();
    let ty: syn::Type = syn::parse_str(&s).unwrap_or_else(|_| panic!("Failed to parse RustPrimitive: {s}"));
    ty.to_tokens(tokens);
  }
}

impl ToTokens for TypeRef {
  fn to_tokens(&self, tokens: &mut proc_macro2::TokenStream) {
    let inner = &self.base_type;
    let mut type_tokens = quote! { #inner };

    if self.boxed {
      type_tokens = quote! { Box<#type_tokens> };
    }

    if self.is_array {
      type_tokens = quote! { Vec<#type_tokens> };
    }

    if self.nullable {
      type_tokens = quote! { Option<#type_tokens> };
    }

    type_tokens.to_tokens(tokens);
  }
}

impl PartialEq<&str> for TypeRef {
  fn eq(&self, other: &&str) -> bool {
    self.to_rust_type() == *other
  }
}

impl PartialEq<String> for TypeRef {
  fn eq(&self, other: &String) -> bool {
    self.to_rust_type() == *other
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
      custom => RustPrimitive::Custom(custom.into()),
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

impl From<StructToken> for RustPrimitive {
  fn from(token: StructToken) -> Self {
    RustPrimitive::Custom(token.to_atom())
  }
}

impl From<&StructToken> for RustPrimitive {
  fn from(token: &StructToken) -> Self {
    RustPrimitive::Custom(token.to_atom())
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

pub(crate) fn parse_date_parts(date_str: &str) -> Option<(i32, u32, u32)> {
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

pub(crate) fn parse_time_parts(time_str: &str) -> Option<(u32, u32, u32)> {
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

use serde::{Deserialize, Serialize};

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
      assert_eq!(parsed, primitive, "Round-trip failed for {:?}", primitive);
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
}

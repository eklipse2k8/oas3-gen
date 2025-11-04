use serde::{Deserialize, Serialize};

/// Discriminated enum variant mapping
#[derive(Debug, Clone)]
pub(crate) struct DiscriminatedVariant {
  pub(crate) discriminator_value: String,
  pub(crate) variant_name: String,
  pub(crate) type_name: String,
}

/// Discriminated enum definition (uses macro for custom ser/de)
#[derive(Debug, Clone)]
pub(crate) struct DiscriminatedEnumDef {
  pub(crate) name: String,
  pub(crate) docs: Vec<String>,
  pub(crate) discriminator_field: String,
  pub(crate) variants: Vec<DiscriminatedVariant>,
  pub(crate) fallback: Option<DiscriminatedVariant>,
}

/// Top-level Rust type representation
#[derive(Debug, Clone)]
pub(crate) enum RustType {
  Struct(StructDef),
  Enum(EnumDef),
  TypeAlias(TypeAliasDef),
  DiscriminatedEnum(DiscriminatedEnumDef),
}

impl RustType {
  pub(crate) fn type_name(&self) -> &str {
    match self {
      RustType::Struct(def) => &def.name,
      RustType::Enum(def) => &def.name,
      RustType::TypeAlias(def) => &def.name,
      RustType::DiscriminatedEnum(def) => &def.name,
    }
  }
}

/// Metadata about an API operation (for tracking, not direct code generation)
#[allow(dead_code)]
#[derive(Debug, Clone)]
pub(crate) struct OperationInfo {
  pub(crate) operation_id: String,
  pub(crate) method: String,
  pub(crate) path: String,
  pub(crate) summary: Option<String>,
  pub(crate) description: Option<String>,
  pub(crate) request_type: Option<String>,
  pub(crate) response_type: Option<String>,
  pub(crate) request_body_types: Vec<String>,
}

/// Semantic kind of a struct to determine code generation behavior
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum StructKind {
  /// Regular OpenAPI schema struct from components.schemas
  #[default]
  Schema,
  /// Operation request struct combining parameters and body (has `render_path` method)
  OperationRequest,
  /// Inline request body struct for an operation
  RequestBody,
}

/// Rust struct definition
#[derive(Debug, Clone)]
pub(crate) struct StructDef {
  pub(crate) name: String,
  pub(crate) docs: Vec<String>,
  pub(crate) fields: Vec<FieldDef>,
  pub(crate) derives: Vec<String>,
  pub(crate) serde_attrs: Vec<String>,
  pub(crate) outer_attrs: Vec<String>,
  pub(crate) methods: Vec<StructMethod>,
  pub(crate) kind: StructKind,
}

/// Associated method definition for a struct
#[derive(Debug, Clone)]
pub(crate) struct StructMethod {
  pub(crate) name: String,
  pub(crate) docs: Vec<String>,
  pub(crate) kind: StructMethodKind,
  pub(crate) attrs: Vec<String>,
}

#[derive(Debug, Clone)]
pub(crate) struct QueryParameter {
  pub(crate) field: String,
  pub(crate) encoded_name: String,
  pub(crate) explode: bool,
  pub(crate) optional: bool,
  pub(crate) is_array: bool,
}

#[derive(Debug, Clone)]
pub(crate) enum StructMethodKind {
  RenderPath {
    segments: Vec<PathSegment>,
    query_params: Vec<QueryParameter>,
  },
}

#[derive(Debug, Clone)]
pub(crate) enum PathSegment {
  Literal(String),
  Parameter { field: String },
}

/// Rust struct field definition
#[derive(Debug, Clone, Default)]
pub(crate) struct FieldDef {
  pub(crate) name: String,
  pub(crate) docs: Vec<String>,
  pub(crate) rust_type: TypeRef,
  pub(crate) serde_attrs: Vec<String>,
  pub(crate) extra_attrs: Vec<String>,
  pub(crate) validation_attrs: Vec<String>,
  pub(crate) regex_validation: Option<String>,
  pub(crate) default_value: Option<serde_json::Value>,
  pub(crate) read_only: bool,
  pub(crate) write_only: bool,
  pub(crate) deprecated: bool,
  pub(crate) multiple_of: Option<serde_json::Number>,
}

/// Rust primitive and standard library types
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub(crate) enum RustPrimitive {
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
  pub(crate) fn is_float(&self) -> bool {
    matches!(self, RustPrimitive::F32 | RustPrimitive::F64)
  }

  #[allow(unused)]
  pub(crate) fn is_integer(&self) -> bool {
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

/// Type reference with wrapper support (Box, Option, Vec)
#[derive(Debug, Clone, Default)]
#[allow(clippy::struct_excessive_bools)]
pub(crate) struct TypeRef {
  pub(crate) base_type: RustPrimitive,
  pub(crate) boxed: bool,
  pub(crate) nullable: bool,
  pub(crate) is_array: bool,
  pub(crate) unique_items: bool,
}

impl TypeRef {
  pub(crate) fn new(base_type: impl Into<RustPrimitive>) -> Self {
    Self {
      base_type: base_type.into(),
      boxed: false,
      nullable: false,
      is_array: false,
      unique_items: false,
    }
  }

  pub(crate) fn with_option(mut self) -> Self {
    self.nullable = true;
    self
  }

  pub(crate) fn with_vec(mut self) -> Self {
    self.is_array = true;
    self
  }

  pub(crate) fn with_unique_items(mut self, unique: bool) -> Self {
    self.unique_items = unique;
    self
  }

  pub(crate) fn with_boxed(mut self) -> Self {
    self.boxed = true;
    self
  }

  /// Get the full Rust type string
  pub(crate) fn to_rust_type(&self) -> String {
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

/// Rust enum definition
#[derive(Debug, Clone)]
pub(crate) struct EnumDef {
  pub(crate) name: String,
  pub(crate) docs: Vec<String>,
  pub(crate) variants: Vec<VariantDef>,
  pub(crate) discriminator: Option<String>,
  pub(crate) derives: Vec<String>,
  pub(crate) serde_attrs: Vec<String>,
  pub(crate) outer_attrs: Vec<String>,
}

/// Rust enum variant definition
#[derive(Debug, Clone)]
pub(crate) struct VariantDef {
  pub(crate) name: String,
  pub(crate) docs: Vec<String>,
  pub(crate) content: VariantContent,
  pub(crate) serde_attrs: Vec<String>,
  pub(crate) deprecated: bool,
}

/// Enum variant content (Unit, Tuple, or Struct)
#[derive(Debug, Clone)]
pub(crate) enum VariantContent {
  Unit,
  Tuple(Vec<TypeRef>),
  Struct(Vec<FieldDef>),
}

/// Type alias definition
#[derive(Debug, Clone)]
pub(crate) struct TypeAliasDef {
  pub(crate) name: String,
  pub(crate) docs: Vec<String>,
  pub(crate) target: TypeRef,
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

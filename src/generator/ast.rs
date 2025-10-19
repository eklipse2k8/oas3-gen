//! Abstract Syntax Tree (AST) types for Rust code generation
//!
//! This module contains the intermediate representation of Rust types
//! that will be generated from OpenAPI schemas.

/// Top-level Rust type representation
#[derive(Debug, Clone)]
pub enum RustType {
  Struct(StructDef),
  Enum(EnumDef),
  TypeAlias(TypeAliasDef),
}

impl RustType {
  pub fn type_name(&self) -> &str {
    match self {
      RustType::Struct(def) => &def.name,
      RustType::Enum(def) => &def.name,
      RustType::TypeAlias(def) => &def.name,
    }
  }
}

/// Metadata about an API operation (for tracking, not direct code generation)
#[derive(Debug, Clone)]
pub struct OperationInfo {
  pub operation_id: String,
  pub method: String,
  pub path: String,
  pub summary: Option<String>,
  pub description: Option<String>,
  pub request_type: Option<String>,
  pub response_type: Option<String>,
}

/// Rust struct definition
#[derive(Debug, Clone)]
pub struct StructDef {
  pub name: String,
  pub docs: Vec<String>,
  pub fields: Vec<FieldDef>,
  pub derives: Vec<String>,
  pub serde_attrs: Vec<String>,
}

/// Rust struct field definition
#[derive(Debug, Clone)]
pub struct FieldDef {
  pub name: String,
  pub docs: Vec<String>,
  pub rust_type: TypeRef,
  pub optional: bool,
  pub serde_attrs: Vec<String>,
  pub validation_attrs: Vec<String>,
  pub regex_validation: Option<String>,
  pub default_value: Option<serde_json::Value>,
  pub read_only: bool,
  pub write_only: bool,
  pub deprecated: bool,
  pub multiple_of: Option<serde_json::Number>,
  pub unique_items: bool,
}

/// Type reference with wrapper support (Box, Option, Vec)
#[derive(Debug, Clone)]
pub struct TypeRef {
  pub base_type: String,
  pub boxed: bool,
  pub nullable: bool,
  pub is_array: bool,
  pub unique_items: bool,
}

impl TypeRef {
  pub fn new(base_type: impl Into<String>) -> Self {
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
    let mut result = self.base_type.clone();

    if self.boxed {
      result = format!("Box<{}>", result);
    }

    if self.is_array {
      if self.unique_items {
        result = format!("indexmap::IndexSet<{}>", result);
      } else {
        result = format!("Vec<{}>", result);
      }
    }

    if self.nullable {
      result = format!("Option<{}>", result);
    }

    result
  }
}

/// Rust enum definition
#[derive(Debug, Clone)]
pub struct EnumDef {
  pub name: String,
  pub docs: Vec<String>,
  pub variants: Vec<VariantDef>,
  pub discriminator: Option<String>,
  pub derives: Vec<String>,
  pub serde_attrs: Vec<String>,
}

/// Rust enum variant definition
#[derive(Debug, Clone)]
pub struct VariantDef {
  pub name: String,
  pub docs: Vec<String>,
  pub content: VariantContent,
  pub serde_attrs: Vec<String>,
  pub deprecated: bool,
}

/// Enum variant content (Unit, Tuple, or Struct)
#[derive(Debug, Clone)]
pub enum VariantContent {
  Unit,
  Tuple(Vec<TypeRef>),
  Struct(Vec<FieldDef>),
}

/// Type alias definition
#[derive(Debug, Clone)]
pub struct TypeAliasDef {
  pub name: String,
  pub docs: Vec<String>,
  pub target: TypeRef,
}

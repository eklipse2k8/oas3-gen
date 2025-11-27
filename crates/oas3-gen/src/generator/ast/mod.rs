mod derives;
pub mod lints;
pub(super) mod serde_attrs;
mod status_codes;
pub mod tokens;
pub(super) mod types;
pub(super) mod validation_attrs;

#[cfg(test)]
mod tests;

use std::collections::BTreeSet;

use derive_builder::Builder;
pub use derives::{DeriveTrait, default_enum_derives, default_struct_derives};
use http::Method;
pub use lints::LintConfig;
pub use serde_attrs::SerdeAttribute;
pub use status_codes::{StatusCodeToken, status_code_to_variant_name};
pub use tokens::{DefaultAtom, EnumToken, EnumVariantToken, MethodNameToken, StructToken};
pub use types::{RustPrimitive, TypeRef};
pub use validation_attrs::{RegexKey, ValidationAttribute};

/// Discriminated enum variant mapping
#[derive(Debug, Clone)]
pub struct DiscriminatedVariant {
  pub discriminator_value: String,
  pub variant_name: String,
  pub type_name: String,
}

/// Discriminated enum definition (uses macro for custom ser/de)
#[derive(Debug, Clone)]
pub struct DiscriminatedEnumDef {
  pub name: EnumToken,
  pub docs: Vec<String>,
  pub discriminator_field: String,
  pub variants: Vec<DiscriminatedVariant>,
  pub fallback: Option<DiscriminatedVariant>,
}

/// Response enum variant definition
#[derive(Debug, Clone)]
pub struct ResponseVariant {
  pub status_code: StatusCodeToken,
  pub variant_name: EnumVariantToken,
  pub description: Option<String>,
  pub schema_type: Option<TypeRef>,
  pub content_category: ContentCategory,
}

/// Response enum definition for operation responses
#[derive(Debug, Clone)]
pub struct ResponseEnumDef {
  pub name: EnumToken,
  pub docs: Vec<String>,
  pub variants: Vec<ResponseVariant>,
  pub request_type: Option<StructToken>,
}

/// Top-level Rust type representation
#[derive(Debug, Clone)]
pub enum RustType {
  Struct(StructDef),
  Enum(EnumDef),
  TypeAlias(TypeAliasDef),
  DiscriminatedEnum(DiscriminatedEnumDef),
  ResponseEnum(ResponseEnumDef),
}

impl RustType {
  pub fn type_name(&self) -> DefaultAtom {
    match self {
      RustType::Struct(def) => def.name.to_atom(),
      RustType::Enum(def) => def.name.to_atom(),
      RustType::TypeAlias(def) => def.name.as_str().into(),
      RustType::DiscriminatedEnum(def) => def.name.to_atom(),
      RustType::ResponseEnum(def) => def.name.to_atom(),
    }
  }
}

/// Metadata about an API operation (for tracking, not direct code generation)
#[derive(Debug, Clone)]
pub struct OperationInfo {
  pub stable_id: String,
  pub operation_id: String,
  pub method: Method,
  pub path: String,
  pub summary: Option<String>,
  pub description: Option<String>,
  pub request_type: Option<StructToken>,
  pub response_type: Option<String>,
  pub response_enum: Option<EnumToken>,
  pub response_content_category: ContentCategory,
  pub success_response_types: Vec<String>,
  pub error_response_types: Vec<String>,
  pub warnings: Vec<String>,
  pub parameters: Vec<OperationParameter>,
  pub body: Option<OperationBody>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ParameterLocation {
  #[default]
  Path,
  Query,
  Header,
  Cookie,
}

#[derive(Debug, Clone, Default)]
pub struct OperationParameter {
  pub original_name: String,
  pub rust_field: String,
  pub location: ParameterLocation,
  pub required: bool,
  pub rust_type: TypeRef,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default)]
pub enum ContentCategory {
  #[default]
  Json,
  FormUrlEncoded,
  Multipart,
  Text,
  Binary,
  Xml,
}

impl ContentCategory {
  #[must_use]
  pub fn from_content_type(content_type: &str) -> Self {
    let ct = content_type.to_ascii_lowercase();
    if ct.contains("json") {
      Self::Json
    } else if ct.contains("x-www-form-urlencoded") {
      Self::FormUrlEncoded
    } else if ct.contains("multipart") {
      Self::Multipart
    } else if ct.contains("text/plain") || ct.contains("text/html") {
      Self::Text
    } else if ct.contains("xml") {
      Self::Xml
    } else if ct.contains("octet-stream")
      || ct.starts_with("image/")
      || ct.starts_with("video/")
      || ct.starts_with("audio/")
      || ct.starts_with("application/pdf")
      || (ct.starts_with("application/") && !ct.contains("json"))
    {
      Self::Binary
    } else {
      Self::Json
    }
  }
}

#[derive(Debug, Clone, Default)]
pub struct OperationBody {
  pub field_name: String,
  pub optional: bool,
  pub content_category: ContentCategory,
}

/// Semantic kind of a struct to determine code generation behavior
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum StructKind {
  /// Regular OpenAPI schema struct from components.schemas
  #[default]
  Schema,
  /// Operation request struct combining parameters and body (has `render_path` method)
  OperationRequest,
  /// Inline request body struct for an operation
  RequestBody,
}

/// Rust struct definition
#[derive(Debug, Clone, Default)]
pub struct StructDef {
  pub name: StructToken,
  pub docs: Vec<String>,
  pub fields: Vec<FieldDef>,
  pub derives: BTreeSet<DeriveTrait>,
  pub serde_attrs: Vec<SerdeAttribute>,
  pub outer_attrs: Vec<String>,
  pub methods: Vec<StructMethod>,
  pub kind: StructKind,
}

/// Associated method definition for a struct
#[derive(Debug, Clone)]
pub struct StructMethod {
  pub name: MethodNameToken,
  pub docs: Vec<String>,
  pub kind: StructMethodKind,
  pub attrs: Vec<String>,
}

#[derive(Debug, Clone, Default)]
pub struct QueryParameter {
  pub field: String,
  pub encoded_name: String,
  pub explode: bool,
  pub optional: bool,
  pub is_array: bool,
  pub style: Option<oas3::spec::ParameterStyle>,
}

#[derive(Debug, Clone)]
pub enum StructMethodKind {
  RenderPath {
    segments: Vec<PathSegment>,
    query_params: Vec<QueryParameter>,
  },
  ParseResponse {
    response_enum: EnumToken,
    variants: Vec<ResponseVariant>,
  },
}

#[derive(Debug, Clone)]
pub enum PathSegment {
  Literal(String),
  Parameter { field: String },
}

/// Associated method definition for an enum
#[derive(Debug, Clone)]
pub struct EnumMethod {
  pub name: String,
  pub docs: Vec<String>,
  pub kind: EnumMethodKind,
}

#[derive(Debug, Clone)]
pub enum EnumMethodKind {
  SimpleConstructor {
    variant_name: String,
    wrapped_type: String,
  },
  ParameterizedConstructor {
    variant_name: String,
    wrapped_type: String,
    param_name: String,
    param_type: String,
  },
}

/// Rust struct field definition
#[derive(Debug, Clone, Default, Builder)]
#[builder(default, setter(into))]
pub struct FieldDef {
  pub name: String,
  pub docs: Vec<String>,
  pub rust_type: TypeRef,
  pub serde_attrs: Vec<SerdeAttribute>,
  pub extra_attrs: Vec<String>,
  pub validation_attrs: Vec<ValidationAttribute>,
  pub default_value: Option<serde_json::Value>,
  pub example_value: Option<serde_json::Value>,
  pub parameter_location: Option<ParameterLocation>,
  pub deprecated: bool,
  pub multiple_of: Option<serde_json::Number>,
}

/// Rust enum definition
#[derive(Debug, Clone, Default)]
pub struct EnumDef {
  pub name: EnumToken,
  pub docs: Vec<String>,
  pub variants: Vec<VariantDef>,
  pub discriminator: Option<String>,
  pub derives: BTreeSet<DeriveTrait>,
  pub serde_attrs: Vec<SerdeAttribute>,
  pub outer_attrs: Vec<String>,
  pub case_insensitive: bool,
  pub methods: Vec<EnumMethod>,
}

/// Rust enum variant definition
#[derive(Debug, Clone, Default)]
pub struct VariantDef {
  pub name: EnumVariantToken,
  pub docs: Vec<String>,
  pub content: VariantContent,
  pub serde_attrs: Vec<SerdeAttribute>,
  pub deprecated: bool,
}

/// Enum variant content (Unit, Tuple, or Struct)
#[derive(Debug, Clone, Default)]
pub enum VariantContent {
  #[default]
  Unit,
  Tuple(Vec<TypeRef>),
}

/// Type alias definition
#[derive(Debug, Clone, Default)]
pub struct TypeAliasDef {
  pub name: String,
  pub docs: Vec<String>,
  pub target: TypeRef,
}

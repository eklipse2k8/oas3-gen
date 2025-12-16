mod derives;
pub mod lints;
mod outer_attrs;
pub(super) mod serde_attrs;
mod status_codes;
pub mod tokens;
pub(super) mod types;
pub(super) mod validation_attrs;

#[cfg(test)]
mod tests;

use derive_builder::Builder;
pub use derives::{DeriveTrait, DerivesProvider, SerdeImpl};
use http::Method;
pub use lints::LintConfig;
pub use outer_attrs::OuterAttr;
pub use serde_attrs::SerdeAttribute;
pub use status_codes::{StatusCodeToken, status_code_to_variant_name};
pub use tokens::{
  DefaultAtom, EnumToken, EnumVariantToken, FieldNameToken, MethodNameToken, StructToken, TypeAliasToken,
};
pub use types::{RustPrimitive, TypeRef};
pub use validation_attrs::{RegexKey, ValidationAttribute};

/// Discriminated enum variant mapping
#[derive(Debug, Clone)]
pub struct DiscriminatedVariant {
  pub discriminator_value: String,
  pub variant_name: String,
  pub type_name: TypeRef,
}

/// Serde mode for discriminated enums
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SerdeMode {
  #[default]
  Both,
  SerializeOnly,
  DeserializeOnly,
}

/// Discriminated enum definition (uses macro for custom ser/de)
#[derive(Debug, Clone, Default, Builder)]
#[builder(default, setter(into))]
pub struct DiscriminatedEnumDef {
  pub name: EnumToken,
  pub docs: Vec<String>,
  pub discriminator_field: String,
  pub variants: Vec<DiscriminatedVariant>,
  pub fallback: Option<DiscriminatedVariant>,
  pub serde_mode: SerdeMode,
}

impl DiscriminatedEnumDef {
  #[must_use]
  pub fn default_variant(&self) -> Option<&DiscriminatedVariant> {
    self.fallback.as_ref().or_else(|| self.variants.first())
  }

  pub fn all_variants(&self) -> impl Iterator<Item = &DiscriminatedVariant> {
    self.variants.iter().chain(self.fallback.as_ref())
  }
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

impl ResponseVariant {
  #[must_use]
  pub fn doc_line(&self) -> String {
    match &self.description {
      Some(desc) => format!("{}: {desc}", self.status_code),
      None => self.status_code.to_string(),
    }
  }
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
      RustType::TypeAlias(def) => def.name.to_atom(),
      RustType::DiscriminatedEnum(def) => def.name.to_atom(),
      RustType::ResponseEnum(def) => def.name.to_atom(),
    }
  }

  #[must_use]
  pub fn is_serializable(&self) -> SerdeImpl {
    match self {
      RustType::Struct(def) => def.is_serializable(),
      RustType::Enum(def) => def.is_serializable(),
      RustType::DiscriminatedEnum(def) => def.is_serializable(),
      RustType::ResponseEnum(def) => def.is_serializable(),
      RustType::TypeAlias(_) => SerdeImpl::None,
    }
  }

  #[must_use]
  pub fn is_deserializable(&self) -> SerdeImpl {
    match self {
      RustType::Struct(def) => def.is_deserializable(),
      RustType::Enum(def) => def.is_deserializable(),
      RustType::DiscriminatedEnum(def) => def.is_deserializable(),
      RustType::ResponseEnum(def) => def.is_deserializable(),
      RustType::TypeAlias(_) => SerdeImpl::None,
    }
  }
}

/// Metadata about an API operation (for tracking, not direct code generation)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OperationKind {
  Http,
  Webhook,
}

#[derive(Debug, Clone)]
pub struct OperationInfo {
  pub stable_id: String,
  pub operation_id: String,
  pub method: Method,
  pub path: String,
  pub kind: OperationKind,
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
  pub rust_field: FieldNameToken,
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
  pub field_name: FieldNameToken,
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
  pub serde_attrs: Vec<SerdeAttribute>,
  pub outer_attrs: Vec<OuterAttr>,
  pub methods: Vec<StructMethod>,
  pub kind: StructKind,
  pub serde_mode: SerdeMode,
}

impl StructDef {
  #[must_use]
  pub fn has_default(&self) -> bool {
    self.derives().contains(&DeriveTrait::Default)
  }

  pub fn required_fields(&self) -> impl Iterator<Item = &FieldDef> {
    self.fields.iter().filter(|f| f.is_required())
  }
}

/// Associated method definition for a struct
#[derive(Debug, Clone, Default)]
pub struct StructMethod {
  pub name: MethodNameToken,
  pub docs: Vec<String>,
  pub kind: StructMethodKind,
}

#[derive(Debug, Clone, Default)]
pub struct QueryParameter {
  pub field: FieldNameToken,
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

impl Default for StructMethodKind {
  fn default() -> Self {
    Self::RenderPath {
      segments: vec![],
      query_params: vec![],
    }
  }
}

#[derive(Debug, Clone)]
pub enum PathSegment {
  Literal(String),
  Parameter { field: FieldNameToken },
}

/// Associated method definition for an enum
#[derive(Debug, Clone)]
pub struct EnumMethod {
  pub name: MethodNameToken,
  pub docs: Vec<String>,
  pub kind: EnumMethodKind,
}

impl EnumMethod {
  pub fn new(name: impl Into<MethodNameToken>, variant_name: &EnumVariantToken, kind: EnumMethodKind) -> Self {
    let docs = match &kind {
      EnumMethodKind::SimpleConstructor { .. } => {
        vec![format!("Creates a `{variant_name}` variant with default values.")]
      }
      EnumMethodKind::ParameterizedConstructor { param_name, .. } => {
        vec![format!(
          "Creates a `{variant_name}` variant with the specified `{param_name}`."
        )]
      }
    };
    Self {
      name: name.into(),
      docs,
      kind,
    }
  }
}

#[derive(Debug, Clone)]
pub enum EnumMethodKind {
  SimpleConstructor {
    variant_name: EnumVariantToken,
    wrapped_type: TypeRef,
  },
  ParameterizedConstructor {
    variant_name: EnumVariantToken,
    wrapped_type: TypeRef,
    param_name: String,
    param_type: TypeRef,
  },
}

/// Rust struct field definition
#[derive(Debug, Clone, Default, Builder)]
#[builder(default, setter(into))]
pub struct FieldDef {
  pub name: FieldNameToken,
  pub docs: Vec<String>,
  pub rust_type: TypeRef,
  pub serde_attrs: Vec<SerdeAttribute>,
  /// Whether to emit `#[doc(hidden)]` for this field (used for discriminator fields)
  pub doc_hidden: bool,
  pub validation_attrs: Vec<ValidationAttribute>,
  pub default_value: Option<serde_json::Value>,
  pub example_value: Option<serde_json::Value>,
  pub parameter_location: Option<ParameterLocation>,
  pub deprecated: bool,
  pub multiple_of: Option<serde_json::Number>,
}

impl FieldDef {
  #[must_use]
  pub fn is_required(&self) -> bool {
    self.default_value.is_none() && !self.rust_type.nullable
  }
}

/// Rust enum definition
#[derive(Debug, Clone, Default)]
pub struct EnumDef {
  pub name: EnumToken,
  pub docs: Vec<String>,
  pub variants: Vec<VariantDef>,
  pub discriminator: Option<String>,
  pub serde_attrs: Vec<SerdeAttribute>,
  pub outer_attrs: Vec<OuterAttr>,
  pub case_insensitive: bool,
  pub methods: Vec<EnumMethod>,
  pub serde_mode: SerdeMode,
}

impl EnumDef {
  #[must_use]
  pub fn fallback_variant(&self) -> Option<&VariantDef> {
    const FALLBACK_NAMES: &[&str] = &["Unknown", "Other"];
    self.variants.iter().find(|v| FALLBACK_NAMES.contains(&v.name.as_str()))
  }
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

impl VariantDef {
  #[must_use]
  pub fn serde_name(&self) -> String {
    self
      .serde_attrs
      .iter()
      .find_map(|attr| match attr {
        SerdeAttribute::Rename(val) => Some(val.clone()),
        _ => None,
      })
      .unwrap_or_else(|| self.name.to_string())
  }

  #[must_use]
  pub fn single_wrapped_type(&self) -> Option<&TypeRef> {
    self.content.single_type()
  }

  #[must_use]
  pub fn unboxed_type_name(&self) -> Option<String> {
    self.content.single_type().map(TypeRef::unboxed_base_type_name)
  }
}

/// Enum variant content (Unit, Tuple, or Struct)
#[derive(Debug, Clone, Default)]
pub enum VariantContent {
  #[default]
  Unit,
  Tuple(Vec<TypeRef>),
}

impl VariantContent {
  #[must_use]
  pub fn tuple_types(&self) -> Option<&[TypeRef]> {
    match self {
      Self::Unit => None,
      Self::Tuple(types) => Some(types),
    }
  }

  #[must_use]
  pub fn single_type(&self) -> Option<&TypeRef> {
    match self {
      Self::Tuple(types) if types.len() == 1 => Some(&types[0]),
      _ => None,
    }
  }
}

/// Type alias definition
#[derive(Debug, Clone, Default)]
pub struct TypeAliasDef {
  pub name: TypeAliasToken,
  pub docs: Vec<String>,
  pub target: TypeRef,
}

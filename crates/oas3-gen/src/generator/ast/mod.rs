mod derives;
mod documentation;
pub mod lints;
mod metadata;
mod outer_attrs;
mod parsed_path;
pub(super) mod serde_attrs;
mod status_codes;
pub mod tokens;
pub(super) mod types;
pub(super) mod validation_attrs;

#[cfg(test)]
mod tests;

use derive_builder::Builder;
pub use derives::{DeriveTrait, DerivesProvider, SerdeImpl};
pub use documentation::Documentation;
use http::Method;
pub use lints::LintConfig;
use mediatype::MediaType;
pub use metadata::CodeMetadata;
pub use outer_attrs::{OuterAttr, SerdeAsFieldAttr, SerdeAsSeparator};
pub use parsed_path::ParsedPath;
#[cfg(test)]
pub use parsed_path::{PathParseError, PathSegment};
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
  pub discriminator_values: Vec<String>,
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
  pub docs: Documentation,
  pub discriminator_field: String,
  pub variants: Vec<DiscriminatedVariant>,
  pub fallback: Option<DiscriminatedVariant>,
  pub serde_mode: SerdeMode,
  pub methods: Vec<EnumMethod>,
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
  pub media_types: Vec<ResponseMediaType>,
  pub schema_type: Option<TypeRef>,
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
#[derive(Debug, Clone, Default)]
pub struct ResponseEnumDef {
  pub name: EnumToken,
  pub docs: Documentation,
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
  pub path: ParsedPath,
  pub path_template: String,
  pub kind: OperationKind,
  pub summary: Option<String>,
  pub description: Option<String>,
  pub request_type: Option<StructToken>,
  pub response_type: Option<String>,
  pub response_enum: Option<EnumToken>,
  pub response_media_types: Vec<ResponseMediaType>,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default, Hash)]
pub enum ContentCategory {
  #[default]
  Json,
  FormUrlEncoded,
  Multipart,
  Text,
  Binary,
  Xml,
  EventStream,
}

impl ContentCategory {
  #[must_use]
  pub fn from_content_type(content_type: &str) -> Self {
    let Some(media) = MediaType::parse(content_type).ok() else {
      return Self::Json;
    };

    match (media.ty.as_str(), media.subty.as_str()) {
      ("multipart", _) => Self::Multipart,
      ("text", "event-stream") => Self::EventStream,
      ("text" | "application", "xml") => Self::Xml,
      ("application", "x-www-form-urlencoded") => Self::FormUrlEncoded,
      ("application", "json") => Self::Json,
      ("image" | "audio" | "video", _) | ("application", "pdf" | "octet-stream") => Self::Binary,
      ("application" | "text", _) => Self::Text,
      _ => Self::Json,
    }
  }
}

/// Media type information for a response variant.
///
/// Stores the parsed category (for determining parsing strategy) and the schema
/// type for this specific media type (since different content types can have
/// different schemas).
#[derive(Debug, Clone)]
pub struct ResponseMediaType {
  pub category: ContentCategory,
  pub schema_type: Option<TypeRef>,
}

impl ResponseMediaType {
  #[must_use]
  pub fn new(content_type: &str) -> Self {
    Self {
      category: ContentCategory::from_content_type(content_type),
      schema_type: None,
    }
  }

  #[must_use]
  pub fn with_schema(content_type: &str, schema_type: Option<TypeRef>) -> Self {
    Self {
      category: ContentCategory::from_content_type(content_type),
      schema_type,
    }
  }

  #[must_use]
  pub fn primary_category(media_types: &[Self]) -> ContentCategory {
    media_types.first().map_or(ContentCategory::Json, |m| m.category)
  }

  #[must_use]
  pub fn has_event_stream(media_types: &[Self]) -> bool {
    media_types.iter().any(|m| m.category == ContentCategory::EventStream)
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
  /// Regular OpenAPI schema struct from components.schemas (includes inline request/response bodies)
  #[default]
  Schema,
  /// Operation request struct combining parameters and body (has `render_path` method)
  OperationRequest,
  /// Nested struct for path parameters (no serde, just storage)
  PathParams,
  /// Nested struct for query parameters (implements Serialize for reqwest's .query())
  QueryParams,
  /// Nested struct for header parameters (no serde, just storage)
  HeaderParams,
}

/// Rust struct definition
#[derive(Debug, Clone, Default)]
pub struct StructDef {
  pub name: StructToken,
  pub docs: Documentation,
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

  #[must_use]
  pub fn has_validation_attrs(&self) -> bool {
    self.fields.iter().any(|f| !f.validation_attrs.is_empty())
  }

  pub fn required_fields(&self) -> impl Iterator<Item = &FieldDef> {
    self.fields.iter().filter(|f| f.is_required())
  }
}

/// Associated method definition for a struct
#[derive(Debug, Clone)]
pub struct StructMethod {
  pub name: MethodNameToken,
  pub docs: Documentation,
  pub kind: StructMethodKind,
}

#[derive(Debug, Clone)]
pub enum StructMethodKind {
  ParseResponse {
    response_enum: EnumToken,
    variants: Vec<ResponseVariant>,
  },
}

/// Associated method definition for an enum
#[derive(Debug, Clone)]
pub struct EnumMethod {
  pub name: MethodNameToken,
  pub docs: Documentation,
  pub kind: EnumMethodKind,
}

impl EnumMethod {
  pub fn new(name: impl Into<MethodNameToken>, kind: EnumMethodKind, docs: impl Into<Documentation>) -> Self {
    Self {
      name: name.into(),
      docs: docs.into(),
      kind,
    }
  }
}

#[derive(Debug, Clone)]
#[allow(clippy::enum_variant_names)]
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
  KnownValueConstructor {
    known_type: EnumToken,
    known_variant: EnumVariantToken,
  },
}

/// Rust struct field definition
#[derive(Debug, Clone, Default, Builder)]
#[builder(default, setter(into))]
pub struct FieldDef {
  pub name: FieldNameToken,
  pub docs: Documentation,
  pub rust_type: TypeRef,
  pub serde_attrs: Vec<SerdeAttribute>,
  pub serde_as_attr: Option<SerdeAsFieldAttr>,
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
  pub docs: Documentation,
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
  pub docs: Documentation,
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
  pub docs: Documentation,
  pub target: TypeRef,
}

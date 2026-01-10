mod client;
pub mod constants;
mod derives;
mod documentation;
pub mod lints;
mod outer_attrs;
mod parsed_path;
pub(super) mod serde_attrs;
mod status_codes;
pub mod tokens;
pub(super) mod types;
pub(super) mod validation_attrs;

#[cfg(test)]
mod tests;

use std::collections::BTreeSet;

pub use client::ClientDef;
pub use derives::{DeriveTrait, DerivesProvider, SerdeImpl};
pub use documentation::Documentation;
use http::Method;
pub use lints::LintConfig;
use mediatype::MediaType;
use oas3::spec::{ParameterIn, ParameterStyle};
pub use outer_attrs::{OuterAttr, SerdeAsFieldAttr, SerdeAsSeparator};
pub use parsed_path::ParsedPath;
#[cfg(test)]
pub use parsed_path::{PathParseError, PathSegment};
pub use serde_attrs::SerdeAttribute;
pub use status_codes::StatusCodeToken;
pub use tokens::{
  DefaultAtom, EnumToken, EnumVariantToken, FieldNameToken, MethodNameToken, StructToken, TypeAliasToken,
};
pub use types::{RustPrimitive, TypeRef};
pub use validation_attrs::{RegexKey, ValidationAttribute};

/// Discriminated enum variant mapping
#[derive(Debug, Clone, Default, PartialEq, Eq, bon::Builder)]
pub struct DiscriminatedVariant {
  #[builder(default)]
  pub discriminator_values: Vec<String>,
  pub variant_name: EnumVariantToken,
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
#[derive(Debug, Clone, Default, PartialEq, Eq, bon::Builder)]
pub struct DiscriminatedEnumDef {
  pub name: EnumToken,
  #[builder(default)]
  pub docs: Documentation,
  pub discriminator_field: String,
  #[builder(default)]
  pub variants: Vec<DiscriminatedVariant>,
  pub fallback: Option<DiscriminatedVariant>,
  #[builder(default)]
  pub serde_mode: SerdeMode,
  #[builder(default)]
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
#[derive(Debug, Clone, Default, PartialEq, Eq, bon::Builder)]
pub struct ResponseVariant {
  pub variant_name: EnumVariantToken,
  #[builder(default)]
  pub status_code: StatusCodeToken,
  pub description: Option<String>,
  #[builder(default)]
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

#[derive(Debug, Clone, Default, PartialEq, Eq, bon::Builder)]
pub struct ResponseVariantCategory {
  pub category: ContentCategory,
  pub variant: ResponseVariant,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResponseStatusCategory {
  Single(ResponseVariantCategory),
  ContentDispatch {
    streams: Vec<ResponseVariantCategory>,
    variants: Vec<ResponseVariantCategory>,
  },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatusHandler {
  pub status_code: StatusCodeToken,
  pub dispatch: ResponseStatusCategory,
}

/// Response enum definition for operation responses
#[derive(Debug, Clone, Default, PartialEq, Eq, bon::Builder)]
pub struct ResponseEnumDef {
  pub name: EnumToken,
  #[builder(default)]
  pub docs: Documentation,
  #[builder(default)]
  pub variants: Vec<ResponseVariant>,
  pub request_type: Option<StructToken>,
}

/// Top-level Rust type representation
#[derive(Debug, Clone, PartialEq, Eq)]
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

  pub fn type_priority(&self) -> u8 {
    match self {
      RustType::Struct(_) => 0,
      RustType::ResponseEnum(_) => 1,
      RustType::DiscriminatedEnum(_) => 2,
      RustType::Enum(_) => 3,
      RustType::TypeAlias(_) => 4,
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

#[derive(Debug, Clone, bon::Builder)]
pub struct OperationInfo {
  #[builder(into)]
  pub stable_id: String,
  #[builder(into)]
  pub operation_id: String,
  pub method: Method,
  pub path: ParsedPath,
  pub kind: OperationKind,
  pub request_type: Option<StructToken>,
  pub response_type: Option<String>,
  pub response_enum: Option<EnumToken>,
  #[builder(default)]
  pub response_media_types: Vec<ResponseMediaType>,
  #[builder(default)]
  pub warnings: Vec<String>,
  #[builder(default)]
  pub parameters: Vec<FieldDef>,
  pub body: Option<OperationBody>,
  #[builder(default)]
  pub documentation: Documentation,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ParameterLocation {
  #[default]
  Path,
  Query,
  Header,
  Cookie,
}

impl From<ParameterIn> for ParameterLocation {
  fn from(value: ParameterIn) -> Self {
    match value {
      ParameterIn::Path => Self::Path,
      ParameterIn::Query => Self::Query,
      ParameterIn::Header => Self::Header,
      ParameterIn::Cookie => Self::Cookie,
    }
  }
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

    let suffix = media.suffix.as_ref().map(mediatype::Name::as_str);

    match (media.ty.as_str(), media.subty.as_str(), suffix) {
      ("multipart", _, _) => Self::Multipart,
      ("text", "event-stream", _) => Self::EventStream,
      ("text" | "application", "xml", _) | (_, _, Some("xml")) => Self::Xml,
      ("application", "x-www-form-urlencoded", _) => Self::FormUrlEncoded,
      ("application", "json", _) | (_, _, Some("json")) => Self::Json,
      ("image" | "audio" | "video", _, _) | ("application", "pdf" | "octet-stream", _) => Self::Binary,
      ("application" | "text", _, _) => Self::Text,
      _ => Self::Json,
    }
  }

  #[must_use]
  pub const fn variant_suffix(self) -> &'static str {
    match self {
      Self::Json => "",
      Self::Binary => "Binary",
      Self::Text => "Text",
      Self::Xml => "Xml",
      Self::EventStream => "EventStream",
      Self::FormUrlEncoded => "Form",
      Self::Multipart => "Multipart",
    }
  }
}

impl EnumVariantToken {
  pub fn with_content_suffix(self, category: ContentCategory) -> Self {
    Self::new(format!("{}{}", self, category.variant_suffix()))
  }
}

/// Media type information for a response variant.
///
/// Stores the parsed category (for determining parsing strategy) and the schema
/// type for this specific media type (since different content types can have
/// different schemas).
#[derive(Debug, Clone, PartialEq, Eq)]
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

#[derive(Debug, Clone, Default, bon::Builder)]
pub struct MultipartFieldInfo {
  pub name: FieldNameToken,
  #[builder(default)]
  pub nullable: bool,
  #[builder(default)]
  pub is_bytes: bool,
  #[builder(default)]
  pub requires_json: bool,
}

#[derive(Debug, Clone, Default, bon::Builder)]
pub struct OperationBody {
  pub field_name: FieldNameToken,
  #[builder(default)]
  pub optional: bool,
  #[builder(default)]
  pub content_category: ContentCategory,
  pub multipart_fields: Option<Vec<MultipartFieldInfo>>,
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
#[derive(Debug, Clone, Default, PartialEq, Eq, bon::Builder)]
pub struct StructDef {
  #[builder(into)]
  pub name: StructToken,
  #[builder(default)]
  pub docs: Documentation,
  #[builder(default)]
  pub fields: Vec<FieldDef>,
  #[builder(default)]
  pub serde_attrs: Vec<SerdeAttribute>,
  #[builder(default)]
  pub outer_attrs: Vec<OuterAttr>,
  #[builder(default)]
  pub methods: Vec<StructMethod>,
  pub kind: StructKind,
  #[builder(default)]
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

  pub fn user_fields(&self) -> impl Iterator<Item = &FieldDef> {
    self.fields.iter().filter(|f| !f.doc_hidden)
  }
}

/// Associated method definition for a struct
#[derive(Debug, Clone, PartialEq, Eq, bon::Builder)]
pub struct StructMethod {
  pub name: MethodNameToken,
  pub docs: Documentation,
  pub kind: StructMethodKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StructMethodKind {
  ParseResponse {
    response_enum: EnumToken,
    status_handlers: Vec<StatusHandler>,
    default_handler: Option<ResponseVariantCategory>,
  },
  Builder {
    fields: Vec<BuilderField>,
    nested_structs: Vec<BuilderNestedStruct>,
  },
}

#[derive(Debug, Clone, Default, PartialEq, Eq, bon::Builder)]
pub struct BuilderField {
  pub name: FieldNameToken,
  pub rust_type: TypeRef,
  pub owner_field: Option<FieldNameToken>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, bon::Builder)]
pub struct BuilderNestedStruct {
  pub field_name: FieldNameToken,
  pub struct_name: StructToken,
  #[builder(default)]
  pub field_names: Vec<FieldNameToken>,
}

/// Associated method definition for an enum
#[derive(Debug, Clone, Default, PartialEq, Eq, bon::Builder)]
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

#[derive(Debug, Clone, PartialEq, Eq)]
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

impl Default for EnumMethodKind {
  fn default() -> Self {
    Self::SimpleConstructor {
      variant_name: EnumVariantToken::from_raw("Default"),
      wrapped_type: TypeRef::default(),
    }
  }
}

/// Rust struct field definition
#[derive(Debug, Clone, Default, PartialEq, Eq, bon::Builder)]
pub struct FieldDef {
  pub name: FieldNameToken,
  #[builder(default)]
  pub docs: Documentation,
  pub rust_type: TypeRef,
  #[builder(default)]
  pub serde_attrs: BTreeSet<SerdeAttribute>,
  pub serde_as_attr: Option<SerdeAsFieldAttr>,
  #[builder(default)]
  pub doc_hidden: bool,
  #[builder(default)]
  pub validation_attrs: Vec<ValidationAttribute>,
  pub default_value: Option<serde_json::Value>,
  pub example_value: Option<serde_json::Value>,
  #[builder(into)]
  pub parameter_location: Option<ParameterLocation>,
  #[builder(default)]
  pub deprecated: bool,
  pub multiple_of: Option<serde_json::Number>,
  #[builder(into)]
  pub original_name: Option<String>,
}

impl FieldDef {
  #[must_use]
  pub fn is_required(&self) -> bool {
    self.default_value.is_none() && !self.rust_type.nullable
  }

  #[must_use]
  pub fn with_discriminator_behavior(mut self, discriminator_value: Option<&str>, is_base: bool) -> Self {
    self.docs.clear();
    self.validation_attrs.clear();
    self.doc_hidden = true;

    if let Some(value) = discriminator_value {
      self.default_value = Some(serde_json::Value::String(value.to_string()));
      self.serde_attrs.insert(SerdeAttribute::SkipDeserializing);
      self.serde_attrs.insert(SerdeAttribute::Default);
    } else if is_base {
      self.serde_attrs.clear();
      self.serde_attrs.insert(SerdeAttribute::Skip);
      if self.rust_type.is_string_like() {
        self.default_value = Some(serde_json::Value::String(String::new()));
      }
    }

    self
  }

  #[must_use]
  pub fn with_serde_attributes(mut self, explode: bool, style: Option<ParameterStyle>) -> Self {
    if let Some(original) = &self.original_name
      && self.name != original.as_str()
    {
      self.serde_attrs.insert(SerdeAttribute::Rename(original.clone()));
    }

    if self.rust_type.is_array && !explode {
      let separator = match style {
        Some(ParameterStyle::SpaceDelimited) => SerdeAsSeparator::Space,
        Some(ParameterStyle::PipeDelimited) => SerdeAsSeparator::Pipe,
        _ => SerdeAsSeparator::Comma,
      };

      self.serde_as_attr = Some(SerdeAsFieldAttr::SeparatedList {
        separator,
        optional: self.rust_type.nullable,
      });
    }

    self
  }
}

pub trait FieldCollection: Send + Sync {
  #[must_use]
  fn has_serde_as(&self) -> bool;
}

impl FieldCollection for [FieldDef] {
  fn has_serde_as(&self) -> bool {
    self.iter().any(|t| t.serde_as_attr.is_some())
  }
}

/// Rust enum definition
#[derive(Debug, Clone, Default, PartialEq, Eq, bon::Builder)]
pub struct EnumDef {
  pub name: EnumToken,
  pub docs: Documentation,
  pub variants: Vec<VariantDef>,
  pub discriminator: Option<String>,
  #[builder(default)]
  pub serde_attrs: Vec<SerdeAttribute>,
  #[builder(default)]
  pub outer_attrs: Vec<OuterAttr>,
  #[builder(default)]
  pub case_insensitive: bool,
  #[builder(default)]
  pub methods: Vec<EnumMethod>,
  #[builder(default)]
  pub serde_mode: SerdeMode,
  #[builder(default)]
  pub generate_display: bool,
}

impl EnumDef {
  #[must_use]
  pub fn fallback_variant(&self) -> Option<&VariantDef> {
    const FALLBACK_NAMES: &[&str] = &["Unknown", "Other"];
    self.variants.iter().find(|v| FALLBACK_NAMES.contains(&v.name.as_str()))
  }
}

/// Rust enum variant definition
#[derive(Debug, Clone, Default, PartialEq, Eq, bon::Builder)]
pub struct VariantDef {
  pub name: EnumVariantToken,
  #[builder(default)]
  pub docs: Documentation,
  pub content: VariantContent,
  #[builder(default)]
  pub serde_attrs: Vec<SerdeAttribute>,
  #[builder(default)]
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

  pub fn add_alias(&mut self, value: impl Into<String>) {
    self.serde_attrs.push(SerdeAttribute::Alias(value.into()));
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
#[derive(Debug, Clone, Default, PartialEq, Eq)]
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
#[derive(Debug, Clone, Default, PartialEq, Eq, bon::Builder)]
pub struct TypeAliasDef {
  pub name: TypeAliasToken,
  pub docs: Documentation,
  pub target: TypeRef,
}

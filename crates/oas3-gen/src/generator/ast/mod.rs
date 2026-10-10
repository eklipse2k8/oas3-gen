pub mod bon_attrs;
mod client;
pub mod constants;
mod derives;
pub(crate) mod documentation;
pub mod fields;
pub mod lints;
mod multipart;
mod outer_attrs;
mod parsed_path;
mod security;
pub(super) mod serde_attrs;
pub(crate) mod server;
mod status_codes;
pub mod tokens;
pub(super) mod types;
pub(super) mod validation_attrs;

#[cfg(test)]
mod tests;

use std::collections::BTreeSet;

pub use client::ClientRootNode;
pub use derives::{DeriveTrait, DerivesProvider, SerdeImpl};
pub use documentation::Documentation;
use http::Method;
use indexmap::IndexMap;
use itertools::{Either, Itertools};
pub use lints::GlobalLintsNode;
use mediatype::MediaType;
pub use multipart::{
  MultipartFieldInfo, MultipartPartKind, MultipartProperty, PartMembers, PartRfc6570, PartStyle, PartType, PartValue,
  is_json_media_type,
};
use oas3::{
  Spec,
  spec::{ObjectSchema, ParameterIn},
};
pub use outer_attrs::{OuterAttr, SerdeAsFieldAttr, SerdeAsSeparator};
pub use parsed_path::ParsedPath;
#[cfg(test)]
pub use parsed_path::{PathParseError, PathSegment};
pub use security::{ApiKeyLocation, CredentialKind, CredentialScheme, OperationSecurity};
pub use serde_attrs::SerdeAttribute;
use serde_json::Value;
pub use server::{HandlerBodyInfo, HandlerCredentials, ServerRequestTraitDef, ServerTraitMethod};
pub use status_codes::StatusCodeToken;
pub use tokens::{
  DefaultAtom, EnumToken, EnumVariantToken, FieldNameToken, MethodNameToken, StructToken, TraitToken, TypeAliasToken,
};
pub use types::{Rounding, RustPrimitive, TypeRef};
pub use validation_attrs::{RegexKey, ValidationAttribute};

pub use crate::generator::ast::fields::{DefaultVariant, FieldCollection, FieldDef};
use crate::generator::{
  ast::constants::HttpHeaderRef,
  converter::GenerationTarget,
  metrics::GenerationWarning,
  naming::{
    constants::{COOKIE_PARAMS_SUFFIX, HEADER_PARAMS_SUFFIX, PATH_PARAMS_SUFFIX, QUERY_PARAMS_SUFFIX},
    inference::NormalizedVariant,
  },
};

/// API metadata from the spec's `info` object.
///
/// Carried into every generated file's header and the crate manifest,
/// independent of whether the client or server target is generated.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ApiMetadata {
  pub title: String,
  pub version: String,
  pub description: Option<String>,
}

impl From<&Spec> for ApiMetadata {
  fn from(spec: &Spec) -> Self {
    Self {
      title: spec.info.title.clone(),
      version: spec.info.version.clone(),
      description: spec.info.description.clone(),
    }
  }
}

/// Node used to generate file header
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash, bon::Builder)]
pub struct FileHeaderNode {
  pub title: String,
  pub version: String,
  pub source_path: String,
  pub generator_version: String,
  pub description: Option<Documentation>,
  pub lints: GlobalLintsNode,
}

/// Discriminated enum variant mapping
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash, bon::Builder)]
pub struct DiscriminatedVariant {
  #[builder(default)]
  pub discriminator_values: Vec<String>,
  pub variant_name: EnumVariantToken,
  pub type_name: TypeRef,
  /// How the member travels, when the enum is a union of frame types.
  pub frame: Option<FrameMember>,
}

/// Serde mode controlling which serde traits a type derives/implements
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SerdeMode {
  #[default]
  Both,
  SerializeOnly,
  DeserializeOnly,
  None,
}

pub type EnumMethod = MethodNode<EnumMethodKind>;
pub type StructMethod = MethodNode<MethodKind>;

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
  /// Whether the enum is a union of `text/event-stream` frame types keyed by their `event`.
  #[builder(default)]
  pub sse_frame: bool,
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
  #[builder(default)]
  pub status_code: StatusCodeToken,
  #[builder(default)]
  pub media_types: Vec<ResponseMediaType>,
  pub schema_type: Option<TypeRef>,
  #[builder(default)]
  pub headers: Vec<FieldDef>,
  #[builder(default)]
  pub mapping: VariantMapping,
}

impl ResponseVariant {
  #[must_use]
  pub fn variant_name(&self) -> EnumVariantToken {
    self.status_code.to_variant_token()
  }

  #[must_use]
  pub fn payload(&self) -> ResponsePayload {
    ResponsePayload::for_status(self.status_code)
  }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub enum ResponsePayload {
  #[default]
  None,
  Value,
  Failure,
  /// A server's status and raw body.
  Raw,
  /// A client's unread `reqwest::Response`.
  Unread,
}

impl ResponsePayload {
  #[must_use]
  pub fn for_status(status_code: StatusCodeToken) -> Self {
    if status_code.is_success() {
      Self::Value
    } else {
      Self::Failure
    }
  }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, bon::Builder)]
pub struct VariantMapping {
  #[builder(default)]
  pub payload: ResponsePayload,
  pub union_variant: Option<EnumVariantToken>,
  /// Whether a text body parses with `FromStr` rather than decoding as JSON.
  #[builder(default)]
  pub parses_text: bool,
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

impl ResponseStatusCategory {
  /// The cases a status reads, event streams first.
  pub fn cases(&self) -> impl Iterator<Item = &ResponseVariantCategory> {
    match self {
      Self::Single(case) => Either::Left(std::iter::once(case)),
      Self::ContentDispatch { streams, variants } => Either::Right(streams.iter().chain(variants)),
    }
  }

  #[must_use]
  pub fn from_variants(variants: &[&ResponseVariant]) -> Self {
    if let [variant] = variants {
      let unique_categories = variant.media_types.iter().map(|m| m.category).unique().count();

      if unique_categories <= 1 {
        return Self::Single(
          ResponseVariantCategory::builder()
            .category(ResponseMediaType::primary_category(&variant.media_types))
            .variant((*variant).clone())
            .build(),
        );
      }
    }

    Self::from_content_types(variants)
  }

  #[must_use]
  pub(crate) fn from_content_types(variants: &[&ResponseVariant]) -> Self {
    let all_categories = variants
      .iter()
      .flat_map(|variant| {
        let default_category = variant
          .media_types
          .is_empty()
          .then(|| ResponseMediaType::primary_category(&[]));

        let explicit_categories = variant.media_types.iter().map(|m| m.category);

        default_category
          .into_iter()
          .chain(explicit_categories)
          .map(move |category| (category, *variant))
      })
      .unique_by(|(category, variant)| (*category, variant.schema_type.as_ref().map(TypeRef::to_rust_type)))
      .map(|(category, variant)| ResponseVariantCategory {
        category,
        variant: variant.clone(),
      })
      .collect_vec();

    let (streams, variants): (Vec<_>, Vec<_>) = all_categories
      .into_iter()
      .partition(|c| c.category == ContentCategory::EventStream);

    Self::ContentDispatch { streams, variants }
  }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatusHandler {
  pub status_code: StatusCodeToken,
  pub dispatch: ResponseStatusCategory,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, bon::Builder)]
pub struct ResponseEnumVariant {
  pub name: EnumVariantToken,
  #[builder(default)]
  pub status_code: StatusCodeToken,
  #[builder(default)]
  pub payload: ResponsePayload,
}

impl ResponseEnumVariant {
  /// Whether the variant holds the status beside its payload; an unread response holds its own.
  #[must_use]
  pub fn carries_status(&self) -> bool {
    self.status_code.carries_status() && self.payload != ResponsePayload::Unread
  }

  #[must_use]
  pub fn doc_line(&self) -> String {
    match self.payload {
      ResponsePayload::Raw => "Status code the operation does not declare; carries the raw body.".to_string(),
      ResponsePayload::Unread => {
        "Response the operation does not declare, by status or media type; carries it unread.".to_string()
      }
      _ => self.status_code.to_string(),
    }
  }
}

/// Response enum definition for operation responses
#[derive(Debug, Clone, Default, PartialEq, Eq, bon::Builder)]
pub struct ResponseEnumDef {
  pub name: EnumToken,
  #[builder(default)]
  pub docs: Documentation,
  #[builder(default)]
  pub variants: Vec<ResponseEnumVariant>,
  pub with_headers: Option<StructToken>,
}

impl ResponseEnumDef {
  #[must_use]
  pub fn has_param(&self, payload: ResponsePayload) -> bool {
    self.variants.iter().any(|v| v.payload == payload)
  }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, bon::Builder)]
pub struct ResponseHeadersRef {
  pub wrapper: StructToken,
  pub name: StructToken,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, bon::Builder)]
pub struct ResponseParam {
  pub body: TypeRef,
  pub headers: Option<ResponseHeadersRef>,
  /// Each body the operation declares, with the media type a server sends it as.
  #[builder(default)]
  pub media_types: Vec<BodyMediaType>,
}

/// One declared body of a payload class, with the response union member it is, and the
/// category and concrete `content_type` a server sends it as.
#[derive(Debug, Clone, PartialEq, Eq, bon::Builder)]
pub struct BodyMediaType {
  pub status: StatusCodeToken,
  pub member: Option<EnumVariantToken>,
  pub body: TypeRef,
  pub category: ContentCategory,
  pub content_type: String,
  pub stream: Option<EventStreamFormat>,
}

impl ResponseParam {
  /// The body declared for `status` as `member`, or for a status the operation doesn't
  /// declare, the first body declared as `member`.
  #[must_use]
  pub fn body_media(&self, status: StatusCodeToken, member: Option<&EnumVariantToken>) -> Option<&BodyMediaType> {
    let same_member = |entry: &&BodyMediaType| entry.member.as_ref() == member;
    self
      .media_types
      .iter()
      .filter(same_member)
      .find(|entry| entry.status == status)
      .or_else(|| self.media_types.iter().find(same_member))
  }
}

impl From<TypeRef> for ResponseParam {
  fn from(body: TypeRef) -> Self {
    Self {
      body,
      headers: None,
      media_types: vec![],
    }
  }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, bon::Builder)]
pub struct OperationResponse {
  pub name: EnumToken,
  #[builder(into)]
  pub value: Option<ResponseParam>,
  #[builder(into)]
  pub failure: Option<ResponseParam>,
}

impl OperationResponse {
  #[must_use]
  pub fn param(&self, payload: ResponsePayload) -> Option<&ResponseParam> {
    match payload {
      ResponsePayload::Value => self.value.as_ref(),
      ResponsePayload::Failure => self.failure.as_ref(),
      ResponsePayload::None | ResponsePayload::Raw | ResponsePayload::Unread => None,
    }
  }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, bon::Builder)]
pub struct ResponseUnionDef {
  pub name: EnumToken,
  #[builder(default)]
  pub docs: Documentation,
  #[builder(default)]
  pub variants: Vec<ResponseUnionVariant>,
  /// The target whose event stream a member holds, if any: a client's `EventStream` is
  /// `Debug`, a server's `BoxStream` isn't, and neither is `Clone`.
  pub streaming: Option<GenerationTarget>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, bon::Builder)]
pub struct ResponseUnionVariant {
  pub name: EnumVariantToken,
  pub rust_type: TypeRef,
}

/// Top-level Rust type representation
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RustType {
  Struct(StructDef),
  Enum(EnumDef),
  TypeAlias(TypeAliasDef),
  DiscriminatedEnum(DiscriminatedEnumDef),
  ResponseEnum(ResponseEnumDef),
  ResponseUnion(ResponseUnionDef),
}

impl RustType {
  pub fn type_name(&self) -> DefaultAtom {
    match self {
      RustType::Struct(def) => def.name.to_atom(),
      RustType::Enum(def) => def.name.to_atom(),
      RustType::TypeAlias(def) => def.name.to_atom(),
      RustType::DiscriminatedEnum(def) => def.name.to_atom(),
      RustType::ResponseEnum(def) => def.name.to_atom(),
      RustType::ResponseUnion(def) => def.name.to_atom(),
    }
  }

  pub fn type_priority(&self) -> u8 {
    match self {
      RustType::Struct(_) => 0,
      RustType::ResponseEnum(_) | RustType::ResponseUnion(_) => 1,
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
      RustType::ResponseUnion(def) => def.is_serializable(),
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
      RustType::ResponseUnion(def) => def.is_deserializable(),
      RustType::TypeAlias(_) => SerdeImpl::None,
    }
  }
}

/// Metadata about an API operation (for tracking, not direct code generation)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OperationKind {
  Http,
  Webhook,
  /// A request an operation's `callbacks` declare the API sends back to the caller
  Callback,
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
  pub response_variants: Option<Vec<ResponseVariant>>,
  pub response: Option<OperationResponse>,
  #[builder(default)]
  pub response_media_types: Vec<ResponseMediaType>,
  #[builder(default)]
  pub warnings: Vec<String>,
  #[builder(default)]
  pub parameters: Vec<FieldDef>,
  pub body: Option<OperationBody>,
  pub security: Option<OperationSecurity>,
  pub credentials_type: Option<StructToken>,
  #[builder(default)]
  pub documentation: Documentation,
  /// The document marks the operation `deprecated`, so its client method is too
  #[builder(default)]
  pub deprecated: bool,
}

impl OperationInfo {
  pub fn header_names(&self) -> impl Iterator<Item = HttpHeaderRef> {
    let request_headers = self
      .parameters
      .iter()
      .filter(|p| matches!(p.parameter_location, Some(ParameterLocation::Header)));
    let response_headers = self.response_variants.iter().flatten().flat_map(|v| &v.headers);
    let api_key_headers =
      self
        .security
        .iter()
        .flat_map(|security| &security.schemes)
        .filter_map(|scheme| match &scheme.kind {
          CredentialKind::ApiKey {
            location: ApiKeyLocation::Header,
            parameter_name,
          } => Some(parameter_name.as_str()),
          _ => None,
        });

    request_headers
      .chain(response_headers)
      .filter_map(|field| field.original_name.as_deref())
      .chain(api_key_headers)
      .map(HttpHeaderRef::from)
  }

  pub fn warnings(&self) -> impl Iterator<Item = GenerationWarning> {
    self
      .warnings
      .iter()
      .map(|warning| GenerationWarning::operation_specific(&self.operation_id, warning))
  }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ParameterLocation {
  #[default]
  Path,
  Query,
  Header,
  Cookie,
}

impl ParameterLocation {
  #[must_use]
  pub const fn suffix(self) -> &'static str {
    match self {
      Self::Path => PATH_PARAMS_SUFFIX,
      Self::Query => QUERY_PARAMS_SUFFIX,
      Self::Header => HEADER_PARAMS_SUFFIX,
      Self::Cookie => COOKIE_PARAMS_SUFFIX,
    }
  }
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
}

/// Media type information for a response variant.
///
/// Stores the parsed category (for determining parsing strategy) and the schema
/// type for this specific media type (since different content types can have
/// different schemas).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResponseMediaType {
  pub category: ContentCategory,
  /// The media type as the document declares it, such as `image/png`
  pub content_type: String,
  pub schema_type: Option<TypeRef>,
  /// How a `text/event-stream` streams its items; `None` for any other media type.
  pub stream: Option<EventStreamFormat>,
}

/// How a `text/event-stream` media type streams its items.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EventStreamFormat {
  /// The event data that ends the stream, from `x-sse-sentinel` or `x-speakeasy-sse-sentinel`.
  pub sentinel: Option<String>,
  pub item: StreamItem,
}

/// What one item of a `text/event-stream` is.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum StreamItem {
  /// The JSON `data` of one event.
  #[default]
  Payload,
  /// A frame type, read from and sent as one whole frame.
  Frame(FrameData),
  /// A union of frame types, told apart by their event names.
  FrameUnion,
}

/// How a frame type carries its `data`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FrameData {
  /// JSON of the `data` property's type.
  Json,
  /// The frame's text, as is, for a `string` property.
  Text,
}

impl FrameData {
  /// How a `data` property of type `data` travels.
  #[must_use]
  pub fn of(data: &TypeRef) -> Self {
    if data.is_string_like() { Self::Text } else { Self::Json }
  }
}

/// How one member of a union of frame types travels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FrameMember {
  pub data: FrameData,
  /// Whether the member has an `event` property. A member without one is sent under
  /// its discriminator value.
  pub declares_event: bool,
}

impl ResponseMediaType {
  #[must_use]
  pub fn new(content_type: &str) -> Self {
    Self::with_schema(content_type, None)
  }

  #[must_use]
  pub fn with_schema(content_type: &str, schema_type: Option<TypeRef>) -> Self {
    let category = ContentCategory::from_content_type(content_type);
    Self {
      category,
      content_type: content_type.to_string(),
      schema_type,
      stream: (category == ContentCategory::EventStream).then(EventStreamFormat::default),
    }
  }

  /// The `text/event-stream` media type among `media_types`.
  #[must_use]
  pub fn event_stream(media_types: &[Self]) -> Option<&Self> {
    media_types.iter().find(|m| m.category == ContentCategory::EventStream)
  }

  #[must_use]
  pub fn primary_category(media_types: &[Self]) -> ContentCategory {
    media_types.first().map_or(ContentCategory::Json, |m| m.category)
  }

  #[must_use]
  pub fn has_event_stream(media_types: &[Self]) -> bool {
    Self::event_stream(media_types).is_some()
  }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, bon::Builder)]
pub struct OperationBody {
  pub field_name: FieldNameToken,
  pub body_type: Option<TypeRef>,
  #[builder(default)]
  pub optional: bool,
  #[builder(default)]
  pub content_category: ContentCategory,
  /// The media type as the document declares it, such as `application/merge-patch+json`
  #[builder(default)]
  pub content_type: String,
  /// What the spec declares about each `multipart/form-data` property, keyed by property name
  #[builder(default)]
  pub multipart_properties: IndexMap<String, MultipartProperty>,
  /// Parts the body struct's fields become; `None` until resolved, or when the body has no struct
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
  /// Nested struct for cookie parameters (no serde: the client renders a `Cookie`
  /// header from it and the server parses one into it)
  CookieParams,
  /// Struct for the headers a response declares (no serde, just storage)
  ResponseHeaders,
  /// Struct for the API keys a server operation accepts (no serde, just storage)
  Credentials,
}

impl StructKind {
  #[must_use]
  pub fn is_header_struct(self) -> bool {
    matches!(self, Self::HeaderParams | Self::ResponseHeaders)
  }
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
  /// Additional traits to derive beyond the standard set (e.g., Builder), controlled by config options
  #[builder(default)]
  pub additional_derives: BTreeSet<DeriveTrait>,
  /// Whether the struct is a `text/event-stream` frame type, built from whole Server-Sent
  /// Events frames.
  #[builder(default)]
  pub sse_frame: bool,
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MethodKind {
  /// Method to parse a reqwest response into the operation's response enum instantiation
  ParseResponse {
    response: Box<OperationResponse>,
    status_handlers: Vec<StatusHandler>,
    default_handler: Option<ResponseStatusCategory>,
  },
  /// Method that wraps bon::Builder for constructing the struct
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
pub struct MethodNode<Kind> {
  pub name: MethodNameToken,
  pub docs: Documentation,
  pub kind: Kind,
}

impl MethodNode<EnumMethodKind> {
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
    wrapper_variant: EnumVariantToken,
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

/// Rust enum definition
#[derive(Debug, Clone, Default, PartialEq, Eq, bon::Builder)]
pub struct EnumDef {
  pub name: EnumToken,
  pub docs: Documentation,
  pub variants: Vec<VariantDef>,
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
  pub scalar_repr: Option<RustPrimitive>,
  #[builder(default)]
  pub default_mode: EnumDefault,
  /// Renders `#[non_exhaustive]`; the last variant is then the `#[serde(untagged)]` catch-all
  /// holding any string the spec doesn't list
  #[builder(default)]
  pub non_exhaustive: bool,
}

impl EnumDef {
  /// Index of the variant a derived `Default` constructs: the one flagged `default`, else the first
  #[must_use]
  pub fn derived_default_index(&self) -> usize {
    self.variants.iter().position(|v| v.default).unwrap_or(0)
  }

  #[must_use]
  pub fn fallback_variant(&self) -> Option<&VariantDef> {
    const FALLBACK_NAMES: &[&str] = &["Unknown", "Other"];
    self.variants.iter().find(|v| FALLBACK_NAMES.contains(&v.name.as_str()))
  }

  /// Catch-all variant of a non-exhaustive enum, e.g. `Other(String)`
  #[must_use]
  pub fn other_variant(&self) -> Option<&VariantDef> {
    self.variants.last().filter(|_| self.non_exhaustive)
  }

  /// Variants for the values the spec lists, without a non-exhaustive enum's catch-all
  #[must_use]
  pub fn listed_variants(&self) -> &[VariantDef] {
    match self.variants.split_last() {
      Some((_, listed)) if self.non_exhaustive => listed,
      _ => &self.variants,
    }
  }
}

/// How an enum's `Default` implementation is produced
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum EnumDefault {
  /// Derive `Default`; the variant flagged `default` (or the first) receives `#[default]`
  #[default]
  Derive,
  /// Hand-written `impl Default` constructing the schema's `default` value
  Value(EnumDefaultValue),
  /// No `Default` implementation (e.g. nullable unions without a `default`)
  None,
}

/// Payload for a hand-written enum `Default` impl
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnumDefaultValue {
  /// Raw schema `default`, coerced to a literal for primitive payloads
  pub value: serde_json::Value,
  /// Resolved variant of the wrapped value enum, when the default selects one
  pub inner_variant: Option<EnumVariantToken>,
}

/// JSON type of the listed value a value-enum variant stands for
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub enum ValueKind {
  #[default]
  String,
  Number,
  Bool,
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
  #[builder(default)]
  pub default: bool,
  /// JSON type of the value this unit variant stands for; `String` for payload variants
  #[builder(default)]
  pub value_kind: ValueKind,
}

impl VariantDef {
  /// Returns the name this variant serializes to: its `#[serde(rename)]` value when
  /// present, otherwise the Rust variant name. Both cases borrow from `self`.
  #[must_use]
  pub fn serde_name(&self) -> &str {
    self
      .serde_attrs
      .iter()
      .find_map(|attr| match attr {
        SerdeAttribute::Rename(val) => Some(val.as_str()),
        _ => None,
      })
      .unwrap_or_else(|| self.name.as_str())
  }

  pub fn add_alias(&mut self, value: impl Into<String>) {
    self.serde_attrs.push(SerdeAttribute::Alias(value.into()));
  }

  /// Returns every value this variant deserializes from: its serde name, then each
  /// `#[serde(alias)]` value.
  pub fn wire_values(&self) -> impl Iterator<Item = &str> {
    std::iter::once(self.serde_name()).chain(self.serde_attrs.iter().filter_map(|attr| match attr {
      SerdeAttribute::Alias(alias) => Some(alias.as_str()),
      _ => None,
    }))
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

impl<S: variant_def_builder::State> VariantDefBuilder<S>
where
  S::Name: variant_def_builder::IsSet,
  S::SerdeAttrs: variant_def_builder::IsSet,
  S::Content: variant_def_builder::IsSet,
  S::Docs: variant_def_builder::IsUnset,
  S::Deprecated: variant_def_builder::IsUnset,
{
  pub fn schema(
    self,
    schema: &ObjectSchema,
  ) -> VariantDefBuilder<variant_def_builder::SetDeprecated<variant_def_builder::SetDocs<S>>> {
    self
      .docs(Documentation::from_optional(schema.description.as_ref()))
      .deprecated(schema.deprecated.unwrap_or(false))
  }
}

type NormalizedVariantBuilder<S> = VariantDefBuilder<
  variant_def_builder::SetValueKind<variant_def_builder::SetSerdeAttrs<variant_def_builder::SetName<S>>>,
>;

impl<S: variant_def_builder::State> VariantDefBuilder<S>
where
  S::Name: variant_def_builder::IsUnset,
  S::SerdeAttrs: variant_def_builder::IsUnset,
  S::ValueKind: variant_def_builder::IsUnset,
{
  pub fn value(self, value: &Value) -> Option<NormalizedVariantBuilder<S>> {
    Some(self.normalized(NormalizedVariant::try_from(value).ok()?))
  }

  pub fn normalized(self, normalized: NormalizedVariant) -> NormalizedVariantBuilder<S> {
    let attributes = if normalized.name == normalized.rename_value {
      vec![]
    } else {
      vec![SerdeAttribute::Rename(normalized.rename_value)]
    };
    self
      .name(EnumVariantToken::from(normalized.name))
      .serde_attrs(attributes)
      .value_kind(normalized.kind)
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

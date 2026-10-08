use mediatype::MediaType;
use strum::EnumString;

use super::{FieldNameToken, TypeRef};

/// Returns `true` for JSON media types: a `json` subtype, such as `application/json`, or a
/// `+json` suffix, such as `application/problem+json`.
#[must_use]
pub fn is_json_media_type(content_type: &str) -> bool {
  MediaType::parse(content_type).is_ok_and(|media| {
    media.subty.as_str().eq_ignore_ascii_case("json")
      || media
        .suffix
        .is_some_and(|suffix| suffix.as_str().eq_ignore_ascii_case("json"))
  })
}

/// RFC 6570 style named by an Encoding Object's `style` field.
#[derive(Debug, Clone, Copy, PartialEq, Eq, EnumString)]
#[strum(serialize_all = "camelCase")]
pub enum PartStyle {
  /// `form`: comma-separated when not exploded
  Form,
  /// `spaceDelimited`: space-separated when not exploded
  SpaceDelimited,
  /// `pipeDelimited`: pipe-separated when not exploded
  PipeDelimited,
  /// `deepObject`: one `name[key]` part per object member
  DeepObject,
}

impl PartStyle {
  /// Separator between the items of a non-exploded array or object.
  #[must_use]
  pub const fn delimiter(self) -> char {
    match self {
      Self::Form | Self::DeepObject => ',',
      Self::SpaceDelimited => ' ',
      Self::PipeDelimited => '|',
    }
  }
}

/// RFC 6570 serialization an Encoding Object selects with an explicit `style`, `explode`, or
/// `allowReserved`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PartRfc6570 {
  /// Style, `form` unless the Encoding Object names another
  pub style: PartStyle,
  /// Whether arrays and objects spread across separate parts
  pub explode: bool,
}

/// What the spec declares about one `multipart/form-data` property.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MultipartProperty {
  /// Media type the Encoding Object's `contentType` resolves to
  pub content_type: Option<String>,
  /// RFC 6570 serialization the Encoding Object selects
  pub rfc6570: Option<PartRfc6570>,
  /// The schema, its non-null variant, or its `items` schema constrains nothing about its JSON
  /// shape, which OpenAPI 3.1 reads as raw binary
  pub raw_binary: bool,
}

/// How a non-file value converts to and from the text of a part.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PartValue {
  /// `String`: the text itself
  String,
  /// Enum whose `Display` writes its serialized name
  StringEnum,
  /// Number, boolean, or numeric enum: `Display` text, read back as JSON
  Scalar,
  /// Type serde writes as a string: dates, UUIDs, and base64 bytes
  SerdeString,
  /// Struct, discriminated union, or a value whose `contentType` is JSON: an `application/json` part
  Json,
  /// Type whose serialized shape depends on the value, such as untagged unions, maps, and
  /// `serde_json::Value`
  Dynamic,
}

/// A value's Rust type and how its part text converts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PartType {
  /// Type the value deserializes into, without the field's `Option` or collection wrappers
  pub rust_type: TypeRef,
  /// How the value converts to and from part text
  pub value: PartValue,
}

/// Members of an object property sent with an RFC 6570 style.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PartMembers {
  /// A struct's properties, by serialized name
  Named(Vec<(String, PartType)>),
  /// A map's entries, each holding the given type
  Map(PartType),
}

/// How a body struct field becomes `multipart/form-data` parts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MultipartPartKind {
  /// Raw bytes sent as file parts labelled `content_type`
  File { content_type: String },
  /// Content-type based encoding, with the Encoding Object's `contentType` when it names one
  Value {
    value: PartValue,
    content_type: Option<String>,
  },
  /// RFC 6570 encoding; `members` is set for struct and map properties
  Styled {
    value: PartValue,
    rfc6570: PartRfc6570,
    members: Option<PartMembers>,
  },
  /// Flattened `additionalProperties`, one part per entry, or one per item when `repeated`
  Flattened { entry: PartType, repeated: bool },
}

/// A body struct field and the parts it becomes.
#[derive(Debug, Clone, PartialEq, Eq, bon::Builder)]
pub struct MultipartFieldInfo {
  /// Rust field name
  pub name: FieldNameToken,
  /// Part name on the wire: the property name
  #[builder(into)]
  pub part_name: String,
  /// Field type, including its `Option` and collection wrappers
  pub rust_type: TypeRef,
  /// Parts the field becomes
  pub kind: MultipartPartKind,
  /// Type each value serializes through via `#[serde_as]`, if any
  pub serialize_as: Option<String>,
  /// The property is deprecated, so generated code allows `deprecated` where it reads the field
  #[builder(default)]
  pub deprecated: bool,
}

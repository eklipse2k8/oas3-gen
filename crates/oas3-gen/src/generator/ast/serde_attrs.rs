use strum::Display;

/// Represents a serde attribute applied to structs, fields, or enum variants.
///
/// These attributes control serialization and deserialization behavior in generated Rust code.
/// Each variant maps directly to a serde attribute that will be rendered in the output.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Display)]
pub enum SerdeAttribute {
  #[strum(serialize = "rename = \"{0}\"")]
  Rename(String),
  #[strum(serialize = "alias = \"{0}\"")]
  Alias(String),
  #[strum(serialize = "default")]
  Default,
  #[strum(serialize = "flatten")]
  Flatten,
  #[strum(serialize = "skip")]
  Skip,
  #[strum(serialize = "skip_deserializing")]
  SkipDeserializing,
  #[strum(serialize = "deny_unknown_fields")]
  DenyUnknownFields,
  #[strum(serialize = "untagged")]
  Untagged,
}

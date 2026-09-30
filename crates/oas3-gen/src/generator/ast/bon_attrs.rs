/// Represents a builder attribute applied to struct fields.
///
/// These attributes control the behavior of the `bon::Builder` pattern in generated Rust code.
/// Each variant maps directly to a builder attribute that will be rendered in the output.
///
/// `Default` and `Skip` render the owning field's default expression, the same one its
/// `#[default(...)]` attribute uses.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BuilderAttribute {
  Default,
  Rename(String),
  Skip,
}

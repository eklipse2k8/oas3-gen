use crate::generator::ast::TypeRef;

/// Represents a builder attribute applied to struct fields.
///
/// These attributes control the behavior of the `bon::Builder` pattern in generated Rust code.
/// Each variant maps directly to a builder attribute that will be rendered in the output.
///
/// `Default` and `Skip` carry the JSON value and type reference needed to produce the
/// Rust expression at code generation time (via `coercion::json_to_rust_literal`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BuilderAttribute {
  Default {
    value: serde_json::Value,
    type_ref: TypeRef,
  },
  Rename(String),
  Skip {
    value: serde_json::Value,
    type_ref: TypeRef,
  },
}

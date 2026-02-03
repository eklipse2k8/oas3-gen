use crate::generator::ast::RustType;

/// Wraps a conversion result with any inline types generated during conversion.
///
/// Used throughout the converter pipeline to track nested type definitions
/// that need to be emitted alongside the primary converted type.
pub(crate) struct ConversionOutput<T> {
  pub result: T,
  pub inline_types: Vec<RustType>,
}

impl<T> ConversionOutput<T> {
  /// Creates a conversion output containing only the primary result.
  ///
  /// Use this constructor when the conversion produces a single type without
  /// generating any additional inline type definitions.
  pub(crate) fn new(result: T) -> Self {
    Self {
      result,
      inline_types: vec![],
    }
  }

  /// Creates a conversion output containing the primary result along with
  /// additional inline type definitions discovered during conversion.
  ///
  /// Inline types are nested definitions (e.g., struct fields with anonymous
  /// object schemas, enum variants with inline schemas) that must be emitted
  /// as separate top-level Rust types alongside the primary converted type.
  pub(crate) fn with_inline_types(result: T, inline_types: Vec<RustType>) -> Self {
    Self { result, inline_types }
  }
}

impl ConversionOutput<RustType> {
  /// Consumes this output and returns all types as a flat vector.
  ///
  /// The inline types appear first, followed by the primary result. This
  /// ordering ensures that type definitions appear before types that
  /// depend on them, which is required for valid Rust code emission.
  pub(crate) fn into_vec(self) -> Vec<RustType> {
    let mut types = self.inline_types;
    types.push(self.result);
    types
  }
}

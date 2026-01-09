use std::collections::BTreeSet;

use oas3::spec::{ObjectOrReference, ObjectSchema};

use crate::generator::{ast::RustType, schema_registry::RefCollector};

/// Wraps a conversion result with any inline types generated during conversion.
///
/// Used throughout the converter pipeline to track nested type definitions
/// that need to be emitted alongside the primary converted type.
pub(crate) struct ConversionOutput<T> {
  pub result: T,
  pub inline_types: Vec<RustType>,
}

impl<T> ConversionOutput<T> {
  pub(crate) fn new(result: T) -> Self {
    Self {
      result,
      inline_types: vec![],
    }
  }

  pub(crate) fn with_inline_types(result: T, inline_types: Vec<RustType>) -> Self {
    Self { result, inline_types }
  }
}

impl ConversionOutput<RustType> {
  pub(crate) fn into_vec(self) -> Vec<RustType> {
    let mut types = self.inline_types;
    types.push(self.result);
    types
  }
}

pub(crate) fn extract_variant_references(variants: &[ObjectOrReference<ObjectSchema>]) -> BTreeSet<String> {
  variants.iter().filter_map(RefCollector::parse_schema_ref).collect()
}

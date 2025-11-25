use oas3::spec::{ObjectSchema, SchemaType, SchemaTypeSet};

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

/// Extension methods for `ObjectSchema` to query its type properties conveniently.
pub(crate) trait SchemaExt {
  /// Returns true if the schema represents a primitive type (no properties, allOf, etc.).
  fn is_primitive(&self) -> bool;
  /// Returns true if the schema is explicitly null.
  fn is_null(&self) -> bool;
  /// Returns true if the schema is a nullable object (e.g. `type: [object, null]`).
  fn is_nullable_object(&self) -> bool;
  /// Returns true if the schema is an array.
  fn is_array(&self) -> bool;
  /// Returns the single `SchemaType` if only one is defined.
  fn single_type(&self) -> Option<SchemaType>;
}

impl SchemaExt for ObjectSchema {
  fn is_primitive(&self) -> bool {
    self.properties.is_empty()
      && self.one_of.is_empty()
      && self.any_of.is_empty()
      && self.all_of.is_empty()
      && (self.schema_type.is_some() || self.enum_values.len() <= 1)
  }

  fn is_null(&self) -> bool {
    self.schema_type == Some(SchemaTypeSet::Single(SchemaType::Null))
  }

  fn is_nullable_object(&self) -> bool {
    if self.is_null() {
      return true;
    }
    if let Some(SchemaTypeSet::Multiple(types)) = &self.schema_type {
      types.contains(&SchemaType::Null)
        && types.contains(&SchemaType::Object)
        && self.properties.is_empty()
        && self.additional_properties.is_none()
    } else {
      false
    }
  }

  fn is_array(&self) -> bool {
    self.schema_type == Some(SchemaTypeSet::Single(SchemaType::Array))
  }

  fn single_type(&self) -> Option<SchemaType> {
    match &self.schema_type {
      Some(SchemaTypeSet::Single(t)) => Some(*t),
      _ => None,
    }
  }
}

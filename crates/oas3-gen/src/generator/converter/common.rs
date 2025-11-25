use oas3::spec::{ObjectOrReference, ObjectSchema, SchemaType, SchemaTypeSet};

use crate::generator::{
  ast::{RustType, TypeRef},
  converter::cache::SharedSchemaCache,
};

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

/// Helper to handle the common pattern of checking cache, generating an inline type, and registering it.
pub(crate) fn handle_inline_creation<F, P>(
  schema: &ObjectSchema,
  base_name: &str,
  forced_name: Option<String>,
  mut cache: Option<&mut SharedSchemaCache>,
  pre_check: P,
  generator: F,
) -> anyhow::Result<ConversionOutput<TypeRef>>
where
  F: FnOnce(&str, Option<&mut SharedSchemaCache>) -> anyhow::Result<ConversionOutput<RustType>>,
  P: FnOnce(&SharedSchemaCache) -> Option<String>,
{
  if let Some(cache) = &cache {
    if let Some(existing_name) = cache.get_type_name(schema)? {
      return Ok(ConversionOutput::new(TypeRef::new(existing_name)));
    }
    if let Some(name) = pre_check(cache) {
      return Ok(ConversionOutput::new(TypeRef::new(name)));
    }
  }

  let name = if let Some(forced) = forced_name {
    forced
  } else if let Some(cache) = &cache {
    cache.get_preferred_name(schema, base_name)?
  } else {
    base_name.to_string()
  };

  let result = match &mut cache {
    Some(cache) => generator(&name, Some(cache))?,
    None => generator(&name, None)?,
  };

  if let Some(cache) = cache {
    let type_name = cache.register_type(schema, &name, result.inline_types, result.result.clone())?;
    Ok(ConversionOutput::new(TypeRef::new(type_name)))
  } else {
    let mut all_types = vec![result.result];
    all_types.extend(result.inline_types);
    Ok(ConversionOutput::with_inline_types(TypeRef::new(name), all_types))
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
  /// Returns true if the schema should be treated as an inline struct.
  fn is_inline_struct(&self, prop_schema_ref: &ObjectOrReference<ObjectSchema>) -> bool;
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

  fn is_inline_struct(&self, prop_schema_ref: &ObjectOrReference<ObjectSchema>) -> bool {
    if matches!(prop_schema_ref, ObjectOrReference::Ref { .. }) {
      return false;
    }

    if !self.enum_values.is_empty() {
      return false;
    }

    if !self.one_of.is_empty() || !self.any_of.is_empty() {
      return false;
    }

    if self.is_array() {
      return false;
    }

    if self.is_primitive() {
      return false;
    }

    !self.properties.is_empty()
  }
}

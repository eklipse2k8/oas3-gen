use std::collections::BTreeSet;

use oas3::spec::{ObjectOrReference, ObjectSchema, Schema, SchemaType, SchemaTypeSet, Spec};

use crate::generator::{
  ast::{RustType, TypeRef},
  converter::cache::SharedSchemaCache,
  schema_registry::ReferenceExtractor,
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
///
/// This function orchestrates inline type creation by:
/// 1. Checking if the type already exists in cache (early return if found)
/// 2. Running a cached name check for special cases like enums (early return if found)
/// 3. Determining the appropriate name for the new type
/// 4. Calling the generator function to create the type
/// 5. Registering the new type in cache or collecting inline types
pub(crate) fn handle_inline_creation<F, C>(
  schema: &ObjectSchema,
  base_name: &str,
  forced_name: Option<String>,
  mut cache: Option<&mut SharedSchemaCache>,
  cached_name_check: C,
  generator: F,
) -> anyhow::Result<ConversionOutput<TypeRef>>
where
  F: FnOnce(&str, Option<&mut SharedSchemaCache>) -> anyhow::Result<ConversionOutput<RustType>>,
  C: FnOnce(&SharedSchemaCache) -> Option<String>,
{
  if let Some(cache) = &cache {
    if let Some(existing_name) = cache.get_type_name(schema)? {
      return Ok(ConversionOutput::new(TypeRef::new(existing_name)));
    }
    if let Some(name) = cached_name_check(cache) {
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

  let result = generator(&name, cache.as_deref_mut())?;

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
  /// Returns true if the schema represents a primitive type (no properties, oneOf, anyOf, allOf).
  fn is_primitive(&self) -> bool;

  /// Returns true if the schema is explicitly null type.
  fn is_null(&self) -> bool;

  /// Returns true if the schema is a nullable placeholder (pure null or empty object with null).
  /// This includes schemas like `{type: "null"}` and `{type: ["object", "null"]}` with no properties.
  fn is_nullable_object(&self) -> bool;

  /// Returns true if the schema is an array type.
  fn is_array(&self) -> bool;

  /// Returns true if the schema is a string type.
  fn is_string(&self) -> bool;

  /// Returns true if the schema is an object type.
  fn is_object(&self) -> bool;

  /// Returns true if the schema is a numeric type (integer or number).
  fn is_numeric(&self) -> bool;

  /// Returns true if the schema is a nullable array type `[array, null]`.
  fn is_nullable_array(&self) -> bool;

  /// Returns true if the schema has exactly the single specified type.
  fn is_single_type(&self, schema_type: SchemaType) -> bool;

  /// Returns the single `SchemaType` if exactly one is defined, None otherwise.
  fn single_type(&self) -> Option<SchemaType>;

  /// Returns the non-null type from a two-type nullable set (e.g., `[string, null]` -> `string`).
  fn non_null_type(&self) -> Option<SchemaType>;

  /// Returns true if the schema represents an inline object definition.
  /// This excludes enums, unions, arrays, and schemas without properties.
  fn is_inline_object(&self) -> bool;

  /// Returns true if the schema is a discriminated base type with a non-empty mapping.
  fn is_discriminated_base_type(&self) -> bool;

  /// Returns true if the schema has no type constraints (no properties, no type info).
  /// An empty schema `{}` or one with only `additionalProperties: {}` both return true,
  /// as neither constrains the shape of the data.
  fn is_empty_object(&self) -> bool;

  /// Returns true if the schema has inline oneOf or anyOf variants.
  fn has_inline_union(&self) -> bool;

  /// Returns true if this is an array with inline union items (oneOf/anyOf in items).
  fn has_inline_union_array_items(&self, spec: &Spec) -> bool;

  /// Extracts the inline array items schema if present and not a reference.
  /// Returns None if: no items, items is a boolean schema, or items is a $ref.
  fn inline_array_items<'a>(&'a self, spec: &'a Spec) -> Option<ObjectSchema>;

  /// Returns true if the schema has enum values defined.
  fn has_enum_values(&self) -> bool;

  /// Returns true if the schema has allOf composition.
  fn has_all_of(&self) -> bool;
}

impl SchemaExt for ObjectSchema {
  fn is_primitive(&self) -> bool {
    self.properties.is_empty()
      && self.one_of.is_empty()
      && self.any_of.is_empty()
      && self.all_of.is_empty()
      && self.enum_values.len() <= 1
      && (self.schema_type.is_some() || self.enum_values.is_empty())
  }

  fn is_null(&self) -> bool {
    self.is_single_type(SchemaType::Null)
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
    self.is_single_type(SchemaType::Array)
  }

  fn is_string(&self) -> bool {
    self.is_single_type(SchemaType::String)
  }

  fn is_object(&self) -> bool {
    self.is_single_type(SchemaType::Object)
  }

  fn is_numeric(&self) -> bool {
    matches!(
      &self.schema_type,
      Some(SchemaTypeSet::Single(SchemaType::Number | SchemaType::Integer))
    )
  }

  fn is_nullable_array(&self) -> bool {
    match &self.schema_type {
      Some(SchemaTypeSet::Multiple(types)) => {
        types.len() == 2 && types.contains(&SchemaType::Array) && types.contains(&SchemaType::Null)
      }
      _ => false,
    }
  }

  fn is_single_type(&self, schema_type: SchemaType) -> bool {
    matches!(
      &self.schema_type,
      Some(SchemaTypeSet::Single(t)) if *t == schema_type
    )
  }

  fn single_type(&self) -> Option<SchemaType> {
    match &self.schema_type {
      Some(SchemaTypeSet::Single(t)) => Some(*t),
      _ => None,
    }
  }

  fn non_null_type(&self) -> Option<SchemaType> {
    match &self.schema_type {
      Some(SchemaTypeSet::Multiple(types)) if types.len() == 2 && types.contains(&SchemaType::Null) => {
        types.iter().find(|t| **t != SchemaType::Null).copied()
      }
      _ => None,
    }
  }

  fn is_inline_object(&self) -> bool {
    if !self.enum_values.is_empty() {
      return false;
    }

    if !self.one_of.is_empty() || !self.any_of.is_empty() {
      return false;
    }

    if self.is_array() {
      return false;
    }

    let is_object_type = self.single_type() == Some(SchemaType::Object) || self.schema_type.is_none();
    is_object_type && !self.properties.is_empty()
  }

  fn is_discriminated_base_type(&self) -> bool {
    self
      .discriminator
      .as_ref()
      .and_then(|d| d.mapping.as_ref().map(|m| !m.is_empty()))
      .unwrap_or(false)
      && !self.properties.is_empty()
  }

  fn is_empty_object(&self) -> bool {
    self.properties.is_empty()
      && self.one_of.is_empty()
      && self.any_of.is_empty()
      && self.all_of.is_empty()
      && self.enum_values.is_empty()
      && self.schema_type.is_none()
  }

  fn has_inline_union(&self) -> bool {
    !self.one_of.is_empty() || !self.any_of.is_empty()
  }

  fn has_inline_union_array_items(&self, spec: &Spec) -> bool {
    if !self.is_array() {
      return false;
    }
    self
      .inline_array_items(spec)
      .is_some_and(|items| items.has_inline_union())
  }

  fn inline_array_items<'a>(&'a self, spec: &'a Spec) -> Option<ObjectSchema> {
    let items_box = self.items.as_ref()?;
    let items_schema_ref = match items_box.as_ref() {
      Schema::Object(o) => o,
      Schema::Boolean(_) => return None,
    };

    if matches!(&**items_schema_ref, ObjectOrReference::Ref { .. }) {
      return None;
    }

    items_schema_ref.resolve(spec).ok()
  }

  fn has_enum_values(&self) -> bool {
    !self.enum_values.is_empty()
  }

  fn has_all_of(&self) -> bool {
    !self.all_of.is_empty()
  }
}

pub(crate) fn extract_variant_references(variants: &[ObjectOrReference<ObjectSchema>]) -> BTreeSet<String> {
  variants
    .iter()
    .filter_map(ReferenceExtractor::extract_ref_name_from_obj_ref)
    .collect()
}

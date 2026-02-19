use std::{collections::BTreeSet, rc::Rc};

use anyhow::Result;
use inflections::Inflect;
use oas3::spec::ObjectSchema;

use super::{
  ConversionOutput, TypeResolver,
  structs::StructConverter,
  union_types::variants_to_cache_key,
  unions::{EnumConverter, UnionConverter},
};
use crate::{
  generator::{
    ast::{RustType, TypeRef},
    converter::{ConverterContext, SchemaConverter, cache::SharedSchemaCache},
    naming::identifiers::strip_parent_prefix,
  },
  utils::SchemaExt,
};

/// Resolves anonymous inline schemas into named Rust types with cache deduplication.
///
/// When a property contains an inline object, enum, or union definition (rather than
/// a `$ref`), this resolver creates a named type and registers it in the shared cache.
/// Subsequent encounters of structurally identical schemas reuse the cached name,
/// preventing duplicate type definitions in generated output.
#[derive(Debug, Clone)]
pub(crate) struct InlineTypeResolver {
  context: Rc<ConverterContext>,
}

impl InlineTypeResolver {
  /// Creates a new inline type resolver with access to the shared converter context.
  pub(crate) fn new(context: Rc<ConverterContext>) -> Self {
    Self { context }
  }

  /// Creates a named struct type from an inline object schema.
  ///
  /// Generates a type name by concatenating `parent_name` with `property_name`
  /// in PascalCase (e.g., `UserAddress` for a property `address` on `User`).
  /// If an identical schema already exists in the cache, returns a reference
  /// to the existing type instead of creating a duplicate.
  pub(crate) fn resolve_inline_struct(
    &self,
    parent_name: &str,
    property_name: &str,
    schema: &ObjectSchema,
  ) -> Result<ConversionOutput<TypeRef>> {
    let prop_pascal = property_name.to_pascal_case();
    let base_name = format!("{parent_name}{}", strip_parent_prefix(parent_name, &prop_pascal));

    self.resolve_with_cache(
      schema,
      &base_name,
      None,
      |_| None,
      |name| StructConverter::new(self.context.clone()).convert_struct(name, schema, None),
    )
  }

  /// Creates a named struct type from an inline schema using an explicit name.
  ///
  /// Unlike [`resolve_inline_struct`], the caller provides the full type name
  /// rather than deriving it from parent and property names. Useful for
  /// union variant structs where the name follows a different convention.
  pub(crate) fn resolve_inline_struct_with_name(
    &self,
    schema: &ObjectSchema,
    base_name: &str,
  ) -> Result<ConversionOutput<TypeRef>> {
    self.resolve_with_cache(
      schema,
      base_name,
      None,
      |_| None,
      |name| StructConverter::new(self.context.clone()).convert_struct(name, schema, None),
    )
  }

  /// Creates a named enum type from an inline string enum schema.
  ///
  /// Uses `enum_values` as a cache key to detect duplicate enum definitions
  /// across the specification. If an enum with identical values already exists,
  /// returns a reference to that type. Otherwise, generates a new enum with
  /// the name `{parent_name}{property_name}`.
  pub(crate) fn resolve_inline_enum(
    &self,
    parent_name: &str,
    property_name: &str,
    schema: &ObjectSchema,
    enum_values: &[String],
  ) -> Result<ConversionOutput<TypeRef>> {
    let base_name = format!("{parent_name}{}", property_name.to_pascal_case());
    let forced_name = self.context.cache().get_enum_name(enum_values);

    self.resolve_with_cache(
      schema,
      &base_name,
      forced_name,
      |cache| cache.get_generated_enum_name(enum_values),
      |name| {
        let converter = EnumConverter::new(self.context.clone());
        Ok(ConversionOutput::new(converter.convert_value_enum(name, schema)))
      },
    )
  }

  /// Creates a named union enum from an inline `oneOf` or `anyOf` schema.
  ///
  /// First checks if a union with the same set of `$ref` targets already
  /// exists in the cache. If so, returns a reference to that type. Otherwise,
  /// generates a new enum and registers it in both the schema cache and
  /// the union registry for future deduplication.
  pub(crate) fn resolve_inline_union(
    &self,
    schema: &ObjectSchema,
    refs: &BTreeSet<String>,
    base_name: &str,
  ) -> Result<ConversionOutput<TypeRef>> {
    if refs.len() > 1
      && let Some(name) = self.context.cache().find_union(refs).map(String::from)
    {
      return Ok(ConversionOutput::new(self.context.graph().type_ref(&name)));
    }

    let discriminator = schema.discriminator.as_ref().map(|d| d.property_name.as_str());

    let enum_cache_key = self
      .context
      .cache()
      .get_precomputed_enum_cache_key(schema)
      .ok()
      .flatten()
      .or_else(|| {
        if schema.is_relaxed_enum_pattern() {
          return None;
        }
        let entries = schema.extract_enum_entries(self.context.graph().spec());
        (!entries.is_empty()).then(|| variants_to_cache_key(&entries))
      });

    {
      let cache = self.context.cache();
      if refs.len() >= 2
        && let Some(name) = cache.get_union_name(refs, discriminator)
      {
        return Ok(ConversionOutput::new(TypeRef::new(name)));
      }
    }

    let result = self.resolve_with_cache(
      schema,
      base_name,
      None,
      |cache| {
        enum_cache_key
          .as_ref()
          .and_then(|key| cache.get_generated_enum_name(key))
      },
      |name| UnionConverter::new(self.context.clone()).convert_union(name, schema),
    )?;

    if refs.len() >= 2 {
      self.context.cache_mut().register_union(
        refs.clone(),
        schema.discriminator.as_ref().map(|d| d.property_name.clone()),
        result.result.base_type.to_string(),
      );
    }

    Ok(result)
  }

  /// Attempts to create a named type from an arbitrary inline schema.
  ///
  /// Inspects the schema to determine if it represents a non-trivial type
  /// (object, enum, or union) that warrants extraction. Returns `None` for
  /// primitive types or empty objects. For nullable unions with a single
  /// non-null variant, returns the inner type wrapped in `Option`.
  pub(crate) fn try_inline_schema(
    &self,
    schema: &ObjectSchema,
    base_name: &str,
  ) -> Result<Option<ConversionOutput<String>>> {
    let schema_converter = SchemaConverter::new(&self.context);
    let result = self.resolve_inline_schema_with_fn(schema, base_name, |name, effective| {
      schema_converter.convert_schema(name, effective)
    })?;

    if result.is_some() {
      return Ok(result);
    }

    let type_resolver = TypeResolver::new(self.context.clone());
    if schema.union_variants_with_kind().is_some()
      && let Some(t) = type_resolver.try_nullable_union(schema)?
    {
      return Ok(Some(ConversionOutput::new(t.to_rust_type())));
    }

    Ok(None)
  }

  /// Resolves an inline schema using a custom conversion function.
  ///
  /// Performs cache lookup first; if the schema hash matches an existing
  /// type, returns the cached name. Otherwise, calls `convert_fn` to
  /// generate the type definition(s) and registers them in the cache.
  /// The primary type name is extracted from the last element of the
  /// generated types vector.
  pub(crate) fn resolve_inline_schema_with_fn<F>(
    &self,
    schema: &ObjectSchema,
    base_name: &str,
    convert_fn: F,
  ) -> Result<Option<ConversionOutput<String>>>
  where
    F: FnOnce(&str, &ObjectSchema) -> Result<Vec<RustType>>,
  {
    if schema.is_empty_object() {
      return Ok(None);
    }

    if let Some(cached) = self.context.cache().get_type_name(schema)? {
      return Ok(Some(ConversionOutput::new(cached)));
    }

    let effective = if schema.all_of.is_empty() {
      schema.clone()
    } else {
      self.context.graph().merge_all_of(schema)
    };

    let unique_name = self.context.cache_mut().make_unique_name(base_name);
    let generated = convert_fn(&unique_name, &effective)?;

    if generated.is_empty() {
      return Ok(None);
    }

    let Some(main_type) = generated.last().cloned() else {
      return Ok(None);
    };

    let enum_cache_key = self
      .context
      .cache()
      .get_precomputed_enum_cache_key(schema)
      .ok()
      .flatten()
      .or_else(|| {
        let entries = schema.extract_enum_entries(self.context.graph().spec());
        (!entries.is_empty()).then(|| variants_to_cache_key(&entries))
      });
    let registration = self
      .context
      .cache()
      .prepare_registration(schema, &unique_name, enum_cache_key)?;
    let named_type = SharedSchemaCache::apply_name_to_type(main_type, &registration.assigned_name);
    let final_name = registration.assigned_name.clone();
    self
      .context
      .cache_mut()
      .commit_registration(registration, vec![], named_type);

    Ok(Some(ConversionOutput::with_inline_types(final_name, generated)))
  }

  /// Internal cache-aware resolution with pluggable generation logic.
  ///
  /// Checks schema hash cache, then `cached_name_check`, then generates
  /// the type using `generator`. The `forced_name` overrides automatic
  /// naming when provided (used for enum deduplication). Registers the
  /// generated type in the cache with its canonical hash.
  fn resolve_with_cache<F, C>(
    &self,
    schema: &ObjectSchema,
    base_name: &str,
    forced_name: Option<String>,
    cached_name_check: C,
    generator: F,
  ) -> Result<ConversionOutput<TypeRef>>
  where
    F: FnOnce(&str) -> Result<ConversionOutput<RustType>>,
    C: FnOnce(&SharedSchemaCache) -> Option<String>,
  {
    {
      let cache = self.context.cache();
      if let Some(existing_name) = cache.get_type_name(schema)? {
        return Ok(ConversionOutput::new(TypeRef::new(existing_name)));
      }
      if let Some(name) = cached_name_check(&cache) {
        return Ok(ConversionOutput::new(TypeRef::new(name)));
      }
    }

    let name = if let Some(forced) = forced_name {
      forced
    } else {
      self.context.cache().get_preferred_name(schema, base_name)?
    };

    let result = generator(&name)?;

    let enum_cache_key = self
      .context
      .cache()
      .get_precomputed_enum_cache_key(schema)
      .ok()
      .flatten()
      .or_else(|| {
        let entries = schema.extract_enum_entries(self.context.graph().spec());
        (!entries.is_empty()).then(|| variants_to_cache_key(&entries))
      });
    let registration = self
      .context
      .cache()
      .prepare_registration(schema, &name, enum_cache_key)?;
    let named_type = SharedSchemaCache::apply_name_to_type(result.result.clone(), &registration.assigned_name);
    let type_name = registration.assigned_name.clone();
    self
      .context
      .cache_mut()
      .commit_registration(registration, result.inline_types, named_type);

    Ok(ConversionOutput::new(TypeRef::new(type_name)))
  }
}

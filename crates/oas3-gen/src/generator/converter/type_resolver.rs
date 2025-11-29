use std::{collections::BTreeSet, sync::Arc};

use anyhow::{Context, Result};
use inflections::Inflect;
use oas3::spec::{ObjectOrReference, ObjectSchema, Schema, SchemaType, SchemaTypeSet};

use super::{
  CodegenConfig, ConversionOutput, SchemaExt,
  cache::SharedSchemaCache,
  enums::{EnumConverter, UnionKind},
};
use crate::generator::{
  ast::{EnumToken, RustPrimitive, RustType, TypeRef},
  converter::common::handle_inline_creation,
  naming::{
    identifiers::to_rust_type_name,
    inference::{CommonVariantName, extract_common_variant_prefix, extract_enum_values, is_relaxed_enum_pattern},
  },
  schema_registry::{ReferenceExtractor, SchemaRegistry},
};

/// Checks if a union schema has a cached enum name.
///
/// Used during inline type creation to find existing generated enums
/// that match the union's enum value pattern.
fn check_cached_enum_name_for_union(schema: &ObjectSchema, cache: &SharedSchemaCache) -> Option<String> {
  if !is_relaxed_enum_pattern(schema)
    && let Some(values) = extract_enum_values(schema)
  {
    return cache.get_enum_name(&values);
  }
  None
}

/// Checks if a union with the given variant refs has already been cached.
fn check_cached_union_by_refs(
  variants: &[ObjectOrReference<ObjectSchema>],
  schema: &ObjectSchema,
  cache: &SharedSchemaCache,
) -> Option<String> {
  let refs = extract_all_variant_refs(variants);
  if refs.len() >= 2 {
    let discriminator = schema.discriminator.as_ref().map(|d| d.property_name.as_str());
    return cache.get_union_name(&refs, discriminator);
  }
  None
}

/// Resolves OpenAPI schemas into Rust Type References (`TypeRef`).
///
/// Handles primitives, references, inlining enums, and union types.
#[derive(Clone)]
pub(crate) struct TypeResolver {
  graph: Arc<SchemaRegistry>,
  preserve_case_variants: bool,
  case_insensitive_enums: bool,
  pub(crate) no_helpers: bool,
}

impl TypeResolver {
  /// Creates a new `TypeResolver`.
  pub(crate) fn new(graph: &Arc<SchemaRegistry>, config: CodegenConfig) -> Self {
    Self {
      graph: graph.clone(),
      preserve_case_variants: config.preserve_case_variants,
      case_insensitive_enums: config.case_insensitive_enums,
      no_helpers: config.no_helpers,
    }
  }

  fn config(&self) -> CodegenConfig {
    CodegenConfig {
      preserve_case_variants: self.preserve_case_variants,
      case_insensitive_enums: self.case_insensitive_enums,
      no_helpers: self.no_helpers,
    }
  }

  fn make_type_ref(&self, schema_name: &str) -> TypeRef {
    let mut type_ref = TypeRef::new(to_rust_type_name(schema_name));
    if self.graph.is_cyclic(schema_name) {
      type_ref = type_ref.with_boxed();
    }
    type_ref
  }

  /// Resolves a schema to a `TypeRef`, potentially wrapping it in `Option` or `Vec`.
  pub(crate) fn schema_to_type_ref(&self, schema: &ObjectSchema) -> anyhow::Result<TypeRef> {
    if let Some(type_ref) = self.try_resolve_by_title(schema) {
      return Ok(type_ref);
    }

    if !schema.one_of.is_empty()
      && let Some(type_ref) = self.try_convert_union_to_type_ref(&schema.one_of)?
    {
      return Ok(type_ref);
    }
    if !schema.any_of.is_empty()
      && let Some(type_ref) = self.try_convert_union_to_type_ref(&schema.any_of)?
    {
      return Ok(type_ref);
    }

    if let Some(ref schema_type) = schema.schema_type {
      return match schema_type {
        SchemaTypeSet::Single(typ) => self.map_single_primitive_type(*typ, schema),
        SchemaTypeSet::Multiple(types) => self.convert_nullable_primitive(types, schema),
      };
    }

    Ok(TypeRef::new("serde_json::Value"))
  }

  /// Resolves a property type, handling inline enums/unions by generating them.
  pub(crate) fn resolve_property_type_with_inlines(
    &self,
    parent_name: &str,
    prop_name: &str,
    prop_schema: &ObjectSchema,
    prop_schema_ref: &ObjectOrReference<ObjectSchema>,
    cache: Option<&mut SharedSchemaCache>,
  ) -> anyhow::Result<ConversionOutput<TypeRef>> {
    if let ObjectOrReference::Ref { ref_path, .. } = prop_schema_ref {
      return self.resolve_reference(ref_path, prop_schema);
    }

    if !prop_schema.enum_values.is_empty() {
      return self.handle_inline_enum(parent_name, prop_name, prop_schema, cache);
    }

    let has_one_of = !prop_schema.one_of.is_empty();
    if has_one_of || !prop_schema.any_of.is_empty() {
      return self.convert_inline_union_type(parent_name, prop_name, prop_schema, has_one_of, cache);
    }

    if prop_schema.is_array()
      && let Some(result) = self.try_convert_array_with_union_items(prop_name, prop_schema, cache)?
    {
      return Ok(result);
    }

    Ok(ConversionOutput::new(self.schema_to_type_ref(prop_schema)?))
  }

  /// Resolves a schema reference to its corresponding Rust type.
  ///
  /// If the reference points to a primitive type, returns the primitive directly.
  /// Otherwise, returns the named type (with Box wrapper if cyclic).
  fn resolve_reference(
    &self,
    ref_path: &str,
    resolved_schema: &ObjectSchema,
  ) -> anyhow::Result<ConversionOutput<TypeRef>> {
    let ref_name = SchemaRegistry::extract_ref_name(ref_path)
      .ok_or_else(|| anyhow::anyhow!("Invalid reference path: {ref_path}"))?;

    if resolved_schema.is_primitive() {
      return Ok(ConversionOutput::new(self.schema_to_type_ref(resolved_schema)?));
    }

    Ok(ConversionOutput::new(self.make_type_ref(&ref_name)))
  }

  fn handle_inline_enum(
    &self,
    parent_name: &str,
    prop_name: &str,
    prop_schema: &ObjectSchema,
    cache: Option<&mut SharedSchemaCache>,
  ) -> anyhow::Result<ConversionOutput<TypeRef>> {
    if prop_schema.enum_values.len() == 1 {
      return Ok(ConversionOutput::new(self.schema_to_type_ref(prop_schema)?));
    }

    let enum_values: Vec<String> = prop_schema
      .enum_values
      .iter()
      .filter_map(|v| v.as_str().map(String::from))
      .collect::<BTreeSet<_>>()
      .into_iter()
      .collect();

    let base_name = format!("{parent_name}{}", prop_name.to_pascal_case());
    let mut forced_name = None;
    if let Some(ref c) = cache
      && let Some(n) = c.get_enum_name(&enum_values)
    {
      forced_name = Some(n);
    }

    super::common::handle_inline_creation(
      prop_schema,
      &base_name,
      forced_name,
      cache,
      |cache| {
        if let Some(name) = cache.get_enum_name(&enum_values)
          && cache.is_enum_generated(&enum_values)
        {
          return Some(name);
        }
        None
      },
      |name, _cache| {
        let converter = EnumConverter::new(&self.graph, self.clone(), self.config());
        let inline_enum = converter
          .convert_simple_enum(name, prop_schema, None)
          .expect("convert_simple_enum should return Some when cache is None");

        Ok(ConversionOutput::new(inline_enum))
      },
    )
  }

  pub(crate) fn convert_inline_union_type(
    &self,
    parent_name: &str,
    prop_name: &str,
    prop_schema: &ObjectSchema,
    uses_one_of: bool,
    mut cache: Option<&mut SharedSchemaCache>,
  ) -> anyhow::Result<ConversionOutput<TypeRef>> {
    let variants = if uses_one_of {
      &prop_schema.one_of
    } else {
      &prop_schema.any_of
    };

    if let Some(type_ref) = self.try_build_nullable_union(variants)? {
      return Ok(ConversionOutput::new(type_ref));
    }

    if let Some(result) =
      self.try_convert_nullable_array_with_union_items(prop_name, prop_schema, variants, &mut cache)?
    {
      return Ok(result);
    }

    if let Some(name) = self.find_matching_union_schema(variants) {
      return Ok(ConversionOutput::new(self.make_type_ref(&name)));
    }

    let variant_refs = extract_all_variant_refs(variants);
    let discriminator = prop_schema.discriminator.as_ref().map(|d| d.property_name.as_str());

    if let Some(ref c) = cache
      && variant_refs.len() >= 2
      && let Some(name) = c.get_union_name(&variant_refs, discriminator)
    {
      return Ok(ConversionOutput::new(self.make_type_ref(&name)));
    }

    let common_prefix = extract_common_variant_prefix(variants);
    let prop_pascal = prop_name.to_pascal_case();

    let base_name = if let Some(CommonVariantName { name, has_suffix }) = common_prefix {
      if has_suffix {
        format!("{name}Kind")
      } else {
        format!("{name}{prop_pascal}Kind")
      }
    } else {
      format!("{parent_name}{prop_pascal}")
    };

    let result = super::common::handle_inline_creation(
      prop_schema,
      &base_name,
      None,
      cache.as_deref_mut(),
      |cache| {
        check_cached_union_by_refs(variants, prop_schema, cache)
          .or_else(|| check_cached_enum_name_for_union(prop_schema, cache))
      },
      |name, cache| self.generate_union_type(name, prop_schema, uses_one_of, cache),
    )?;

    if let Some(ref mut c) = cache
      && variant_refs.len() >= 2
    {
      c.register_union(
        variant_refs,
        prop_schema.discriminator.as_ref().map(|d| d.property_name.clone()),
        result.result.base_type.to_string(),
      );
    }

    Ok(result)
  }

  fn try_convert_nullable_array_with_union_items(
    &self,
    prop_name: &str,
    prop_schema: &ObjectSchema,
    variants: &[ObjectOrReference<ObjectSchema>],
    cache: &mut Option<&mut SharedSchemaCache>,
  ) -> anyhow::Result<Option<ConversionOutput<TypeRef>>> {
    if variants.len() != 2 {
      return Ok(None);
    }

    let (non_null, has_null) = self.partition_nullable_variants(variants)?;
    if !has_null || non_null.is_none() {
      return Ok(None);
    }

    let non_null_variant = non_null.unwrap();
    if ReferenceExtractor::extract_ref_name_from_obj_ref(non_null_variant).is_some() {
      return Ok(None);
    }

    let resolved = non_null_variant
      .resolve(self.graph.spec())
      .context("Resolving non-null variant for array check")?;

    if !resolved.is_array() || !self.array_items_have_union(&resolved) {
      return Ok(None);
    }

    let result = self.try_convert_array_with_union_items(prop_name, &resolved, cache.as_deref_mut())?;
    if let Some(mut output) = result {
      output.result = output.result.with_option();
      let unique_items = prop_schema.unique_items.unwrap_or(false);
      if unique_items {
        output.result = output.result.with_unique_items(true);
      }
      return Ok(Some(output));
    }

    Ok(None)
  }

  /// Extracts the main type from a list of generated types.
  ///
  /// Searches for a type matching the target name. If not found,
  /// falls back to the last type in the list (which is typically the main enum).
  fn extract_main_type(mut types: Vec<RustType>, target_name: &EnumToken) -> Result<(RustType, Vec<RustType>)> {
    let pos = types
      .iter()
      .position(|t| match t {
        RustType::Enum(e) => e.name == *target_name,
        _ => false,
      })
      .or_else(|| (!types.is_empty()).then_some(types.len() - 1))
      .ok_or_else(|| anyhow::anyhow!("Failed to locate generated union type"))?;

    let main = types.remove(pos);
    Ok((main, types))
  }

  /// Generates a union enum type from a schema with oneOf/anyOf variants.
  ///
  /// Used by both inline union property conversion and array-with-union-items conversion.
  fn generate_union_type(
    &self,
    name: &str,
    schema: &ObjectSchema,
    uses_one_of: bool,
    cache: Option<&mut SharedSchemaCache>,
  ) -> anyhow::Result<ConversionOutput<RustType>> {
    let converter = EnumConverter::new(&self.graph, self.clone(), self.config());
    let kind = if uses_one_of {
      UnionKind::OneOf
    } else {
      UnionKind::AnyOf
    };

    let generated_types = converter.convert_union_enum(name, schema, kind, cache)?;

    let expected_name = EnumToken::from_raw(name);
    let (main_type, nested) = Self::extract_main_type(generated_types, &expected_name)?;
    Ok(ConversionOutput::with_inline_types(main_type, nested))
  }

  fn try_build_nullable_union(&self, variants: &[ObjectOrReference<ObjectSchema>]) -> anyhow::Result<Option<TypeRef>> {
    if variants.len() != 2 {
      return Ok(None);
    }

    let (non_null, has_null) = self.partition_nullable_variants(variants)?;

    if !has_null || non_null.is_none() {
      return Ok(None);
    }

    let non_null_variant = non_null.unwrap();

    if let Some(ref_name) = ReferenceExtractor::extract_ref_name_from_obj_ref(non_null_variant) {
      return Ok(Some(self.make_type_ref(&ref_name).with_option()));
    }

    let resolved = non_null_variant
      .resolve(self.graph.spec())
      .context("Resolving non-null union variant")?;

    if resolved.is_array() && self.array_items_have_union(&resolved) {
      return Ok(None);
    }

    Ok(Some(self.schema_to_type_ref(&resolved)?.with_option()))
  }

  fn array_items_have_union(&self, schema: &ObjectSchema) -> bool {
    schema
      .items
      .as_ref()
      .and_then(|b| match b.as_ref() {
        Schema::Object(o) => o.resolve(self.graph.spec()).ok(),
        Schema::Boolean(_) => None,
      })
      .is_some_and(|items| !items.one_of.is_empty() || !items.any_of.is_empty())
  }

  /// Tries to convert a union schema into a single `TypeRef` (e.g. `Option<T>`, `Vec<T>`).
  pub(crate) fn try_convert_union_to_type_ref(
    &self,
    variants: &[ObjectOrReference<ObjectSchema>],
  ) -> anyhow::Result<Option<TypeRef>> {
    if let Some(name) = self.find_matching_union_schema(variants) {
      return Ok(Some(self.make_type_ref(&name)));
    }

    if let Some(non_null_variant) = self.find_non_null_variant(variants) {
      if let Some(ref_name) = ReferenceExtractor::extract_ref_name_from_obj_ref(non_null_variant) {
        return Ok(Some(TypeRef::new(to_rust_type_name(&ref_name)).with_option()));
      }
      let resolved = non_null_variant
        .resolve(self.graph.spec())
        .context("Resolving non-null variant")?;
      return Ok(Some(self.schema_to_type_ref(&resolved)?.with_option()));
    }

    self.find_best_union_fallback(variants)
  }

  fn find_best_union_fallback(&self, variants: &[ObjectOrReference<ObjectSchema>]) -> anyhow::Result<Option<TypeRef>> {
    let ref_count = variants
      .iter()
      .filter(|v| ReferenceExtractor::extract_ref_name_from_obj_ref(v).is_some())
      .count();

    if ref_count >= 2 {
      return Ok(None);
    }

    let mut fallback_type: Option<TypeRef> = None;

    for variant_ref in variants {
      if let Some(ref_name) = ReferenceExtractor::extract_ref_name_from_obj_ref(variant_ref) {
        return Ok(Some(TypeRef::new(to_rust_type_name(&ref_name))));
      }

      let Ok(resolved) = variant_ref.resolve(self.graph.spec()) else {
        continue;
      };

      if resolved.is_null() {
        continue;
      }

      if resolved.is_array() {
        let item_type = self.convert_array_items(&resolved)?;
        let unique_items = resolved.unique_items.unwrap_or(false);
        return Ok(Some(
          TypeRef::new(item_type.to_rust_type())
            .with_vec()
            .with_unique_items(unique_items),
        ));
      }

      if resolved.single_type() == Some(SchemaType::String) && fallback_type.is_none() {
        fallback_type = Some(TypeRef::new(RustPrimitive::String));
        continue;
      }

      if resolved.one_of.len() == 1
        && let Some(ref_name) = ReferenceExtractor::extract_ref_name_from_obj_ref(&resolved.one_of[0])
      {
        return Ok(Some(TypeRef::new(to_rust_type_name(&ref_name))));
      }

      if let Some(ref variant_title) = resolved.title
        && self.graph.get_schema(variant_title).is_some()
      {
        return Ok(Some(TypeRef::new(to_rust_type_name(variant_title))));
      }
    }

    Ok(fallback_type)
  }

  fn map_single_primitive_type(&self, schema_type: SchemaType, schema: &ObjectSchema) -> anyhow::Result<TypeRef> {
    let primitive = match schema_type {
      SchemaType::String => schema
        .format
        .as_ref()
        .and_then(|f| RustPrimitive::from_format(f))
        .unwrap_or(RustPrimitive::String),
      SchemaType::Number => schema
        .format
        .as_ref()
        .and_then(|f| RustPrimitive::from_format(f))
        .unwrap_or(RustPrimitive::F64),
      SchemaType::Integer => schema
        .format
        .as_ref()
        .and_then(|f| RustPrimitive::from_format(f))
        .unwrap_or(RustPrimitive::I64),
      SchemaType::Boolean => RustPrimitive::Bool,
      SchemaType::Object => RustPrimitive::Value,
      SchemaType::Null => RustPrimitive::Unit,
      SchemaType::Array => {
        let item_type = self.convert_array_items(schema)?;
        let unique_items = schema.unique_items.unwrap_or(false);
        return Ok(
          TypeRef::new(item_type.to_rust_type())
            .with_vec()
            .with_unique_items(unique_items),
        );
      }
    };

    let mut type_ref = TypeRef::new(primitive);
    if schema_type == SchemaType::Null {
      type_ref = type_ref.with_option();
    }
    Ok(type_ref)
  }

  fn convert_nullable_primitive(&self, types: &[SchemaType], schema: &ObjectSchema) -> anyhow::Result<TypeRef> {
    if types.len() == 2
      && types.contains(&SchemaType::Null)
      && let Some(non_null_type) = types.iter().find(|t| **t != SchemaType::Null)
    {
      let type_ref = self.map_single_primitive_type(*non_null_type, schema)?;
      return Ok(type_ref.with_option());
    }
    Ok(TypeRef::new("serde_json::Value"))
  }

  fn convert_array_items(&self, schema: &ObjectSchema) -> anyhow::Result<TypeRef> {
    let Some(items_ref) = schema.items.as_ref().and_then(|b| match b.as_ref() {
      Schema::Object(o) => Some(o),
      Schema::Boolean(_) => None,
    }) else {
      return Ok(TypeRef::new(RustPrimitive::Value));
    };

    if let Some(ref_name) = ReferenceExtractor::extract_ref_name_from_obj_ref(items_ref) {
      return Ok(TypeRef::new(to_rust_type_name(&ref_name)));
    }

    let items_schema = items_ref.resolve(self.graph.spec()).context("Resolving array items")?;

    self.schema_to_type_ref(&items_schema)
  }

  /// Converts array properties whose items are inline `oneOf`/`anyOf` unions.
  ///
  /// When an array's items schema contains a union (e.g., `items: { oneOf: [...] }`),
  /// this generates a dedicated enum type for the union variants and returns
  /// `Vec<GeneratedEnumName>` instead of a generic `Vec<serde_json::Value>`.
  ///
  /// Returns `None` if the array items are not an inline union, allowing the caller
  /// to fall back to standard array handling.
  pub(crate) fn try_convert_array_with_union_items(
    &self,
    prop_name: &str,
    prop_schema: &ObjectSchema,
    mut cache: Option<&mut SharedSchemaCache>,
  ) -> anyhow::Result<Option<ConversionOutput<TypeRef>>> {
    let Some(items_ref) = prop_schema.items.as_ref().and_then(|b| match b.as_ref() {
      Schema::Object(o) => Some(o),
      Schema::Boolean(_) => None,
    }) else {
      return Ok(None);
    };

    if let ObjectOrReference::Ref { .. } = &**items_ref {
      return Ok(None);
    }

    let items_schema = items_ref.resolve(self.graph.spec()).context("Resolving array items")?;

    let has_one_of = !items_schema.one_of.is_empty();
    let has_any_of = !items_schema.any_of.is_empty();

    if !has_one_of && !has_any_of {
      return Ok(None);
    }

    let variants = if has_one_of {
      &items_schema.one_of
    } else {
      &items_schema.any_of
    };

    if let Some(name) = self.find_matching_union_schema(variants) {
      let unique_items = prop_schema.unique_items.unwrap_or(false);
      let type_ref = self.make_type_ref(&name).with_vec().with_unique_items(unique_items);
      return Ok(Some(ConversionOutput::new(type_ref)));
    }

    let variant_refs = extract_all_variant_refs(variants);
    let discriminator = items_schema.discriminator.as_ref().map(|d| d.property_name.as_str());

    if let Some(ref c) = cache
      && variant_refs.len() >= 2
      && let Some(name) = c.get_union_name(&variant_refs, discriminator)
    {
      let unique_items = prop_schema.unique_items.unwrap_or(false);
      let type_ref = self.make_type_ref(&name).with_vec().with_unique_items(unique_items);
      return Ok(Some(ConversionOutput::new(type_ref)));
    }

    let singular_name = cruet::to_singular(prop_name);
    let base_kind_name = format!("{}Kind", singular_name.to_pascal_case());

    let common_prefix = extract_common_variant_prefix(variants);

    let name_to_use = if let Some(CommonVariantName { name, has_suffix }) = common_prefix {
      if has_suffix {
        format!("{name}Kind")
      } else {
        format!("{name}{base_kind_name}")
      }
    } else if let Some(ref c) = cache
      && c.name_conflicts_with_different_schema(&base_kind_name, &items_schema)?
    {
      c.make_unique_name(&base_kind_name)
    } else {
      base_kind_name
    };

    let result = handle_inline_creation(
      &items_schema,
      &name_to_use,
      None,
      cache.as_deref_mut(),
      |cache| {
        check_cached_union_by_refs(variants, &items_schema, cache)
          .or_else(|| check_cached_enum_name_for_union(&items_schema, cache))
      },
      |name, cache| self.generate_union_type(name, &items_schema, has_one_of, cache),
    )?;

    if let Some(ref mut c) = cache
      && variant_refs.len() >= 2
    {
      c.register_union(
        variant_refs,
        items_schema.discriminator.as_ref().map(|d| d.property_name.clone()),
        result.result.base_type.to_string(),
      );
    }

    let unique_items = prop_schema.unique_items.unwrap_or(false);
    let vec_type_ref = result.result.with_vec().with_unique_items(unique_items);

    Ok(Some(ConversionOutput::with_inline_types(
      vec_type_ref,
      result.inline_types,
    )))
  }

  fn try_resolve_by_title(&self, schema: &ObjectSchema) -> Option<TypeRef> {
    let title = schema.title.as_ref()?;
    if schema.schema_type.is_some() {
      return None;
    }
    self.graph.get_schema(title)?;
    Some(self.make_type_ref(title))
  }

  fn find_matching_union_schema(&self, variants: &[ObjectOrReference<ObjectSchema>]) -> Option<String> {
    let variant_refs = extract_all_variant_refs(variants);
    if variant_refs.len() < 2 {
      return None;
    }
    self
      .graph
      .schema_names()
      .iter()
      .find(|&&name| {
        self.graph.get_schema(name).is_some_and(|s| {
          let one_of_match = !s.one_of.is_empty() && extract_all_variant_refs(&s.one_of) == variant_refs;
          let any_of_match = !s.any_of.is_empty() && extract_all_variant_refs(&s.any_of) == variant_refs;
          one_of_match || any_of_match
        })
      })
      .map(|&s| s.clone())
  }

  /// Partitions union variants into nullable and non-nullable types.
  ///
  /// Returns the first non-null variant found and whether any null variant exists.
  fn partition_nullable_variants<'b>(
    &self,
    variants: &'b [ObjectOrReference<ObjectSchema>],
  ) -> Result<(Option<&'b ObjectOrReference<ObjectSchema>>, bool)> {
    let mut non_null = None;
    let mut has_null = false;

    for variant in variants {
      let resolved = variant
        .resolve(self.graph.spec())
        .context("Resolving variant for null check")?;

      if resolved.is_nullable_object() {
        has_null = true;
      } else {
        non_null = Some(variant);
      }
    }
    Ok((non_null, has_null))
  }

  fn find_non_null_variant<'b>(
    &self,
    variants: &'b [ObjectOrReference<ObjectSchema>],
  ) -> Option<&'b ObjectOrReference<ObjectSchema>> {
    if variants.len() != 2 {
      return None;
    }
    let has_null = variants
      .iter()
      .any(|v| v.resolve(self.graph.spec()).ok().is_some_and(|s| s.is_null()));

    if !has_null {
      return None;
    }

    variants
      .iter()
      .find(|v| v.resolve(self.graph.spec()).ok().is_some_and(|s| !s.is_null()))
  }
}

fn extract_all_variant_refs(variants: &[ObjectOrReference<ObjectSchema>]) -> BTreeSet<String> {
  variants
    .iter()
    .filter_map(ReferenceExtractor::extract_ref_name_from_obj_ref)
    .collect()
}

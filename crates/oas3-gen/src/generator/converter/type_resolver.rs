use std::{
  collections::{BTreeMap, BTreeSet, HashMap},
  sync::Arc,
};

use anyhow::{Context, Result};
use derive_builder::Builder;
use inflections::Inflect;
use oas3::spec::{Discriminator, ObjectOrReference, ObjectSchema, Schema, SchemaType, SchemaTypeSet};

use super::{
  CodegenConfig, ConversionOutput, SchemaExt,
  cache::SharedSchemaCache,
  discriminator::DiscriminatorHandler,
  enums::{EnumConverter, UnionKind},
  structs::StructConverter,
};
use crate::generator::{
  ast::{EnumToken, RustPrimitive, RustType, TypeRef},
  converter::common::handle_inline_creation,
  naming::{
    identifiers::{strip_parent_prefix, to_rust_type_name},
    inference::{CommonVariantName, extract_common_variant_prefix, extract_enum_values, is_relaxed_enum_pattern},
  },
  schema_registry::{ReferenceExtractor, SchemaRegistry},
};

/// Resolves OpenAPI schemas into Rust Type References (`TypeRef`).
#[derive(Clone, Debug, Builder)]
#[builder(setter(into))]
pub(crate) struct TypeResolver {
  graph: Arc<SchemaRegistry>,
  #[builder(default)]
  reachable_schemas: Option<Arc<BTreeSet<String>>>,
  config: CodegenConfig,
}

impl TypeResolver {
  pub(crate) fn graph(&self) -> &Arc<SchemaRegistry> {
    &self.graph
  }

  fn create_type_reference(&self, schema_name: &str) -> TypeRef {
    let mut type_reference = TypeRef::new(to_rust_type_name(schema_name));
    if self.graph.is_cyclic(schema_name) {
      type_reference = type_reference.with_boxed();
    }
    type_reference
  }

  /// Resolves a schema to a `TypeRef`, potentially wrapping it in `Option` or `Vec`.
  pub(crate) fn resolve_type(&self, schema: &ObjectSchema) -> anyhow::Result<TypeRef> {
    if let Some(type_ref) = self.resolve_by_title(schema) {
      return Ok(type_ref);
    }

    if !schema.one_of.is_empty() {
      if let Some(type_ref) = self.resolve_union(&schema.one_of)? {
        return Ok(type_ref);
      }
    } else if !schema.any_of.is_empty()
      && let Some(type_ref) = self.resolve_union(&schema.any_of)?
    {
      return Ok(type_ref);
    }

    if let Some(typ) = schema.single_type() {
      return self.resolve_primitive(typ, schema);
    }
    if let Some(non_null) = schema.non_null_type() {
      return Ok(self.resolve_primitive(non_null, schema)?.with_option());
    }
    if let Some(ref const_value) = schema.const_value {
      return Ok(Self::infer_type_from_const(const_value));
    }

    Ok(TypeRef::new(RustPrimitive::Value))
  }

  fn infer_type_from_const(const_value: &serde_json::Value) -> TypeRef {
    match const_value {
      serde_json::Value::String(_) => TypeRef::new(RustPrimitive::String),
      serde_json::Value::Number(n) if n.is_i64() => TypeRef::new(RustPrimitive::I64),
      serde_json::Value::Number(_) => TypeRef::new(RustPrimitive::F64),
      serde_json::Value::Bool(_) => TypeRef::new(RustPrimitive::Bool),
      _ => TypeRef::new(RustPrimitive::Value),
    }
  }

  /// Resolves a property type, handling inline structs/enums/unions by generating them.
  pub(crate) fn resolve_property_type(
    &self,
    parent_type_name: &str,
    property_name: &str,
    property_schema: &ObjectSchema,
    property_schema_ref: &ObjectOrReference<ObjectSchema>,
    cache: Option<&mut SharedSchemaCache>,
  ) -> anyhow::Result<ConversionOutput<TypeRef>> {
    if let ObjectOrReference::Ref { ref_path, .. } = property_schema_ref {
      return self.resolve_reference(ref_path, property_schema);
    }

    if property_schema.all_of.len() == 1
      && let Some(type_ref) = self.resolve_union(&property_schema.all_of)?
    {
      return Ok(ConversionOutput::new(type_ref));
    }

    if property_schema.is_inline_object() {
      return self.resolve_inline_struct(parent_type_name, property_name, property_schema, cache);
    }

    if !property_schema.enum_values.is_empty() {
      return self.resolve_inline_enum(parent_type_name, property_name, property_schema, cache);
    }

    if property_schema.has_inline_union() {
      let has_one_of = !property_schema.one_of.is_empty();
      return self.resolve_inline_union_type(parent_type_name, property_name, property_schema, has_one_of, cache);
    }

    if property_schema.is_array()
      && let Some(result) =
        self.resolve_array_with_inline_items(parent_type_name, property_name, property_schema, cache)?
    {
      return Ok(result);
    }

    Ok(ConversionOutput::new(self.resolve_type(property_schema)?))
  }

  /// Resolves a schema reference to its corresponding Rust type.
  ///
  /// If the reference points to a primitive type, returns the primitive directly.
  /// Otherwise, returns the named type (with Box wrapper if cyclic).
  fn resolve_reference(
    &self,
    reference_path: &str,
    resolved_schema: &ObjectSchema,
  ) -> anyhow::Result<ConversionOutput<TypeRef>> {
    let reference_name = SchemaRegistry::extract_ref_name(reference_path)
      .ok_or_else(|| anyhow::anyhow!("Invalid reference path: {reference_path}"))?;

    let is_complex_array = resolved_schema.has_inline_union_array_items(self.graph.spec());

    if resolved_schema.is_primitive() && !is_complex_array {
      Ok(ConversionOutput::new(self.resolve_type(resolved_schema)?))
    } else {
      Ok(ConversionOutput::new(self.create_type_reference(&reference_name)))
    }
  }

  fn resolve_inline_enum(
    &self,
    parent_name: &str,
    property_name: &str,
    property_schema: &ObjectSchema,
    cache: Option<&mut SharedSchemaCache>,
  ) -> anyhow::Result<ConversionOutput<TypeRef>> {
    if property_schema.enum_values.len() == 1 {
      return Ok(ConversionOutput::new(self.resolve_type(property_schema)?));
    }

    let enum_values: Vec<String> = extract_enum_values(property_schema).unwrap_or_default();
    let base_name = format!("{parent_name}{}", property_name.to_pascal_case());

    let forced_name = cache.as_ref().and_then(|c| c.get_enum_name(&enum_values));

    handle_inline_creation(
      property_schema,
      &base_name,
      forced_name,
      cache,
      |cache| {
        if let Some(name) = cache.get_enum_name(&enum_values)
          && cache.is_enum_generated(&enum_values)
        {
          Some(name)
        } else {
          None
        }
      },
      |name, _| {
        let converter = EnumConverter::new(&self.graph, self.clone(), self.config);
        let inline_enum = converter
          .convert_value_enum(name, property_schema, None)
          .expect("convert_value_enum should return Some when cache is None");

        Ok(ConversionOutput::new(inline_enum))
      },
    )
  }

  fn resolve_inline_struct(
    &self,
    parent_name: &str,
    property_name: &str,
    property_schema: &ObjectSchema,
    cache: Option<&mut SharedSchemaCache>,
  ) -> anyhow::Result<ConversionOutput<TypeRef>> {
    let prop_pascal = property_name.to_pascal_case();
    let base_name = format!("{parent_name}{}", strip_parent_prefix(parent_name, &prop_pascal));
    self.resolve_inline_struct_schema(property_schema, &base_name, cache)
  }

  fn resolve_inline_struct_schema(
    &self,
    schema: &ObjectSchema,
    base_name: &str,
    cache: Option<&mut SharedSchemaCache>,
  ) -> anyhow::Result<ConversionOutput<TypeRef>> {
    handle_inline_creation(
      schema,
      base_name,
      None,
      cache,
      |_| None,
      |name, cache| {
        let converter = StructConverter::new(&self.graph, self.config, self.reachable_schemas.clone());
        converter.convert_struct(name, schema, None, cache)
      },
    )
  }

  /// Consolidates common logic for creating union types (oneOf/anyOf).
  ///
  /// Handles:
  /// 1. Checking for existing matching union schemas via O(1) fingerprint lookup
  /// 2. Checking the schema cache for identical unions
  /// 3. Generating a new union type if needed
  /// 4. Registering the new union in the cache
  fn create_union_type(
    &self,
    schema: &ObjectSchema,
    variants: &[ObjectOrReference<ObjectSchema>],
    base_name: &str,
    mut cache: Option<&mut SharedSchemaCache>,
  ) -> anyhow::Result<ConversionOutput<TypeRef>> {
    let variant_references = Self::extract_variant_references(variants);

    if let Some(name) = self.lookup_matching_union_schema(&variant_references) {
      return Ok(ConversionOutput::new(self.create_type_reference(&name)));
    }

    let discriminator = schema.discriminator.as_ref().map(|d| d.property_name.as_str());

    if let Some(ref c) = cache
      && variant_references.len() >= 2
      && let Some(name) = c.get_union_name(&variant_references, discriminator)
    {
      return Ok(ConversionOutput::new(self.create_type_reference(&name)));
    }

    let uses_one_of = !schema.one_of.is_empty();

    let result = handle_inline_creation(
      schema,
      base_name,
      None,
      cache.as_deref_mut(),
      |cache| Self::lookup_cached_enum_name(schema, cache),
      |name, cache| self.build_union_enum(name, schema, uses_one_of, cache),
    )?;

    if let Some(ref mut c) = cache
      && variant_references.len() >= 2
    {
      c.register_union(
        variant_references,
        schema.discriminator.as_ref().map(|d| d.property_name.clone()),
        result.result.base_type.to_string(),
      );
    }

    Ok(result)
  }

  pub(crate) fn resolve_inline_union_type(
    &self,
    parent_name: &str,
    property_name: &str,
    property_schema: &ObjectSchema,
    uses_one_of: bool,
    mut cache: Option<&mut SharedSchemaCache>,
  ) -> anyhow::Result<ConversionOutput<TypeRef>> {
    let variants = if uses_one_of {
      &property_schema.one_of
    } else {
      &property_schema.any_of
    };

    if let Some(type_reference) = self.resolve_nullable_union(variants)? {
      return Ok(ConversionOutput::new(type_reference));
    }

    if let Some(result) =
      self.resolve_array_with_inline_items(parent_name, property_name, property_schema, cache.as_deref_mut())?
    {
      return Ok(result);
    }

    let property_pascal = property_name.to_pascal_case();
    let suffix_part = format!("{property_pascal}Kind");
    let base_name =
      Self::get_common_union_name(variants, &suffix_part).unwrap_or_else(|| format!("{parent_name}{property_pascal}"));

    self.create_union_type(property_schema, variants, &base_name, cache)
  }

  fn get_common_union_name(variants: &[ObjectOrReference<ObjectSchema>], suffix_part: &str) -> Option<String> {
    let common_prefix = extract_common_variant_prefix(variants);

    if let Some(CommonVariantName { name, has_suffix }) = common_prefix {
      if has_suffix {
        Some(format!("{name}Kind"))
      } else {
        Some(format!("{name}{suffix_part}"))
      }
    } else {
      None
    }
  }

  pub(crate) fn resolve_array_with_inline_items(
    &self,
    parent_name: &str,
    property_name: &str,
    array_schema: &ObjectSchema,
    cache: Option<&mut SharedSchemaCache>,
  ) -> anyhow::Result<Option<ConversionOutput<TypeRef>>> {
    let Some(items_schema) = array_schema.inline_array_items(self.graph.spec()) else {
      return Ok(None);
    };

    let unique_items = array_schema.unique_items.unwrap_or(false);
    let singular_name = cruet::to_singular(property_name);
    let singular_pascal = singular_name.to_pascal_case();

    let result = if items_schema.is_inline_object() {
      let base_name = format!("{parent_name}{}", strip_parent_prefix(parent_name, &singular_pascal));
      self.resolve_inline_struct_schema(&items_schema, &base_name, cache)?
    } else if items_schema.has_inline_union() {
      let has_one_of = !items_schema.one_of.is_empty();
      let variants = if has_one_of {
        &items_schema.one_of
      } else {
        &items_schema.any_of
      };

      let base_kind_name = format!("{singular_pascal}Kind");
      let name_to_use =
        Self::get_common_union_name(variants, &base_kind_name).unwrap_or_else(|| base_kind_name.clone());

      let final_name = if let Some(ref c) = cache
        && c.name_conflicts_with_different_schema(&name_to_use, &items_schema)?
      {
        c.make_unique_name(&name_to_use)
      } else {
        name_to_use
      };

      self.create_union_type(&items_schema, variants, &final_name, cache)?
    } else {
      return Ok(None);
    };

    let mut type_reference = result.result;
    type_reference.boxed = false;
    let vec_type_reference = type_reference.with_vec().with_unique_items(unique_items);

    Ok(Some(ConversionOutput::with_inline_types(
      vec_type_reference,
      result.inline_types,
    )))
  }

  fn resolve_by_title(&self, schema: &ObjectSchema) -> Option<TypeRef> {
    let title = schema.title.as_ref()?;
    if schema.schema_type.is_some() {
      return None;
    }
    self.graph.get_schema(title)?;
    Some(self.create_type_reference(title))
  }

  /// Looks up a matching union schema by fingerprint in O(1) time.
  ///
  /// Uses the SchemaRegistry's precomputed union fingerprint index instead of
  /// iterating through all schemas (which would be O(n × m)).
  fn lookup_matching_union_schema(&self, variant_references: &BTreeSet<String>) -> Option<String> {
    if variant_references.len() < 2 {
      return None;
    }
    self.graph.lookup_union_by_fingerprint(variant_references).cloned()
  }

  /// Partitions union variants into nullable and non-nullable types.
  ///
  /// Returns the first non-null variant found and whether any null variant exists.
  fn partition_nullable_variants<'b>(
    &self,
    variants: &'b [ObjectOrReference<ObjectSchema>],
  ) -> Result<(Option<&'b ObjectOrReference<ObjectSchema>>, bool)> {
    let mut non_null_variant = None;
    let mut contains_null = false;

    for variant in variants {
      let resolved = variant
        .resolve(self.graph.spec())
        .context("Resolving variant for null check")?;

      if resolved.is_nullable_object() {
        contains_null = true;
      } else {
        non_null_variant = Some(variant);
      }
    }
    Ok((non_null_variant, contains_null))
  }

  /// Extracts the main type from a list of generated types.
  ///
  /// Searches for a type matching the target name. If not found,
  /// falls back to the last type in the list (which is typically the main enum).
  fn extract_primary_type(mut types: Vec<RustType>, target_name: &EnumToken) -> Result<(RustType, Vec<RustType>)> {
    let pos = types
      .iter()
      .position(|t| match t {
        RustType::Enum(e) => e.name == *target_name,
        _ => false,
      })
      .or_else(|| (!types.is_empty()).then_some(types.len() - 1))
      .ok_or_else(|| anyhow::anyhow!("Failed to locate generated union type"))?;

    Ok((types.remove(pos), types))
  }

  /// Generates a union enum type from a schema with oneOf/anyOf variants.
  ///
  /// Used by both inline union property conversion and array-with-union-items conversion.
  fn build_union_enum(
    &self,
    name: &str,
    schema: &ObjectSchema,
    uses_one_of: bool,
    cache: Option<&mut SharedSchemaCache>,
  ) -> anyhow::Result<ConversionOutput<RustType>> {
    let converter = EnumConverter::new(&self.graph, self.clone(), self.config);
    let kind = if uses_one_of {
      UnionKind::OneOf
    } else {
      UnionKind::AnyOf
    };

    let generated_types = converter.convert_union(name, schema, kind, cache)?;

    let expected_name = EnumToken::from_raw(name);
    let (main_type, nested) = Self::extract_primary_type(generated_types, &expected_name)?;
    Ok(ConversionOutput::with_inline_types(main_type, nested))
  }

  fn resolve_nullable_union(&self, variants: &[ObjectOrReference<ObjectSchema>]) -> anyhow::Result<Option<TypeRef>> {
    if variants.len() != 2 {
      return Ok(None);
    }

    let (non_null_variant, contains_null) = self.partition_nullable_variants(variants)?;

    if !contains_null {
      return Ok(None);
    }

    if let Some(non_null) = non_null_variant {
      if let Some(reference_name) = ReferenceExtractor::extract_ref_name_from_obj_ref(non_null) {
        return Ok(Some(self.create_type_reference(&reference_name).with_option()));
      }

      let resolved = non_null
        .resolve(self.graph.spec())
        .context("Resolving non-null union variant")?;

      if resolved.is_array() && self.contains_union_items(&resolved) {
        return Ok(None);
      }

      return Ok(Some(self.resolve_type(&resolved)?.with_option()));
    }

    Ok(None)
  }

  fn contains_union_items(&self, schema: &ObjectSchema) -> bool {
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
  pub(crate) fn resolve_union(&self, variants: &[ObjectOrReference<ObjectSchema>]) -> anyhow::Result<Option<TypeRef>> {
    let variant_references = Self::extract_variant_references(variants);

    if let Some(name) = self.lookup_matching_union_schema(&variant_references) {
      return Ok(Some(self.create_type_reference(&name)));
    }

    if let Some(nullable_type) = self.resolve_nullable_union(variants)? {
      return Ok(Some(nullable_type));
    }

    self.resolve_union_fallback(variants)
  }

  /// Resolves union variants that don't match simple patterns.
  ///
  /// Uses single-pass iteration with early exit for O(m) complexity.
  /// Returns None if multiple refs exist (triggering enum generation).
  fn resolve_union_fallback(&self, variants: &[ObjectOrReference<ObjectSchema>]) -> anyhow::Result<Option<TypeRef>> {
    let mut reference_count = 0;
    let mut first_reference_name: Option<String> = None;

    for variant in variants {
      if let Some(reference_name) = ReferenceExtractor::extract_ref_name_from_obj_ref(variant) {
        reference_count += 1;
        if reference_count >= 2 {
          return Ok(None);
        }
        first_reference_name = Some(reference_name);
        continue;
      }

      let Ok(resolved) = variant.resolve(self.graph.spec()) else {
        continue;
      };

      if resolved.is_null() {
        continue;
      }

      if resolved.is_array() {
        let item_type = self.resolve_array_items(&resolved)?;
        let unique_items = resolved.unique_items.unwrap_or(false);
        return Ok(Some(
          TypeRef::new(item_type.to_rust_type())
            .with_vec()
            .with_unique_items(unique_items),
        ));
      }

      if resolved.single_type() == Some(SchemaType::String) {
        return Ok(Some(TypeRef::new(RustPrimitive::String)));
      }

      if resolved.one_of.len() == 1
        && let Some(reference_name) = ReferenceExtractor::extract_ref_name_from_obj_ref(&resolved.one_of[0])
      {
        return Ok(Some(self.create_type_reference(&reference_name)));
      }

      if let Some(ref variant_title) = resolved.title
        && self.graph.get_schema(variant_title).is_some()
      {
        return Ok(Some(self.create_type_reference(variant_title)));
      }
    }

    if let Some(reference_name) = first_reference_name {
      return Ok(Some(self.create_type_reference(&reference_name)));
    }

    Ok(None)
  }

  fn resolve_primitive(&self, schema_type: SchemaType, schema: &ObjectSchema) -> anyhow::Result<TypeRef> {
    let primitive = match schema_type {
      SchemaType::String | SchemaType::Number | SchemaType::Integer => {
        let default = match schema_type {
          SchemaType::String => RustPrimitive::String,
          SchemaType::Number => RustPrimitive::F64,
          SchemaType::Integer => RustPrimitive::I64,
          _ => unreachable!(),
        };
        schema
          .format
          .as_ref()
          .and_then(|f| RustPrimitive::from_format(f))
          .unwrap_or(default)
      }
      SchemaType::Boolean => RustPrimitive::Bool,
      SchemaType::Object => {
        if let Some(map_type) = self.try_resolve_map_type(schema)? {
          return Ok(map_type);
        }
        RustPrimitive::Value
      }
      SchemaType::Null => RustPrimitive::Unit,
      SchemaType::Array => {
        let item_type = self.resolve_array_items(schema)?;
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

  fn try_resolve_map_type(&self, schema: &ObjectSchema) -> anyhow::Result<Option<TypeRef>> {
    let Some(ref additional) = schema.additional_properties else {
      return Ok(None);
    };

    if matches!(additional, Schema::Boolean(b) if !b.0) {
      return Ok(None);
    }

    if !schema.properties.is_empty() {
      return Ok(None);
    }

    let value_type = self.resolve_additional_properties_type(additional)?;
    let map_type = TypeRef::new(format!(
      "std::collections::HashMap<String, {}>",
      value_type.to_rust_type()
    ));
    Ok(Some(map_type))
  }

  pub(crate) fn resolve_additional_properties_type(&self, additional: &Schema) -> anyhow::Result<TypeRef> {
    match additional {
      Schema::Boolean(_) => Ok(TypeRef::new(RustPrimitive::Value)),
      Schema::Object(schema_ref) => {
        if let ObjectOrReference::Ref { ref_path, .. } = &**schema_ref {
          let reference_name = SchemaRegistry::extract_ref_name(ref_path)
            .ok_or_else(|| anyhow::anyhow!("Invalid reference path: {ref_path}"))?;
          return Ok(self.create_type_reference(&reference_name));
        }

        let additional_schema = schema_ref
          .resolve(self.graph.spec())
          .context("Schema resolution failed for additionalProperties")?;

        if additional_schema.is_empty_object() {
          return Ok(TypeRef::new(RustPrimitive::Value));
        }

        self.resolve_type(&additional_schema)
      }
    }
  }

  fn resolve_array_items(&self, schema: &ObjectSchema) -> anyhow::Result<TypeRef> {
    let Some(items_reference) = schema.items.as_ref().and_then(|b| match b.as_ref() {
      Schema::Object(o) => Some(o),
      Schema::Boolean(_) => None,
    }) else {
      return Ok(TypeRef::new(RustPrimitive::Value));
    };

    if let Some(reference_name) = ReferenceExtractor::extract_ref_name_from_obj_ref(items_reference) {
      let mut type_reference = self.create_type_reference(&reference_name);
      type_reference.boxed = false;
      return Ok(type_reference);
    }

    let items_schema = items_reference
      .resolve(self.graph.spec())
      .context("Resolving array items")?;

    let mut type_reference = self.resolve_type(&items_schema)?;
    type_reference.boxed = false;
    Ok(type_reference)
  }

  fn lookup_cached_enum_name(schema: &ObjectSchema, cache: &SharedSchemaCache) -> Option<String> {
    if !is_relaxed_enum_pattern(schema)
      && let Some(values) = extract_enum_values(schema)
    {
      return cache.get_enum_name(&values);
    }
    None
  }

  fn extract_variant_references(variants: &[ObjectOrReference<ObjectSchema>]) -> BTreeSet<String> {
    variants
      .iter()
      .filter_map(ReferenceExtractor::extract_ref_name_from_obj_ref)
      .collect()
  }

  pub(crate) fn merge_child_schema_with_parent(
    &self,
    child_schema: &ObjectSchema,
    parent_schema: &ObjectSchema,
  ) -> anyhow::Result<ObjectSchema> {
    let mut merged_properties = BTreeMap::new();
    let mut merged_required = BTreeSet::new();
    let mut merged_discriminator = parent_schema.discriminator.clone();
    let mut merged_schema_type = parent_schema.schema_type.clone();

    self.collect_all_of_properties(
      child_schema,
      &mut merged_properties,
      &mut merged_required,
      &mut merged_discriminator,
      &mut merged_schema_type,
    )?;

    let mut merged_schema = child_schema.clone();
    merged_schema.properties = merged_properties;
    merged_schema.required = merged_required.into_iter().collect();
    merged_schema.discriminator = merged_discriminator;
    merged_schema.schema_type = merged_schema_type;
    merged_schema.all_of.clear();

    if merged_schema.additional_properties.is_none() {
      merged_schema
        .additional_properties
        .clone_from(&parent_schema.additional_properties);
    }

    Ok(merged_schema)
  }

  pub(crate) fn merge_all_of_schema(&self, schema: &ObjectSchema) -> anyhow::Result<ObjectSchema> {
    let mut merged_properties = BTreeMap::new();
    let mut merged_required = BTreeSet::new();
    let mut merged_discriminator = None;
    let mut merged_schema_type = None;

    self.collect_all_of_properties(
      schema,
      &mut merged_properties,
      &mut merged_required,
      &mut merged_discriminator,
      &mut merged_schema_type,
    )?;

    let mut merged_schema = schema.clone();
    merged_schema.properties = merged_properties;
    merged_schema.required = merged_required.into_iter().collect();
    merged_schema.discriminator = merged_discriminator;
    if merged_schema_type.is_some() {
      merged_schema.schema_type = merged_schema_type;
    }
    merged_schema.all_of.clear();

    Ok(merged_schema)
  }

  fn collect_all_of_properties(
    &self,
    schema: &ObjectSchema,
    properties: &mut BTreeMap<String, ObjectOrReference<ObjectSchema>>,
    required: &mut BTreeSet<String>,
    discriminator: &mut Option<Discriminator>,
    schema_type: &mut Option<SchemaTypeSet>,
  ) -> anyhow::Result<()> {
    for all_of_ref in &schema.all_of {
      let all_of_schema = all_of_ref
        .resolve(self.graph.spec())
        .with_context(|| "Schema resolution failed for allOf item")?;
      self.collect_all_of_properties(&all_of_schema, properties, required, discriminator, schema_type)?;
    }

    for (prop_name, prop_ref) in &schema.properties {
      properties.insert(prop_name.clone(), prop_ref.clone());
    }
    required.extend(schema.required.iter().cloned());

    if schema.discriminator.is_some() {
      discriminator.clone_from(&schema.discriminator);
    }

    if schema.schema_type.is_some() {
      schema_type.clone_from(&schema.schema_type);
    }
    Ok(())
  }

  pub(crate) fn get_merged_schema(
    &self,
    schema_name: &str,
    schema: &ObjectSchema,
    merged_schema_cache: &mut HashMap<String, ObjectSchema>,
  ) -> anyhow::Result<ObjectSchema> {
    if let Some(cached) = merged_schema_cache.get(schema_name) {
      return Ok(cached.clone());
    }

    let merged = self.merge_all_of_schema(schema)?;
    merged_schema_cache.insert(schema_name.to_string(), merged.clone());
    Ok(merged)
  }

  pub(crate) fn detect_discriminated_parent(
    &self,
    schema: &ObjectSchema,
    merged_schema_cache: &mut HashMap<String, ObjectSchema>,
  ) -> Option<ObjectSchema> {
    let handler = DiscriminatorHandler::new(&self.graph, self.reachable_schemas.as_ref());
    handler.detect_discriminated_parent(schema, merged_schema_cache, |name, s, cache| {
      self.get_merged_schema(name, s, cache)
    })
  }

  pub(crate) fn create_discriminated_enum(
    &self,
    base_name: &str,
    schema: &ObjectSchema,
    base_struct_name: &str,
  ) -> anyhow::Result<RustType> {
    let handler = DiscriminatorHandler::new(&self.graph, self.reachable_schemas.as_ref());
    handler.create_discriminated_enum(base_name, schema, base_struct_name)
  }
}

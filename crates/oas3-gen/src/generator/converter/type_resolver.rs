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
  ast::{RustPrimitive, RustType, TypeRef},
  naming::{
    identifiers::to_rust_type_name,
    inference::{extract_enum_values, is_relaxed_enum_pattern},
  },
  schema_graph::SchemaGraph,
};

/// Resolves OpenAPI schemas into Rust Type References (`TypeRef`).
///
/// Handles primitives, references, inlining enums, and union types.
#[derive(Clone)]
pub(crate) struct TypeResolver {
  graph: Arc<SchemaGraph>,
  preserve_case_variants: bool,
  case_insensitive_enums: bool,
  pub(crate) no_helpers: bool,
}

impl TypeResolver {
  /// Creates a new `TypeResolver`.
  pub(crate) fn new(graph: &Arc<SchemaGraph>, config: CodegenConfig) -> Self {
    Self {
      graph: graph.clone(),
      preserve_case_variants: config.preserve_case_variants,
      case_insensitive_enums: config.case_insensitive_enums,
      no_helpers: config.no_helpers,
    }
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
    let ref_name =
      SchemaGraph::extract_ref_name(ref_path).ok_or_else(|| anyhow::anyhow!("Invalid reference path: {ref_path}"))?;

    if resolved_schema.is_primitive() {
      return Ok(ConversionOutput::new(self.schema_to_type_ref(resolved_schema)?));
    }

    let mut type_ref = TypeRef::new(to_rust_type_name(&ref_name));
    if self.graph.is_cyclic(&ref_name) {
      type_ref = type_ref.with_boxed();
    }

    Ok(ConversionOutput::new(type_ref))
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

    if let Some(ref cache) = cache {
      if let Some(existing_name) = cache.get_type_name(prop_schema)? {
        return Ok(ConversionOutput::new(TypeRef::new(existing_name)));
      }
      if let Some(name) = cache.get_enum_name(&enum_values)
        && cache.is_enum_generated(&enum_values)
      {
        return Ok(ConversionOutput::new(TypeRef::new(name)));
      }
    }

    let base_name = format!("{parent_name}{}", prop_name.to_pascal_case());
    let enum_name = Self::determine_enum_name(prop_schema, &base_name, &enum_values, &cache)?;

    let config = CodegenConfig {
      preserve_case_variants: self.preserve_case_variants,
      case_insensitive_enums: self.case_insensitive_enums,
      no_helpers: self.no_helpers,
    };
    let converter = EnumConverter::new(&self.graph, self.clone(), config);
    let inline_enum = converter
      .convert_simple_enum(&enum_name, prop_schema, None)
      .expect("convert_simple_enum should return Some when cache is None");

    if let Some(c) = cache {
      let type_name = c.register_type(prop_schema, &enum_name, vec![], inline_enum.clone())?;
      c.register_enum(enum_values, type_name.clone());
      Ok(ConversionOutput::new(TypeRef::new(type_name)))
    } else {
      Ok(ConversionOutput::with_inline_types(
        TypeRef::new(RustPrimitive::Custom(enum_name)),
        vec![inline_enum],
      ))
    }
  }

  /// Determines the final name for a generated enum.
  ///
  /// Checks the cache for existing enum names matching the values,
  /// falls back to schema-based preferred names, or generates a unique name.
  fn determine_enum_name(
    schema: &ObjectSchema,
    base_name: &str,
    values: &[String],
    cache: &Option<&mut SharedSchemaCache>,
  ) -> Result<String> {
    if let Some(c) = cache {
      if let Some(name) = c.get_enum_name(values) {
        return Ok(name);
      }
      c.get_preferred_name(schema, base_name)
        .or_else(|_| Ok(c.make_unique_name(base_name)))
    } else {
      Ok(base_name.to_string())
    }
  }

  fn convert_inline_union_type(
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

    if let Some(ref cache) = cache {
      if let Some(existing_name) = cache.get_type_name(prop_schema)? {
        return Ok(ConversionOutput::new(TypeRef::new(existing_name)));
      }

      if !is_relaxed_enum_pattern(prop_schema)
        && let Some(values) = extract_enum_values(prop_schema)
        && let Some(name) = cache.get_enum_name(&values)
      {
        return Ok(ConversionOutput::new(TypeRef::new(name)));
      }
    }

    if let Some(name) = self.find_matching_union_schema(variants) {
      let mut type_ref = TypeRef::new(to_rust_type_name(&name));
      if self.graph.is_cyclic(&name) {
        type_ref = type_ref.with_boxed();
      }
      return Ok(ConversionOutput::new(type_ref));
    }

    let base_name = format!("{parent_name}{}", prop_name.to_pascal_case());
    let enum_name = if let Some(ref mut c) = cache {
      c.get_preferred_name(prop_schema, &base_name)?
    } else {
      base_name
    };

    let config = CodegenConfig {
      preserve_case_variants: self.preserve_case_variants,
      case_insensitive_enums: self.case_insensitive_enums,
      no_helpers: self.no_helpers,
    };
    let converter = EnumConverter::new(&self.graph, self.clone(), config);
    let kind = if uses_one_of {
      UnionKind::OneOf
    } else {
      UnionKind::AnyOf
    };

    let generated_types = converter.convert_union_enum(&enum_name, prop_schema, kind, cache.as_deref_mut())?;

    let main_type_name = generated_types
      .iter()
      .find_map(|t| match t {
        RustType::Enum(e) if e.name == to_rust_type_name(&enum_name) => Some(e.name.clone()),
        _ => None,
      })
      .unwrap_or_else(|| to_rust_type_name(&enum_name));

    if let Some(c) = cache {
      let (main_type, nested) = Self::extract_main_type(generated_types, &main_type_name)?;
      let registered_name = c.register_type(prop_schema, &enum_name, nested, main_type)?;
      Ok(ConversionOutput::new(TypeRef::new(registered_name)))
    } else {
      Ok(ConversionOutput::with_inline_types(
        TypeRef::new(main_type_name),
        generated_types,
      ))
    }
  }

  /// Extracts the main type from a list of generated types.
  ///
  /// Searches for a type matching the target name. If not found,
  /// falls back to the last type in the list (which is typically the main enum).
  fn extract_main_type(mut types: Vec<RustType>, target_name: &str) -> Result<(RustType, Vec<RustType>)> {
    let pos = types
      .iter()
      .position(|t| match t {
        RustType::Enum(e) => e.name == target_name,
        _ => false,
      })
      .or_else(|| (!types.is_empty()).then_some(types.len() - 1))
      .ok_or_else(|| anyhow::anyhow!("Failed to locate generated union type"))?;

    let main = types.remove(pos);
    Ok((main, types))
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

    if let Some(ref_name) = SchemaGraph::extract_ref_name_from_ref(non_null_variant) {
      let mut type_ref = TypeRef::new(to_rust_type_name(&ref_name));
      if self.graph.is_cyclic(&ref_name) {
        type_ref = type_ref.with_boxed();
      }
      return Ok(Some(type_ref.with_option()));
    }

    let resolved = non_null_variant
      .resolve(self.graph.spec())
      .context("Resolving non-null union variant")?;

    Ok(Some(self.schema_to_type_ref(&resolved)?.with_option()))
  }

  /// Tries to convert a union schema into a single `TypeRef` (e.g. `Option<T>`, `Vec<T>`).
  pub(crate) fn try_convert_union_to_type_ref(
    &self,
    variants: &[ObjectOrReference<ObjectSchema>],
  ) -> anyhow::Result<Option<TypeRef>> {
    if let Some(name) = self.find_matching_union_schema(variants) {
      let mut type_ref = TypeRef::new(to_rust_type_name(&name));
      if self.graph.is_cyclic(&name) {
        type_ref = type_ref.with_boxed();
      }
      return Ok(Some(type_ref));
    }

    if let Some(non_null_variant) = self.find_non_null_variant(variants) {
      if let Some(ref_name) = SchemaGraph::extract_ref_name_from_ref(non_null_variant) {
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
    let mut fallback_type: Option<TypeRef> = None;

    for variant_ref in variants {
      if let Some(ref_name) = SchemaGraph::extract_ref_name_from_ref(variant_ref) {
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
        && let Some(ref_name) = SchemaGraph::extract_ref_name_from_ref(&resolved.one_of[0])
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

    if let Some(ref_name) = SchemaGraph::extract_ref_name_from_ref(items_ref) {
      return Ok(TypeRef::new(to_rust_type_name(&ref_name)));
    }

    let items_schema = items_ref.resolve(self.graph.spec()).context("Resolving array items")?;

    self.schema_to_type_ref(&items_schema)
  }

  fn try_resolve_by_title(&self, schema: &ObjectSchema) -> Option<TypeRef> {
    let title = schema.title.as_ref()?;
    if schema.schema_type.is_some() {
      return None;
    }
    self.graph.get_schema(title)?;
    let mut type_ref = TypeRef::new(to_rust_type_name(title));
    if self.graph.is_cyclic(title) {
      type_ref = type_ref.with_boxed();
    }
    Some(type_ref)
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
    .filter_map(SchemaGraph::extract_ref_name_from_ref)
    .collect()
}

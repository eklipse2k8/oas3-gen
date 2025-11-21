use std::collections::{BTreeMap, BTreeSet, HashSet};

use anyhow::Context;
use oas3::spec::{ObjectOrReference, ObjectSchema, SchemaType, SchemaTypeSet};

use super::{
  ConversionResult, cache::SharedSchemaCache, field_optionality::FieldOptionalityPolicy, metadata, naming,
  structs::StructConverter, type_resolver::TypeResolver,
};
use crate::{
  generator::{
    ast::{EnumDef, RustType, SerdeAttribute, TypeRef, VariantContent, VariantDef, default_enum_derives},
    schema_graph::SchemaGraph,
  },
  reserved::to_rust_type_name,
};

#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub(crate) enum UnionKind {
  OneOf,
  AnyOf,
}

#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub(crate) enum CollisionStrategy {
  /// Append an index to the new variant (e.g., `Value`, `Value1`).
  Preserve,
  /// Merge with existing variant and add a serde alias.
  Deduplicate,
}

/// Holds the result of normalizing a schema value into a Rust identifier.
pub(crate) struct NormalizedVariant {
  /// The valid Rust identifier (e.g., "Value10_5").
  pub(crate) name: String,
  /// The original value string for serialization (e.g., "10.5").
  pub(crate) rename_value: String,
}

#[derive(Clone)]
pub(crate) struct EnumConverter<'a> {
  graph: &'a SchemaGraph,
  type_resolver: TypeResolver<'a>,
  struct_converter: StructConverter<'a>,
  preserve_case_variants: bool,
  case_insensitive_enums: bool,
}

impl<'a> EnumConverter<'a> {
  pub(crate) fn new(
    graph: &'a SchemaGraph,
    type_resolver: TypeResolver<'a>,
    preserve_case_variants: bool,
    case_insensitive_enums: bool,
  ) -> Self {
    let struct_converter = StructConverter::new(graph, type_resolver.clone(), None, FieldOptionalityPolicy::standard());
    Self {
      graph,
      type_resolver,
      struct_converter,
      preserve_case_variants,
      case_insensitive_enums,
    }
  }

  /// Converts a simple enum (list of values) into a Rust Enum.
  pub(crate) fn convert_simple_enum(&self, name: &str, schema: &ObjectSchema) -> RustType {
    let strategy = if self.preserve_case_variants {
      CollisionStrategy::Preserve
    } else {
      CollisionStrategy::Deduplicate
    };

    self.build_simple_enum(name, schema, strategy)
  }

  /// Converts a union (oneOf/anyOf) into a Rust Enum.
  pub(crate) fn convert_union_enum(
    &self,
    name: &str,
    schema: &ObjectSchema,
    kind: UnionKind,
    mut cache: Option<&mut SharedSchemaCache>,
  ) -> ConversionResult<Vec<RustType>> {
    if kind == UnionKind::AnyOf {
      let optimizer = StringEnumOptimizer::new(self.graph, self.case_insensitive_enums);
      if let Some(result) = optimizer.try_convert(name, schema, cache.as_deref_mut()) {
        return Ok(result);
      }
    }
    let processor = UnionProcessor::new(self, name, schema, kind);
    processor.process(cache)
  }

  fn build_simple_enum(&self, name: &str, schema: &ObjectSchema, strategy: CollisionStrategy) -> RustType {
    let mut variants: Vec<VariantDef> = Vec::new();
    // Map of VariantName -> Index in `variants` vector
    let mut seen_names: BTreeMap<String, usize> = BTreeMap::new();

    for (i, value) in schema.enum_values.iter().enumerate() {
      let Some(normalized) = VariantNameNormalizer::normalize(value) else {
        continue;
      };

      match seen_names.get(&normalized.name) {
        Some(&existing_idx) if strategy == CollisionStrategy::Deduplicate => {
          variants[existing_idx]
            .serde_attrs
            .push(SerdeAttribute::Alias(normalized.rename_value));
        }
        Some(_) => {
          let unique_name = format!("{}{}", normalized.name, i);
          let idx = variants.len();
          seen_names.insert(unique_name.clone(), idx);
          Self::push_variant(&mut variants, unique_name, &normalized.rename_value);
        }
        None => {
          let idx = variants.len();
          seen_names.insert(normalized.name.clone(), idx);
          Self::push_variant(&mut variants, normalized.name, &normalized.rename_value);
        }
      }
    }

    RustType::Enum(EnumDef {
      name: to_rust_type_name(name),
      docs: metadata::extract_docs(schema.description.as_ref()),
      variants,
      discriminator: None,
      derives: default_enum_derives(true),
      serde_attrs: vec![],
      outer_attrs: vec![],
      case_insensitive: self.case_insensitive_enums,
    })
  }

  fn push_variant(variants: &mut Vec<VariantDef>, name: String, rename: &str) {
    variants.push(VariantDef {
      name,
      docs: vec![],
      content: VariantContent::Unit,
      serde_attrs: vec![SerdeAttribute::Rename(rename.to_string())],
      deprecated: false,
    });
  }
}

/// Normalizes JSON values into valid Rust variant names.
///
/// Converts strings, numbers, and booleans into PascalCase identifiers
/// suitable for enum variants, preserving original values for serde rename.
pub(crate) struct VariantNameNormalizer;

impl VariantNameNormalizer {
  pub(crate) fn normalize(value: &serde_json::Value) -> Option<NormalizedVariant> {
    if let Some(str_val) = value.as_str() {
      Some(NormalizedVariant {
        name: to_rust_type_name(str_val),
        rename_value: str_val.to_string(),
      })
    } else if let Some(num_val) = value.as_i64() {
      Some(NormalizedVariant {
        name: format!("Value{num_val}"),
        rename_value: num_val.to_string(),
      })
    } else if let Some(num_val) = value.as_f64() {
      let raw_str = num_val.to_string();
      let safe_name = raw_str.replace(['.', '-'], "_");
      Some(NormalizedVariant {
        name: format!("Value{safe_name}"),
        rename_value: raw_str,
      })
    } else {
      value.as_bool().map(|bool_val| NormalizedVariant {
        name: if bool_val { "True".into() } else { "False".into() },
        rename_value: bool_val.to_string(),
      })
    }
  }
}

/// Processes union types (oneOf/anyOf) into Rust enum definitions.
///
/// Handles reference variants, inline schemas, discriminator mapping,
/// and generates nested types for complex variants with properties.
struct UnionProcessor<'a, 'b> {
  converter: &'b EnumConverter<'a>,
  name: &'b str,
  schema: &'b ObjectSchema,
  kind: UnionKind,
  discriminator_map: BTreeMap<String, String>,
}

impl<'a, 'b> UnionProcessor<'a, 'b> {
  fn new(converter: &'b EnumConverter<'a>, name: &'b str, schema: &'b ObjectSchema, kind: UnionKind) -> Self {
    let discriminator_map = if kind == UnionKind::OneOf {
      Self::build_discriminator_map(schema)
    } else {
      BTreeMap::new()
    };

    Self {
      converter,
      name,
      schema,
      kind,
      discriminator_map,
    }
  }

  fn build_discriminator_map(schema: &ObjectSchema) -> BTreeMap<String, String> {
    schema
      .discriminator
      .as_ref()
      .and_then(|d| d.mapping.as_ref())
      .map(|mapping| {
        mapping
          .iter()
          .filter_map(|(val, ref_path)| SchemaGraph::extract_ref_name(ref_path).map(|name| (name, val.clone())))
          .collect()
      })
      .unwrap_or_default()
  }

  fn process(&self, mut cache: Option<&mut SharedSchemaCache>) -> ConversionResult<Vec<RustType>> {
    let variants_src = match self.kind {
      UnionKind::OneOf => &self.schema.one_of,
      UnionKind::AnyOf => &self.schema.any_of,
    };

    let mut inline_types = Vec::new();
    let mut variants = Vec::new();
    let mut seen_names = BTreeSet::new();

    for (i, variant_ref) in variants_src.iter().enumerate() {
      let resolved = variant_ref
        .resolve(self.converter.graph.spec())
        .with_context(|| format!("Schema resolution failed for union variant {i}"))?;

      // Skip explicit nulls in unions (usually handled by Option wrapper upstream)
      if resolved.schema_type == Some(SchemaTypeSet::Single(SchemaType::Null)) {
        continue;
      }

      let (variant, mut generated) =
        self.process_single_variant(i, variant_ref, &resolved, &mut seen_names, cache.as_deref_mut())?;

      variants.push(variant);
      inline_types.append(&mut generated);
    }

    strip_common_affixes(&mut variants);

    let main_enum = self.build_enum_def(variants);
    inline_types.push(main_enum);

    Ok(inline_types)
  }

  fn process_single_variant(
    &self,
    index: usize,
    variant_ref: &ObjectOrReference<ObjectSchema>,
    resolved_schema: &ObjectSchema,
    seen_names: &mut BTreeSet<String>,
    cache: Option<&mut SharedSchemaCache>,
  ) -> ConversionResult<(VariantDef, Vec<RustType>)> {
    // Case A: It's a Reference (e.g., $ref: "#/components/schemas/User")
    if let Some(schema_name) = SchemaGraph::extract_ref_name_from_ref(variant_ref) {
      return Ok(self.create_ref_variant(&schema_name, resolved_schema, seen_names));
    }

    // Case B: It's an Inline Schema
    self.create_inline_variant(index, resolved_schema, seen_names, cache)
  }

  fn create_ref_variant(
    &self,
    schema_name: &str,
    resolved_schema: &ObjectSchema,
    seen_names: &mut BTreeSet<String>,
  ) -> (VariantDef, Vec<RustType>) {
    let rust_type_name = to_rust_type_name(schema_name);
    let mut type_ref = TypeRef::new(&rust_type_name);

    if self.converter.graph.is_cyclic(schema_name) {
      type_ref = type_ref.with_boxed();
    }

    let variant_name = naming::ensure_unique(&rust_type_name, seen_names);

    let mut serde_attrs = Vec::new();
    if let Some(disc_value) = self.discriminator_map.get(schema_name) {
      serde_attrs.push(SerdeAttribute::Rename(disc_value.clone()));
    }

    let variant = VariantDef {
      name: variant_name,
      docs: metadata::extract_docs(resolved_schema.description.as_ref()),
      content: VariantContent::Tuple(vec![type_ref]),
      serde_attrs,
      deprecated: resolved_schema.deprecated.unwrap_or(false),
    };

    (variant, vec![])
  }

  fn create_inline_variant(
    &self,
    index: usize,
    resolved_schema: &ObjectSchema,
    seen_names: &mut BTreeSet<String>,
    cache: Option<&mut SharedSchemaCache>,
  ) -> ConversionResult<(VariantDef, Vec<RustType>)> {
    if let Some(const_value) = &resolved_schema.const_value {
      return self.create_const_variant(const_value, resolved_schema, seen_names);
    }

    let base_name = resolved_schema
      .title
      .as_ref()
      .map_or_else(|| infer_variant_name(resolved_schema, index), |t| to_rust_type_name(t));
    let variant_name = naming::ensure_unique(&base_name, seen_names);

    let (content, generated_types) = if resolved_schema.properties.is_empty() {
      let type_ref = self.converter.type_resolver.schema_to_type_ref(resolved_schema)?;
      (VariantContent::Tuple(vec![type_ref]), vec![])
    } else {
      let struct_name_prefix = format!("{}{}", self.name, variant_name);
      let (struct_def, mut inline_types) =
        self
          .converter
          .struct_converter
          .convert_struct(&struct_name_prefix, resolved_schema, None, cache)?;

      let struct_name = match &struct_def {
        RustType::Struct(s) => s.name.clone(),
        _ => unreachable!("convert_struct must return a Struct"),
      };

      inline_types.push(struct_def);
      (VariantContent::Tuple(vec![TypeRef::new(struct_name)]), inline_types)
    };

    let variant = VariantDef {
      name: variant_name,
      docs: metadata::extract_docs(resolved_schema.description.as_ref()),
      content,
      serde_attrs: vec![],
      deprecated: resolved_schema.deprecated.unwrap_or(false),
    };

    Ok((variant, generated_types))
  }

  fn create_const_variant(
    &self,
    const_value: &serde_json::Value,
    resolved_schema: &ObjectSchema,
    seen_names: &mut BTreeSet<String>,
  ) -> ConversionResult<(VariantDef, Vec<RustType>)> {
    let normalized = VariantNameNormalizer::normalize(const_value)
      .ok_or_else(|| anyhow::anyhow!("Unsupported const value type: {const_value}"))?;

    let variant_name = naming::ensure_unique(&normalized.name, seen_names);

    let variant = VariantDef {
      name: variant_name,
      docs: metadata::extract_docs(resolved_schema.description.as_ref()),
      content: VariantContent::Unit,
      serde_attrs: vec![SerdeAttribute::Rename(normalized.rename_value)],
      deprecated: resolved_schema.deprecated.unwrap_or(false),
    };

    Ok((variant, vec![]))
  }

  fn build_enum_def(&self, variants: Vec<VariantDef>) -> RustType {
    let has_discriminator = self.schema.discriminator.is_some();

    // If AnyOf and no discriminator, we use untagged to allow Serde to try matching fields
    let (serde_attrs, derives) = if self.kind == UnionKind::AnyOf && !has_discriminator {
      (vec![SerdeAttribute::Untagged], default_enum_derives(false))
    } else {
      (vec![], default_enum_derives(false))
    };

    RustType::Enum(EnumDef {
      name: to_rust_type_name(self.name),
      docs: metadata::extract_docs(self.schema.description.as_ref()),
      variants,
      discriminator: self.schema.discriminator.as_ref().map(|d| d.property_name.clone()),
      derives,
      serde_attrs,
      outer_attrs: vec![],
      case_insensitive: false,
    })
  }
}

/// Optimizes anyOf unions containing string enums and a freeform string.
///
/// Detects patterns like `anyOf: [const "foo", const "bar", type: string]`
/// and generates a two-variant enum: Known(KnownEnum) | Other(String).
/// This provides type safety for known values while accepting unknown ones.
pub(crate) struct StringEnumOptimizer<'a> {
  graph: &'a SchemaGraph,
  case_insensitive: bool,
}

impl<'a> StringEnumOptimizer<'a> {
  pub(crate) fn new(graph: &'a SchemaGraph, case_insensitive: bool) -> Self {
    Self {
      graph,
      case_insensitive,
    }
  }

  pub(crate) fn try_convert(
    &self,
    name: &str,
    schema: &ObjectSchema,
    cache: Option<&mut SharedSchemaCache>,
  ) -> Option<Vec<RustType>> {
    if !self.has_freeform_string(schema) {
      return None;
    }

    let known_values = self.collect_known_values(schema);
    if known_values.is_empty() {
      return None;
    }

    Some(self.generate_optimized_types(name, schema, &known_values, cache))
  }

  fn has_freeform_string(&self, schema: &ObjectSchema) -> bool {
    schema.any_of.iter().any(|s| {
      s.resolve(self.graph.spec()).ok().is_some_and(|resolved| {
        resolved.const_value.is_none()
          && resolved.enum_values.is_empty()
          && resolved.schema_type == Some(SchemaTypeSet::Single(SchemaType::String))
      })
    })
  }

  fn collect_known_values(&self, schema: &ObjectSchema) -> Vec<(String, Option<String>, bool)> {
    let mut seen_values = HashSet::new();
    let mut known_values = Vec::new();

    for variant in &schema.any_of {
      let Ok(resolved) = variant.resolve(self.graph.spec()) else {
        continue;
      };

      // Check for const string
      if let Some(const_val) = resolved.const_value.as_ref().and_then(|v| v.as_str()) {
        if seen_values.insert(const_val.to_string()) {
          known_values.push((
            const_val.to_string(),
            resolved.description.clone(),
            resolved.deprecated.unwrap_or(false),
          ));
        }
        continue;
      }

      // Check for enum strings
      if resolved.schema_type == Some(SchemaTypeSet::Single(SchemaType::String)) {
        for enum_value in &resolved.enum_values {
          if let Some(str_val) = enum_value.as_str()
            && seen_values.insert(str_val.to_string())
          {
            known_values.push((
              str_val.to_string(),
              resolved.description.clone(),
              resolved.deprecated.unwrap_or(false),
            ));
          }
        }
      }
    }
    known_values
  }

  fn generate_optimized_types(
    &self,
    name: &str,
    schema: &ObjectSchema,
    known_values: &[(String, Option<String>, bool)],
    cache: Option<&mut SharedSchemaCache>,
  ) -> Vec<RustType> {
    let base_name = to_rust_type_name(name);

    // Generate cache key based on sorted values
    let mut cache_key_values: Vec<String> = known_values.iter().map(|(v, _, _)| v.clone()).collect();
    cache_key_values.sort();

    let (known_enum_name, inner_enum_type) =
      self.resolve_cached_enum(&base_name, known_values, cache_key_values, cache);

    let outer_enum = Self::build_outer_enum(&base_name, &known_enum_name, schema);

    let mut types = Vec::new();
    if let Some(ie) = inner_enum_type {
      types.push(ie);
    }
    types.push(outer_enum);
    types
  }

  fn resolve_cached_enum(
    &self,
    base_name: &str,
    known_values: &[(String, Option<String>, bool)],
    cache_key: Vec<String>,
    cache: Option<&mut SharedSchemaCache>,
  ) -> (String, Option<RustType>) {
    if let Some(c) = cache {
      if let Some(existing) = c.get_enum_name(&cache_key) {
        let name = existing.clone();
        if c.is_enum_generated(&cache_key) {
          (name, None)
        } else {
          let def = self.build_known_enum(&name, known_values);
          c.register_enum(cache_key, name.clone());
          c.mark_name_used(name.clone());
          (name, Some(def))
        }
      } else {
        let name = format!("{base_name}Known");
        let def = self.build_known_enum(&name, known_values);
        c.register_enum(cache_key, name.clone());
        c.mark_name_used(name.clone());
        (name, Some(def))
      }
    } else {
      let name = format!("{base_name}Known");
      (name.clone(), Some(self.build_known_enum(&name, known_values)))
    }
  }

  fn build_known_enum(&self, name: &str, values: &[(String, Option<String>, bool)]) -> RustType {
    let mut seen_names = BTreeSet::new();
    let mut variants = Vec::new();

    for (value, description, deprecated) in values {
      let base_name = to_rust_type_name(value);
      let variant_name = naming::ensure_unique(&base_name, &seen_names);
      seen_names.insert(variant_name.clone());

      variants.push(VariantDef {
        name: variant_name,
        docs: metadata::extract_docs(description.as_ref()),
        content: VariantContent::Unit,
        serde_attrs: vec![SerdeAttribute::Rename(value.clone())],
        deprecated: *deprecated,
      });
    }

    RustType::Enum(EnumDef {
      name: name.to_string(),
      docs: vec!["/// Known values for the string enum.".to_string()],
      variants,
      discriminator: None,
      derives: default_enum_derives(true),
      serde_attrs: vec![],
      outer_attrs: vec![],
      case_insensitive: self.case_insensitive,
    })
  }

  fn build_outer_enum(name: &str, known_type_name: &str, schema: &ObjectSchema) -> RustType {
    let variants = vec![
      VariantDef {
        name: "Known".to_string(),
        docs: vec!["/// A known value.".to_string()],
        content: VariantContent::Tuple(vec![TypeRef::new(known_type_name)]),
        serde_attrs: vec![SerdeAttribute::Default],
        deprecated: false,
      },
      VariantDef {
        name: "Other".to_string(),
        docs: vec!["/// An unknown value.".to_string()],
        content: VariantContent::Tuple(vec![TypeRef::new("String")]),
        serde_attrs: vec![],
        deprecated: false,
      },
    ];

    RustType::Enum(EnumDef {
      name: name.to_string(),
      docs: metadata::extract_docs(schema.description.as_ref()),
      variants,
      discriminator: None,
      derives: default_enum_derives(false),
      serde_attrs: vec![SerdeAttribute::Untagged],
      outer_attrs: vec![],
      case_insensitive: false,
    })
  }
}

pub(crate) fn infer_variant_name(schema: &ObjectSchema, index: usize) -> String {
  if !schema.enum_values.is_empty() {
    return "Enum".to_string();
  }
  if let Some(ref schema_type) = schema.schema_type {
    match schema_type {
      SchemaTypeSet::Single(typ) => match typ {
        SchemaType::String => "String".to_string(),
        SchemaType::Number => "Number".to_string(),
        SchemaType::Integer => "Integer".to_string(),
        SchemaType::Boolean => "Boolean".to_string(),
        SchemaType::Array => "Array".to_string(),
        SchemaType::Object => "Object".to_string(),
        SchemaType::Null => "Null".to_string(),
      },
      SchemaTypeSet::Multiple(_) => "Mixed".to_string(),
    }
  } else {
    format!("Variant{index}")
  }
}

pub(crate) fn strip_common_affixes(variants: &mut [VariantDef]) {
  let variant_names: Vec<_> = variants.iter().map(|v| v.name.clone()).collect();
  if variant_names.len() < 2 {
    return;
  }

  let split_names: Vec<Vec<String>> = variant_names.iter().map(|n| split_pascal_case(n)).collect();

  let common_prefix_len = find_common_prefix_len(&split_names);
  let common_suffix_len = find_common_suffix_len(&split_names);

  let mut stripped_names = Vec::new();
  for words in &split_names {
    let start = common_prefix_len;
    let end = words.len().saturating_sub(common_suffix_len);
    if start >= end {
      stripped_names.push(words.join(""));
    } else {
      stripped_names.push(words[start..end].join(""));
    }
  }

  let mut seen = BTreeSet::new();
  if stripped_names.iter().any(|n| n.is_empty() || !seen.insert(n)) {
    return;
  }

  for (variant, new_name) in variants.iter_mut().zip(stripped_names) {
    variant.name = new_name;
  }
}

fn find_common_prefix_len(split_names: &[Vec<String>]) -> usize {
  let Some(first) = split_names.first() else {
    return 0;
  };
  let mut len = 0;
  'outer: for (i, word) in first.iter().enumerate() {
    for other in &split_names[1..] {
      if other.get(i) != Some(word) {
        break 'outer;
      }
    }
    len = i + 1;
  }
  len
}

fn find_common_suffix_len(split_names: &[Vec<String>]) -> usize {
  let Some(first) = split_names.first() else {
    return 0;
  };
  let mut len = 0;
  'outer: for i in 1..=first.len() {
    let word = &first[first.len() - i];
    for other in &split_names[1..] {
      if other.len() < i || &other[other.len() - i] != word {
        break 'outer;
      }
    }
    len = i;
  }
  len
}

fn split_pascal_case(name: &str) -> Vec<String> {
  let mut words = Vec::new();
  let mut current_word = String::new();
  for (i, ch) in name.chars().enumerate() {
    if ch.is_uppercase() && i > 0 && !current_word.is_empty() {
      words.push(std::mem::take(&mut current_word));
    }
    current_word.push(ch);
  }
  if !current_word.is_empty() {
    words.push(current_word);
  }
  words
}

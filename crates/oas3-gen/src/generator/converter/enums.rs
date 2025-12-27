use std::{
  collections::{BTreeMap, BTreeSet, HashSet},
  sync::Arc,
};

use anyhow::Context;
use oas3::spec::{ObjectOrReference, ObjectSchema};
use serde_json::Value;

use super::{
  CodegenConfig,
  cache::{SharedSchemaCache, StructSummary},
  common::{SchemaExt, handle_inline_creation},
  metadata,
  structs::StructConverter,
  type_resolver::TypeResolver,
};
use crate::generator::{
  ast::{
    DiscriminatedEnumDef, DiscriminatedVariant, Documentation, EnumDef, EnumMethod, EnumMethodKind, EnumToken,
    EnumVariantToken, RustType, SerdeAttribute, TypeRef, VariantContent, VariantDef,
  },
  naming::{
    identifiers::{ensure_unique, to_rust_type_name},
    inference::{
      VariantNameNormalizer, derive_method_names, extract_enum_values, infer_variant_name, strip_common_affixes,
    },
  },
  schema_registry::{ReferenceExtractor, SchemaRegistry},
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

struct EnumValueEntry {
  value: Value,
  docs: Documentation,
  deprecated: bool,
}

struct UnionVariantSpec {
  variant_name: EnumVariantToken,
  resolved_schema: ObjectSchema,
  ref_name: Option<String>,
}

#[derive(Clone)]
pub(crate) struct EnumConverter {
  graph: Arc<SchemaRegistry>,
  type_resolver: TypeResolver,
  struct_converter: StructConverter,
  preserve_case_variants: bool,
  case_insensitive_enums: bool,
  pub(crate) no_helpers: bool,
}

impl EnumConverter {
  pub(crate) fn new(graph: &Arc<SchemaRegistry>, type_resolver: TypeResolver, config: CodegenConfig) -> Self {
    let struct_converter = StructConverter::new(graph, config, None);
    Self {
      graph: graph.clone(),
      type_resolver,
      struct_converter,
      preserve_case_variants: config.preserve_case_variants(),
      case_insensitive_enums: config.case_insensitive_enums(),
      no_helpers: config.no_helpers(),
    }
  }

  /// Converts a value-based enum (list of string/number/bool values) into a Rust Enum.
  pub(crate) fn convert_value_enum(
    &self,
    name: &str,
    schema: &ObjectSchema,
    cache: Option<&mut SharedSchemaCache>,
  ) -> Option<RustType> {
    let mut enum_values: Vec<String> = schema
      .enum_values
      .iter()
      .filter_map(|v| v.as_str().map(String::from))
      .collect();
    enum_values.sort();

    if cache.as_ref().is_some_and(|c| c.is_enum_generated(&enum_values)) {
      return None;
    }

    let strategy = if self.preserve_case_variants {
      CollisionStrategy::Preserve
    } else {
      CollisionStrategy::Deduplicate
    };

    let entries: Vec<EnumValueEntry> = schema
      .enum_values
      .iter()
      .cloned()
      .map(|value| EnumValueEntry {
        value,
        docs: Documentation::default(),
        deprecated: false,
      })
      .collect();

    let enum_def = Self::build_enum_from_values(
      name,
      &entries,
      strategy,
      metadata::extract_docs(schema.description.as_ref()),
      self.case_insensitive_enums,
    );

    if let (Some(c), RustType::Enum(e)) = (cache, &enum_def) {
      c.register_enum(enum_values, e.name.to_string());
      c.mark_name_used(e.name.to_string());
    }

    Some(enum_def)
  }

  /// Converts a union (oneOf/anyOf) into a Rust Enum.
  pub(crate) fn convert_union(
    &self,
    name: &str,
    schema: &ObjectSchema,
    kind: UnionKind,
    mut cache: Option<&mut SharedSchemaCache>,
  ) -> anyhow::Result<Vec<RustType>> {
    if kind == UnionKind::AnyOf
      && let Some(result) = self.try_build_relaxed_enum(name, schema, cache.as_deref_mut())
    {
      return Ok(result);
    }

    let result = self.collect_union_variants(name, schema, kind, cache.as_deref_mut())?;

    if let Some(c) = cache
      && let Some(values) = extract_enum_values(schema)
      && let Some(RustType::Enum(e)) = result.last()
    {
      c.register_enum(values, e.name.to_string());
    }

    Ok(result)
  }

  fn collect_union_variants(
    &self,
    name: &str,
    schema: &ObjectSchema,
    kind: UnionKind,
    mut cache: Option<&mut SharedSchemaCache>,
  ) -> anyhow::Result<Vec<RustType>> {
    let variants_src = match kind {
      UnionKind::OneOf => &schema.one_of,
      UnionKind::AnyOf => &schema.any_of,
    };

    let specs = self.collect_union_variant_specs(variants_src)?;

    let mut inline_types = vec![];
    let mut variants = vec![];
    for spec in specs {
      let (variant, mut generated) = self.build_union_variant(name, &spec, cache.as_deref_mut())?;
      variants.push(variant);
      inline_types.append(&mut generated);
    }

    strip_common_affixes(&mut variants);

    let methods = if self.no_helpers {
      vec![]
    } else {
      self.build_constructors(&variants, &inline_types, name, cache)
    };

    let main_enum = Self::build_union_def(name, schema, kind, variants, methods);
    inline_types.push(main_enum);

    Ok(inline_types)
  }

  fn collect_union_variant_specs(
    &self,
    variants_src: &[ObjectOrReference<ObjectSchema>],
  ) -> anyhow::Result<Vec<UnionVariantSpec>> {
    let mut specs = vec![];
    let mut seen_names = BTreeSet::new();

    for (i, variant_ref) in variants_src.iter().enumerate() {
      let resolved = variant_ref
        .resolve(self.graph.spec())
        .with_context(|| format!("Schema resolution failed for union variant {i}"))?;

      if resolved.is_null() {
        continue;
      }

      let ref_name = ReferenceExtractor::extract_ref_name_from_obj_ref(variant_ref).or_else(|| {
        if resolved.all_of.len() == 1 {
          ReferenceExtractor::extract_ref_name_from_obj_ref(&resolved.all_of[0])
        } else {
          None
        }
      });

      let base_name = if let Some(const_value) = &resolved.const_value {
        VariantNameNormalizer::normalize(const_value).map_or_else(|| infer_variant_name(&resolved, i), |n| n.name)
      } else if let Some(schema_name) = &ref_name {
        to_rust_type_name(schema_name)
      } else {
        resolved
          .title
          .as_ref()
          .map_or_else(|| infer_variant_name(&resolved, i), |t| to_rust_type_name(t))
      };

      let variant_name = ensure_unique(&base_name, &seen_names);
      seen_names.insert(variant_name.clone());

      specs.push(UnionVariantSpec {
        variant_name: EnumVariantToken::new(variant_name),
        resolved_schema: resolved,
        ref_name,
      });
    }

    Ok(specs)
  }

  fn build_union_variant(
    &self,
    enum_name: &str,
    spec: &UnionVariantSpec,
    cache: Option<&mut SharedSchemaCache>,
  ) -> anyhow::Result<(VariantDef, Vec<RustType>)> {
    if let Some(ref schema_name) = spec.ref_name {
      Ok(self.build_ref_variant(schema_name, &spec.resolved_schema, spec.variant_name.clone()))
    } else {
      self.build_inline_variant(&spec.resolved_schema, enum_name, spec.variant_name.clone(), cache)
    }
  }

  fn build_enum_from_values(
    name: &str,
    entries: &[EnumValueEntry],
    strategy: CollisionStrategy,
    docs: Documentation,
    case_insensitive: bool,
  ) -> RustType {
    let mut variants: Vec<VariantDef> = vec![];
    let mut seen_names: BTreeMap<String, usize> = BTreeMap::new();

    for (i, entry) in entries.iter().enumerate() {
      let Some(normalized) = VariantNameNormalizer::normalize(&entry.value) else {
        continue;
      };

      match seen_names.get(&normalized.name) {
        Some(&existing_idx) if strategy == CollisionStrategy::Deduplicate => {
          variants[existing_idx]
            .serde_attrs
            .push(SerdeAttribute::Alias(normalized.rename_value));
        }
        Some(_) => {
          let unique_name = format!("{}{i}", normalized.name);
          let idx = variants.len();
          seen_names.insert(unique_name.clone(), idx);
          variants.push(VariantDef {
            name: EnumVariantToken::from(unique_name),
            docs: entry.docs.clone(),
            content: VariantContent::Unit,
            serde_attrs: vec![SerdeAttribute::Rename(normalized.rename_value)],
            deprecated: entry.deprecated,
          });
        }
        None => {
          let idx = variants.len();
          seen_names.insert(normalized.name.clone(), idx);
          variants.push(VariantDef {
            name: EnumVariantToken::from(normalized.name),
            docs: entry.docs.clone(),
            content: VariantContent::Unit,
            serde_attrs: vec![SerdeAttribute::Rename(normalized.rename_value)],
            deprecated: entry.deprecated,
          });
        }
      }
    }

    RustType::Enum(EnumDef {
      name: EnumToken::from_raw(name),
      docs,
      variants,
      case_insensitive,
      ..Default::default()
    })
  }

  fn build_constructors(
    &self,
    variants: &[VariantDef],
    inline_types: &[RustType],
    enum_name: &str,
    mut cache: Option<&mut SharedSchemaCache>,
  ) -> Vec<EnumMethod> {
    let enum_name = to_rust_type_name(enum_name);

    // Build a map of struct summaries from inline types
    let mut summary_cache: BTreeMap<String, StructSummary> = inline_types
      .iter()
      .filter_map(|t| match t {
        RustType::Struct(s) => {
          let summary = StructSummary {
            has_default: s.has_default(),
            required_fields: s
              .required_fields()
              .map(|f| (f.name.clone(), f.rust_type.clone()))
              .collect(),
            user_fields: s
              .fields
              .iter()
              .filter(|f| !f.doc_hidden)
              .map(|f| (f.name.clone(), f.rust_type.clone()))
              .collect(),
          };
          Some((s.name.to_string(), summary))
        }
        _ => None,
      })
      .collect();

    let mut eligible = vec![];

    for variant in variants {
      let Some(type_ref) = variant.single_wrapped_type() else {
        continue;
      };

      // We need to reborrow cache for each iteration
      let cache_reborrow = cache.as_deref_mut();
      let Some(summary) = self.resolve_struct_summary(type_ref, cache_reborrow, &mut summary_cache) else {
        continue;
      };

      if let Some(method_kind) = Self::constructor_kind_for(type_ref, &variant.name, &summary) {
        eligible.push((variant.name.clone(), method_kind));
      }
    }

    if eligible.is_empty() {
      return vec![];
    }

    let variant_names: Vec<String> = eligible.iter().map(|(name, _)| name.to_string()).collect();
    let method_names = derive_method_names(&enum_name, &variant_names);

    let mut seen = BTreeSet::new();
    eligible
      .into_iter()
      .zip(method_names)
      .map(|((variant_name, kind), base_name)| {
        let method_name = ensure_unique(&base_name, &seen);
        seen.insert(method_name.clone());
        let docs = variants
          .iter()
          .find(|v| v.name == variant_name)
          .map(|v| v.docs.clone())
          .unwrap_or_default();

        EnumMethod::new(method_name, kind, docs)
      })
      .collect()
  }

  fn constructor_kind_for(
    type_ref: &TypeRef,
    variant_name: &EnumVariantToken,
    summary: &StructSummary,
  ) -> Option<EnumMethodKind> {
    if !summary.has_default || type_ref.is_array {
      return None;
    }

    match summary.required_fields.len() {
      0 => {
        if summary.user_fields.len() == 1 {
          let (ref name, ref rust_type) = summary.user_fields[0];
          Some(EnumMethodKind::ParameterizedConstructor {
            variant_name: variant_name.clone(),
            wrapped_type: type_ref.clone(),
            param_name: name.to_string(),
            param_type: rust_type.clone(),
          })
        } else {
          Some(EnumMethodKind::SimpleConstructor {
            variant_name: variant_name.clone(),
            wrapped_type: type_ref.clone(),
          })
        }
      }
      1 => {
        let (ref name, ref rust_type) = summary.required_fields[0];
        Some(EnumMethodKind::ParameterizedConstructor {
          variant_name: variant_name.clone(),
          wrapped_type: type_ref.clone(),
          param_name: name.to_string(),
          param_type: rust_type.clone(),
        })
      }
      _ => None,
    }
  }

  /// Resolves a struct summary for constructor eligibility.
  ///
  /// First checks the inline types cache, then the shared schema cache,
  /// and finally falls back to converting the schema if needed.
  fn resolve_struct_summary(
    &self,
    type_ref: &TypeRef,
    cache: Option<&mut SharedSchemaCache>,
    summary_cache: &mut BTreeMap<String, StructSummary>,
  ) -> Option<StructSummary> {
    let base_name = type_ref.unboxed_base_type_name();

    // Check local cache first
    if let Some(summary) = summary_cache.get(&base_name) {
      return Some(summary.clone());
    }

    // Check shared schema cache
    // We can't use `if let` easily here because we need to borrow cache mutably later
    // So we just check existence first
    let has_summary = cache
      .as_ref()
      .is_some_and(|c| c.get_struct_summary(&base_name).is_some());

    if has_summary {
      // Safe to unwrap because we checked above
      let summary = cache.as_ref().unwrap().get_struct_summary(&base_name).unwrap().clone();
      summary_cache.insert(base_name, summary.clone());
      return Some(summary);
    }

    // Fall back to conversion if schema exists
    let schema = self.graph.get_schema(&base_name)?;
    if !schema.is_object() && schema.properties.is_empty() {
      return None;
    }

    let struct_result = self
      .struct_converter
      .convert_struct(&base_name, schema, None, cache)
      .ok()?;

    if let RustType::Struct(s) = struct_result.result {
      let summary = StructSummary {
        has_default: s.has_default(),
        required_fields: s
          .required_fields()
          .map(|f| (f.name.clone(), f.rust_type.clone()))
          .collect(),
        user_fields: s
          .fields
          .iter()
          .filter(|f| !f.doc_hidden)
          .map(|f| (f.name.clone(), f.rust_type.clone()))
          .collect(),
      };
      summary_cache.insert(base_name, summary.clone());
      Some(summary)
    } else {
      None
    }
  }

  fn build_ref_variant(
    &self,
    schema_name: &str,
    resolved_schema: &ObjectSchema,
    variant_name: EnumVariantToken,
  ) -> (VariantDef, Vec<RustType>) {
    let rust_type_name = to_rust_type_name(schema_name);
    let mut type_ref = TypeRef::new(&rust_type_name);

    if self.graph.is_cyclic(schema_name) {
      type_ref = type_ref.with_boxed();
    }

    let variant = VariantDef {
      name: variant_name,
      docs: metadata::extract_docs(resolved_schema.description.as_ref()),
      content: VariantContent::Tuple(vec![type_ref]),
      deprecated: resolved_schema.deprecated.unwrap_or(false),
      ..Default::default()
    };

    (variant, vec![])
  }

  fn build_inline_variant(
    &self,
    resolved_schema: &ObjectSchema,
    enum_name: &str,
    variant_name: EnumVariantToken,
    mut cache: Option<&mut SharedSchemaCache>,
  ) -> anyhow::Result<(VariantDef, Vec<RustType>)> {
    let mut resolved_schema_merged = resolved_schema.clone();
    if !resolved_schema.all_of.is_empty() {
      resolved_schema_merged = self.type_resolver.merge_all_of_schema(resolved_schema)?;
    }
    let resolved_schema = &resolved_schema_merged;

    if let Some(result) = Self::build_const_variant(resolved_schema, &variant_name)? {
      return Ok(result);
    }

    let variant_label = variant_name.to_string();

    let (content, generated_types) = if resolved_schema.properties.is_empty() {
      if let Some(result) =
        self.build_array_content(enum_name, &variant_label, resolved_schema, cache.as_deref_mut())?
      {
        result
      } else if let Some(result) =
        self.build_nested_union_content(enum_name, &variant_label, resolved_schema, cache.as_deref_mut())?
      {
        result
      } else {
        self.build_primitive_content(resolved_schema)?
      }
    } else {
      self.build_struct_content(enum_name, &variant_label, resolved_schema, cache)?
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

  fn build_const_variant(
    resolved_schema: &ObjectSchema,
    variant_name: &EnumVariantToken,
  ) -> anyhow::Result<Option<(VariantDef, Vec<RustType>)>> {
    if let Some(const_value) = &resolved_schema.const_value {
      let normalized = VariantNameNormalizer::normalize(const_value)
        .ok_or_else(|| anyhow::anyhow!("Unsupported const value type: {const_value}"))?;

      let variant = VariantDef {
        name: variant_name.clone(),
        docs: metadata::extract_docs(resolved_schema.description.as_ref()),
        content: VariantContent::Unit,
        serde_attrs: vec![SerdeAttribute::Rename(normalized.rename_value)],
        deprecated: resolved_schema.deprecated.unwrap_or(false),
      };

      return Ok(Some((variant, vec![])));
    }

    Ok(None)
  }

  fn build_array_content(
    &self,
    enum_name: &str,
    variant_label: &str,
    resolved_schema: &ObjectSchema,
    cache: Option<&mut SharedSchemaCache>,
  ) -> anyhow::Result<Option<(VariantContent, Vec<RustType>)>> {
    if !resolved_schema.is_array() {
      return Ok(None);
    }

    let conversion =
      self
        .type_resolver
        .resolve_array_with_inline_items(enum_name, variant_label, resolved_schema, cache)?;

    Ok(conversion.map(|c| (VariantContent::Tuple(vec![c.result]), c.inline_types)))
  }

  fn build_nested_union_content(
    &self,
    enum_name: &str,
    variant_label: &str,
    resolved_schema: &ObjectSchema,
    cache: Option<&mut SharedSchemaCache>,
  ) -> anyhow::Result<Option<(VariantContent, Vec<RustType>)>> {
    if resolved_schema.one_of.is_empty() && resolved_schema.any_of.is_empty() {
      return Ok(None);
    }

    let uses_one_of = !resolved_schema.one_of.is_empty();
    let result =
      self
        .type_resolver
        .resolve_inline_union_type(enum_name, variant_label, resolved_schema, uses_one_of, cache)?;

    Ok(Some((VariantContent::Tuple(vec![result.result]), result.inline_types)))
  }

  fn build_struct_content(
    &self,
    enum_name: &str,
    variant_label: &str,
    resolved_schema: &ObjectSchema,
    cache: Option<&mut SharedSchemaCache>,
  ) -> anyhow::Result<(VariantContent, Vec<RustType>)> {
    let struct_name_prefix = format!("{enum_name}{variant_label}");

    let result = handle_inline_creation(
      resolved_schema,
      &struct_name_prefix,
      None,
      cache,
      |_| None,
      |name, cache| self.struct_converter.convert_struct(name, resolved_schema, None, cache),
    )?;

    Ok((VariantContent::Tuple(vec![result.result]), result.inline_types))
  }

  fn build_primitive_content(&self, resolved_schema: &ObjectSchema) -> anyhow::Result<(VariantContent, Vec<RustType>)> {
    let type_ref = self.type_resolver.resolve_type(resolved_schema)?;
    Ok((VariantContent::Tuple(vec![type_ref]), vec![]))
  }

  fn build_union_def(
    name: &str,
    schema: &ObjectSchema,
    _kind: UnionKind,
    variants: Vec<VariantDef>,
    methods: Vec<EnumMethod>,
  ) -> RustType {
    if let Some(discriminated) = Self::build_discriminated_union(name, schema, &variants, methods.clone()) {
      return discriminated;
    }

    RustType::Enum(EnumDef {
      name: EnumToken::from_raw(name),
      docs: metadata::extract_docs(schema.description.as_ref()),
      variants,
      serde_attrs: vec![SerdeAttribute::Untagged],
      case_insensitive: false,
      methods,
      ..Default::default()
    })
  }

  fn build_discriminated_union(
    name: &str,
    schema: &ObjectSchema,
    variants: &[VariantDef],
    methods: Vec<EnumMethod>,
  ) -> Option<RustType> {
    let discriminator = schema.discriminator.as_ref()?;
    let mapping = discriminator.mapping.as_ref()?;

    if !Self::all_variants_are_refs(variants, mapping) {
      return None;
    }

    let disc_variants = Self::build_discriminated_variants(variants, mapping);
    Some(RustType::DiscriminatedEnum(DiscriminatedEnumDef {
      name: EnumToken::from_raw(name),
      docs: metadata::extract_docs(schema.description.as_ref()),
      discriminator_field: discriminator.property_name.clone(),
      variants: disc_variants,
      methods,
      ..Default::default()
    }))
  }

  fn all_variants_are_refs(variants: &[VariantDef], mapping: &BTreeMap<String, String>) -> bool {
    if variants.is_empty() || mapping.is_empty() {
      return false;
    }

    let variant_types: BTreeSet<String> = variants.iter().filter_map(VariantDef::unboxed_type_name).collect();

    mapping
      .values()
      .filter_map(|ref_path| Self::ref_path_to_type_name(ref_path))
      .all(|type_name| variant_types.contains(&type_name))
  }

  fn build_discriminated_variants(
    variants: &[VariantDef],
    mapping: &BTreeMap<String, String>,
  ) -> Vec<DiscriminatedVariant> {
    let mut type_to_disc_values: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for (disc_value, ref_path) in mapping {
      if let Some(expected_type) = Self::ref_path_to_type_name(ref_path) {
        type_to_disc_values
          .entry(expected_type)
          .or_default()
          .push(disc_value.clone());
      }
    }

    type_to_disc_values
      .into_iter()
      .filter_map(|(expected_type, disc_values)| {
        let variant = Self::find_variant_by_type(variants, &expected_type)?;
        let type_ref = variant.single_wrapped_type()?;

        Some(DiscriminatedVariant {
          discriminator_values: disc_values,
          variant_name: variant.name.to_string(),
          type_name: type_ref.clone(),
        })
      })
      .collect()
  }

  fn ref_path_to_type_name(ref_path: &str) -> Option<String> {
    SchemaRegistry::extract_ref_name(ref_path).map(|name| to_rust_type_name(&name))
  }

  fn find_variant_by_type<'a>(variants: &'a [VariantDef], type_name: &str) -> Option<&'a VariantDef> {
    variants
      .iter()
      .find(|v| v.unboxed_type_name().is_some_and(|name| name == type_name))
  }

  fn try_build_relaxed_enum(
    &self,
    name: &str,
    schema: &ObjectSchema,
    cache: Option<&mut SharedSchemaCache>,
  ) -> Option<Vec<RustType>> {
    let known_values = self.collect_relaxed_known_values(schema);
    if known_values.is_empty() {
      return None;
    }

    Some(self.build_relaxed_enum_types(name, schema, &known_values, cache))
  }

  fn collect_relaxed_known_values(&self, schema: &ObjectSchema) -> Vec<(String, Option<String>, bool)> {
    let mut seen_values = HashSet::new();
    let mut known_values = vec![];
    let mut has_freeform = false;

    for variant in &schema.any_of {
      let Ok(resolved) = variant.resolve(self.graph.spec()) else {
        continue;
      };

      if resolved.const_value.is_none() && resolved.enum_values.is_empty() && resolved.is_string() {
        has_freeform = true;
      }

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

      if resolved.is_string() {
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

    if has_freeform { known_values } else { vec![] }
  }

  fn build_relaxed_enum_types(
    &self,
    name: &str,
    schema: &ObjectSchema,
    known_values: &[(String, Option<String>, bool)],
    cache: Option<&mut SharedSchemaCache>,
  ) -> Vec<RustType> {
    let base_name = to_rust_type_name(name);

    let mut cache_key_values: Vec<String> = known_values.iter().map(|(v, _, _)| v.clone()).collect();
    cache_key_values.sort();

    let (known_enum_name, inner_enum_type) =
      self.resolve_cached_known_enum(&base_name, known_values, cache_key_values, cache);

    let methods = if self.no_helpers {
      vec![]
    } else {
      Self::build_known_value_constructors(&base_name, &known_enum_name, known_values)
    };

    let outer_enum = Self::build_relaxed_wrapper_enum(&base_name, &known_enum_name, schema, methods);

    let mut types = vec![];
    if let Some(ie) = inner_enum_type {
      types.push(ie);
    }
    types.push(outer_enum);
    types
  }

  fn resolve_cached_known_enum(
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
          let def = self.build_known_values_enum(&name, known_values);
          c.register_enum(cache_key, name.clone());
          c.mark_name_used(name.clone());
          (name, Some(def))
        }
      } else {
        let name = format!("{base_name}Known");
        let def = self.build_known_values_enum(&name, known_values);
        c.register_enum(cache_key, name.clone());
        c.mark_name_used(name.clone());
        (name, Some(def))
      }
    } else {
      let name = format!("{base_name}Known");
      (name.clone(), Some(self.build_known_values_enum(&name, known_values)))
    }
  }

  fn build_known_values_enum(&self, name: &str, values: &[(String, Option<String>, bool)]) -> RustType {
    let entries: Vec<EnumValueEntry> = values
      .iter()
      .map(|(value, description, deprecated)| EnumValueEntry {
        value: Value::String(value.clone()),
        docs: metadata::extract_docs(description.as_ref()),
        deprecated: *deprecated,
      })
      .collect();

    Self::build_enum_from_values(
      name,
      &entries,
      CollisionStrategy::Preserve,
      Documentation::from_lines(["Known values for the string enum."]),
      self.case_insensitive_enums,
    )
  }

  fn build_known_value_constructors(
    wrapper_enum_name: &str,
    known_type_name: &str,
    values: &[(String, Option<String>, bool)],
  ) -> Vec<EnumMethod> {
    let known_type = EnumToken::new(known_type_name);

    let variant_names: Vec<EnumVariantToken> = values
      .iter()
      .filter_map(|(value, _, _)| {
        VariantNameNormalizer::normalize(&Value::String(value.clone())).map(|n| EnumVariantToken::new(n.name))
      })
      .collect();

    let variant_name_strings: Vec<String> = variant_names.iter().map(std::string::ToString::to_string).collect();
    let method_names = derive_method_names(wrapper_enum_name, &variant_name_strings);

    let mut seen = BTreeSet::new();
    variant_names
      .into_iter()
      .zip(method_names)
      .zip(values.iter())
      .map(|((variant, base_name), (_, description, _))| {
        let method_name = ensure_unique(&base_name, &seen);
        seen.insert(method_name.clone());
        let docs = metadata::extract_docs(description.as_ref());
        EnumMethod::new(
          method_name,
          EnumMethodKind::KnownValueConstructor {
            known_type: known_type.clone(),
            known_variant: variant,
          },
          docs,
        )
      })
      .collect()
  }

  fn build_relaxed_wrapper_enum(
    name: &str,
    known_type_name: &str,
    schema: &ObjectSchema,
    methods: Vec<EnumMethod>,
  ) -> RustType {
    let variants = vec![
      VariantDef {
        name: EnumVariantToken::new("Known"),
        docs: Documentation::from_lines(["A known value."]),
        content: VariantContent::Tuple(vec![TypeRef::new(known_type_name)]),
        serde_attrs: vec![],
        deprecated: false,
      },
      VariantDef {
        name: EnumVariantToken::new("Other"),
        docs: Documentation::from_lines(["An unknown value."]),
        content: VariantContent::Tuple(vec![TypeRef::new("String")]),
        serde_attrs: vec![],
        deprecated: false,
      },
    ];

    RustType::Enum(EnumDef {
      name: EnumToken::new(name),
      docs: metadata::extract_docs(schema.description.as_ref()),
      variants,
      serde_attrs: vec![SerdeAttribute::Untagged],
      case_insensitive: false,
      methods,
      ..Default::default()
    })
  }
}

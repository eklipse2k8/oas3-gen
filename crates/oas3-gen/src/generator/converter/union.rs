use std::{
  collections::{BTreeMap, BTreeSet, HashSet},
  sync::Arc,
};

use anyhow::Context;
use oas3::spec::{ObjectOrReference, ObjectSchema};
use serde_json::Value;

use super::{
  CodegenConfig, ConversionOutput,
  cache::SharedSchemaCache,
  common::{SchemaExt, handle_inline_creation},
  struct_summaries::StructSummary,
  structs::StructConverter,
  type_resolver::TypeResolver,
};
use crate::generator::{
  ast::{
    Documentation, EnumDef, EnumMethod, EnumMethodKind, EnumToken, EnumVariantToken, MethodNameToken, RustPrimitive,
    RustType, SerdeAttribute, TypeRef, VariantContent, VariantDef,
  },
  converter::discriminator::try_build_discriminated_enum_from_variants,
  naming::{
    identifiers::{ensure_unique, to_rust_type_name},
    inference::{
      VariantNameNormalizer, derive_method_names, extract_enum_values, infer_union_variant_label, strip_common_affixes,
    },
  },
  schema_registry::{RefCollector, SchemaRegistry},
};

#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub(crate) enum UnionKind {
  OneOf,
  AnyOf,
}

#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub(crate) enum CollisionStrategy {
  Preserve,
  Deduplicate,
}

#[derive(Clone, Debug)]
pub(crate) struct EnumValueEntry {
  pub(crate) value: Value,
  pub(crate) docs: Documentation,
  pub(crate) deprecated: bool,
}

#[derive(Clone, Debug)]
struct UnionVariantSpec {
  variant_name: EnumVariantToken,
  resolved_schema: ObjectSchema,
  ref_name: Option<String>,
}

#[derive(Clone, Debug)]
pub(crate) struct UnionConverter {
  graph: Arc<SchemaRegistry>,
  type_resolver: TypeResolver,
  struct_converter: StructConverter,
  case_insensitive_enums: bool,
  no_helpers: bool,
}

impl UnionConverter {
  pub(crate) fn new(graph: &Arc<SchemaRegistry>, type_resolver: TypeResolver, config: &CodegenConfig) -> Self {
    let struct_converter = StructConverter::new(graph, config, None);
    Self {
      graph: graph.clone(),
      type_resolver,
      struct_converter,
      case_insensitive_enums: config.case_insensitive_enums(),
      no_helpers: config.no_helpers(),
    }
  }

  pub(crate) fn convert_union(
    &self,
    name: &str,
    schema: &ObjectSchema,
    kind: UnionKind,
    mut cache: Option<&mut SharedSchemaCache>,
  ) -> anyhow::Result<ConversionOutput<RustType>> {
    if kind == UnionKind::AnyOf
      && let Some(output) = self.try_build_relaxed_enum(name, schema, cache.as_deref_mut())
    {
      return Ok(output);
    }

    let output = self.collect_union_variants(name, schema, kind, cache.as_deref_mut())?;

    if let Some(c) = cache
      && let Some(values) = extract_enum_values(schema)
      && let RustType::Enum(e) = &output.result
    {
      c.register_enum(values, e.name.to_string());
    }

    Ok(output)
  }

  fn collect_union_variants(
    &self,
    name: &str,
    schema: &ObjectSchema,
    kind: UnionKind,
    mut cache: Option<&mut SharedSchemaCache>,
  ) -> anyhow::Result<ConversionOutput<RustType>> {
    let variants_src = match kind {
      UnionKind::OneOf => &schema.one_of,
      UnionKind::AnyOf => &schema.any_of,
    };

    let (mut variants, inline_types) = self.collect_union_variant_specs(variants_src)?.into_iter().try_fold(
      (vec![], vec![]),
      |(mut variants, mut inline_types), spec| {
        let output = self.build_union_variant(name, &spec, cache.as_deref_mut())?;
        variants.push(output.result);
        inline_types.extend(output.inline_types);
        anyhow::Ok((variants, inline_types))
      },
    )?;

    strip_common_affixes(&mut variants);

    let methods = if self.no_helpers {
      vec![]
    } else {
      self.build_constructors(&variants, &inline_types, name, cache)
    };

    let main_enum = Self::build_union_def(name, schema, variants, methods);

    Ok(ConversionOutput::with_inline_types(main_enum, inline_types))
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
        .context(format!("Schema resolution failed for union variant {i}"))?;

      if resolved.is_null() {
        continue;
      }

      let ref_name = RefCollector::parse_schema_ref(variant_ref).or_else(|| {
        if resolved.all_of.len() == 1 {
          RefCollector::parse_schema_ref(&resolved.all_of[0])
        } else {
          None
        }
      });

      let base_name = infer_union_variant_label(&resolved, ref_name.as_deref(), i);
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
  ) -> anyhow::Result<ConversionOutput<VariantDef>> {
    if let Some(ref schema_name) = spec.ref_name {
      Ok(self.build_ref_variant(schema_name, &spec.resolved_schema, spec.variant_name.clone()))
    } else {
      self.build_inline_variant(&spec.resolved_schema, enum_name, spec.variant_name.clone(), cache)
    }
  }

  fn build_ref_variant(
    &self,
    schema_name: &str,
    resolved_schema: &ObjectSchema,
    variant_name: EnumVariantToken,
  ) -> ConversionOutput<VariantDef> {
    let rust_type_name = to_rust_type_name(schema_name);

    // We never want optional variants since this builds untagged enums
    let type_ref = if self.graph.is_cyclic(schema_name) {
      TypeRef::new(&rust_type_name).unwrap_option().with_boxed()
    } else {
      TypeRef::new(&rust_type_name).unwrap_option()
    };

    ConversionOutput::new(
      VariantDef::builder()
        .name(variant_name)
        .docs(Documentation::from_optional(resolved_schema.description.as_ref()))
        .content(VariantContent::Tuple(vec![type_ref]))
        .deprecated(resolved_schema.deprecated.unwrap_or(false))
        .build(),
    )
  }

  fn build_inline_variant(
    &self,
    resolved_schema: &ObjectSchema,
    enum_name: &str,
    variant_name: EnumVariantToken,
    mut cache: Option<&mut SharedSchemaCache>,
  ) -> anyhow::Result<ConversionOutput<VariantDef>> {
    let resolved_schema = if resolved_schema.has_intersection() {
      self.graph.merge_inline(resolved_schema)?
    } else {
      resolved_schema.clone()
    };

    if let Some(output) = Self::build_const_variant(&resolved_schema, &variant_name)? {
      return Ok(output);
    }

    let variant_label = variant_name.to_string();

    let content_output = if resolved_schema.properties.is_empty() {
      if let Some(output) =
        self.build_array_content(enum_name, &variant_label, &resolved_schema, cache.as_deref_mut())?
      {
        output
      } else if let Some(output) =
        self.build_nested_union_content(enum_name, &variant_label, &resolved_schema, cache.as_deref_mut())?
      {
        output
      } else {
        self.build_primitive_content(&resolved_schema)?
      }
    } else {
      self.build_struct_content(enum_name, &variant_label, &resolved_schema, cache)?
    };

    Ok(ConversionOutput::with_inline_types(
      VariantDef::builder()
        .name(variant_name)
        .content(content_output.result)
        .docs(Documentation::from_optional(resolved_schema.description.as_ref()))
        .deprecated(resolved_schema.deprecated.unwrap_or(false))
        .build(),
      content_output.inline_types,
    ))
  }

  fn build_const_variant(
    resolved_schema: &ObjectSchema,
    variant_name: &EnumVariantToken,
  ) -> anyhow::Result<Option<ConversionOutput<VariantDef>>> {
    let Some(const_value) = &resolved_schema.const_value else {
      return Ok(None);
    };

    let normalized = VariantNameNormalizer::normalize(const_value)
      .ok_or_else(|| anyhow::anyhow!("Unsupported const value type: {const_value}"))?;

    let variant = VariantDef::builder()
      .name(variant_name.clone())
      .docs(Documentation::from_optional(resolved_schema.description.as_ref()))
      .content(VariantContent::Unit)
      .serde_attrs(vec![SerdeAttribute::Rename(normalized.rename_value)])
      .deprecated(resolved_schema.deprecated.unwrap_or(false))
      .build();

    Ok(Some(ConversionOutput::new(variant)))
  }

  fn build_array_content(
    &self,
    enum_name: &str,
    variant_label: &str,
    resolved_schema: &ObjectSchema,
    cache: Option<&mut SharedSchemaCache>,
  ) -> anyhow::Result<Option<ConversionOutput<VariantContent>>> {
    if !resolved_schema.is_array() {
      return Ok(None);
    }

    let conversion =
      self
        .type_resolver
        .resolve_array_with_inline_items(enum_name, variant_label, resolved_schema, cache)?;

    Ok(conversion.map(|c| ConversionOutput::with_inline_types(VariantContent::Tuple(vec![c.result]), c.inline_types)))
  }

  fn build_nested_union_content(
    &self,
    enum_name: &str,
    variant_label: &str,
    resolved_schema: &ObjectSchema,
    cache: Option<&mut SharedSchemaCache>,
  ) -> anyhow::Result<Option<ConversionOutput<VariantContent>>> {
    if !resolved_schema.has_union() {
      return Ok(None);
    }

    let uses_one_of = !resolved_schema.one_of.is_empty();
    let result =
      self
        .type_resolver
        .resolve_inline_union_type(enum_name, variant_label, resolved_schema, uses_one_of, cache)?;

    Ok(Some(ConversionOutput::with_inline_types(
      VariantContent::Tuple(vec![result.result]),
      result.inline_types,
    )))
  }

  fn build_struct_content(
    &self,
    enum_name: &str,
    variant_label: &str,
    resolved_schema: &ObjectSchema,
    cache: Option<&mut SharedSchemaCache>,
  ) -> anyhow::Result<ConversionOutput<VariantContent>> {
    let enum_name_converted = to_rust_type_name(enum_name);
    let struct_name_prefix = format!("{enum_name_converted}{variant_label}");

    let result = handle_inline_creation(
      resolved_schema,
      &struct_name_prefix,
      None,
      cache,
      |_| None,
      |name, cache| self.struct_converter.convert_struct(name, resolved_schema, None, cache),
    )?;

    Ok(ConversionOutput::with_inline_types(
      VariantContent::Tuple(vec![result.result]),
      result.inline_types,
    ))
  }

  fn build_primitive_content(
    &self,
    resolved_schema: &ObjectSchema,
  ) -> anyhow::Result<ConversionOutput<VariantContent>> {
    let type_ref = self.type_resolver.resolve_type(resolved_schema)?.unwrap_option();
    Ok(ConversionOutput::new(VariantContent::Tuple(vec![type_ref])))
  }

  fn build_union_def(
    name: &str,
    schema: &ObjectSchema,
    variants: Vec<VariantDef>,
    methods: Vec<EnumMethod>,
  ) -> RustType {
    if let Some(discriminated) = try_build_discriminated_enum_from_variants(name, schema, &variants, methods.clone()) {
      return discriminated;
    }

    RustType::Enum(
      EnumDef::builder()
        .name(EnumToken::from_raw(name))
        .docs(Documentation::from_optional(schema.description.as_ref()))
        .variants(variants)
        .serde_attrs(vec![SerdeAttribute::Untagged])
        .case_insensitive(false)
        .methods(methods)
        .build(),
    )
  }

  fn try_build_relaxed_enum(
    &self,
    name: &str,
    schema: &ObjectSchema,
    cache: Option<&mut SharedSchemaCache>,
  ) -> Option<ConversionOutput<RustType>> {
    let known_values = self.collect_relaxed_known_values(schema);
    if known_values.is_empty() {
      return None;
    }

    Some(self.build_relaxed_enum_types(name, schema, &known_values, cache))
  }

  fn collect_relaxed_known_values(&self, schema: &ObjectSchema) -> Vec<EnumValueEntry> {
    let mut seen_values = HashSet::new();
    let mut known_values = vec![];
    let mut has_freeform = false;

    for variant in &schema.any_of {
      let Ok(resolved) = variant.resolve(self.graph.spec()) else {
        continue;
      };

      if !resolved.has_const_value() && !resolved.has_enum_values() && resolved.is_string() {
        has_freeform = true;
      }

      let docs = Documentation::from_optional(resolved.description.as_ref());
      let deprecated = resolved.deprecated.unwrap_or(false);

      if let Some(const_val) = resolved.const_value.as_ref().and_then(|v| v.as_str()) {
        if seen_values.insert(const_val.to_string()) {
          known_values.push(EnumValueEntry {
            value: Value::String(const_val.to_string()),
            docs,
            deprecated,
          });
        }
        continue;
      }

      if resolved.is_string() {
        for enum_value in &resolved.enum_values {
          if let Some(str_val) = enum_value.as_str()
            && seen_values.insert(str_val.to_string())
          {
            known_values.push(EnumValueEntry {
              value: Value::String(str_val.to_string()),
              docs: docs.clone(),
              deprecated,
            });
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
    known_values: &[EnumValueEntry],
    cache: Option<&mut SharedSchemaCache>,
  ) -> ConversionOutput<RustType> {
    let base_name = to_rust_type_name(name);

    let mut cache_key_values: Vec<String> = known_values
      .iter()
      .filter_map(|e| e.value.as_str().map(String::from))
      .collect();
    cache_key_values.sort();

    let (known_enum_name, inner_enum_type) =
      self.resolve_cached_known_enum(&base_name, known_values, cache_key_values, cache);

    let methods = if self.no_helpers {
      vec![]
    } else {
      Self::build_known_value_constructors(&base_name, &known_enum_name, known_values)
    };

    let outer_enum = Self::build_relaxed_wrapper_enum(&base_name, &known_enum_name, schema, methods);
    let inline_types = inner_enum_type.into_iter().collect();

    ConversionOutput::with_inline_types(outer_enum, inline_types)
  }

  fn resolve_cached_known_enum(
    &self,
    base_name: &str,
    known_values: &[EnumValueEntry],
    cache_key: Vec<String>,
    cache: Option<&mut SharedSchemaCache>,
  ) -> (String, Option<RustType>) {
    let cached_state = cache.as_ref().and_then(|c| {
      c.get_enum_name(&cache_key)
        .map(|name| (name.clone(), c.is_enum_generated(&cache_key)))
    });

    if let Some((name, true)) = cached_state {
      return (name, None);
    }

    let name = cached_state.map_or_else(|| format!("{base_name}Known"), |(name, _)| name);

    let def = Self::build_enum_from_values(
      name.as_str(),
      known_values,
      CollisionStrategy::Preserve,
      Documentation::from_lines(["Known values for the string enum."]),
      self.case_insensitive_enums,
    );

    if let Some(c) = cache {
      c.register_enum(cache_key, name.clone());
      c.mark_name_used(name.clone());
    }

    (name, Some(def))
  }

  fn build_known_value_constructors(
    wrapper_enum_name: &str,
    known_type_name: &str,
    entries: &[EnumValueEntry],
  ) -> Vec<EnumMethod> {
    let known_type = EnumToken::new(known_type_name);

    let variant_names: Vec<EnumVariantToken> = entries
      .iter()
      .filter_map(|entry| VariantNameNormalizer::normalize(&entry.value).map(|n| EnumVariantToken::new(n.name)))
      .collect();

    let variant_name_strings: Vec<String> = variant_names.iter().map(std::string::ToString::to_string).collect();
    let method_names = derive_method_names(wrapper_enum_name, &variant_name_strings);

    let mut seen = BTreeSet::new();
    variant_names
      .into_iter()
      .zip(method_names)
      .zip(entries.iter())
      .map(|((variant, base_name), entry)| {
        let method_name = ensure_unique(&base_name, &seen);
        seen.insert(method_name.clone());
        EnumMethod::new(
          method_name,
          EnumMethodKind::KnownValueConstructor {
            known_type: known_type.clone(),
            known_variant: variant,
          },
          entry.docs.clone(),
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
      VariantDef::builder()
        .name(EnumVariantToken::new("Known"))
        .content(VariantContent::Tuple(vec![TypeRef::new(known_type_name)]))
        .build(),
      VariantDef::builder()
        .name(EnumVariantToken::new("Other"))
        .content(VariantContent::Tuple(vec![TypeRef::new(RustPrimitive::String)]))
        .build(),
    ];

    RustType::Enum(
      EnumDef::builder()
        .name(EnumToken::new(name))
        .docs(Documentation::from_optional(schema.description.as_ref()))
        .variants(variants)
        .serde_attrs(vec![SerdeAttribute::Untagged])
        .methods(methods)
        .build(),
    )
  }

  fn build_constructors(
    &self,
    variants: &[VariantDef],
    inline_types: &[RustType],
    enum_name: &str,
    mut cache: Option<&mut SharedSchemaCache>,
  ) -> Vec<EnumMethod> {
    let enum_name = to_rust_type_name(enum_name);

    let mut summary_cache: BTreeMap<String, StructSummary> = inline_types
      .iter()
      .filter_map(|t| match t {
        RustType::Struct(s) => Some((s.name.to_string(), StructSummary::from(s))),
        _ => None,
      })
      .collect();

    let mut eligible = vec![];

    for variant in variants {
      let Some(type_ref) = variant.single_wrapped_type() else {
        continue;
      };

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

        EnumMethod::new(MethodNameToken::from_raw(&method_name), kind, docs)
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

  fn resolve_struct_summary(
    &self,
    type_ref: &TypeRef,
    cache: Option<&mut SharedSchemaCache>,
    summary_cache: &mut BTreeMap<String, StructSummary>,
  ) -> Option<StructSummary> {
    let base_name = type_ref.unboxed_base_type_name();

    if let Some(summary) = summary_cache.get(&base_name) {
      return Some(summary.clone());
    }

    let has_summary = cache
      .as_ref()
      .is_some_and(|c| c.get_struct_summary(&base_name).is_some());

    if has_summary {
      let summary = cache.as_ref().unwrap().get_struct_summary(&base_name).unwrap().clone();
      summary_cache.insert(base_name, summary.clone());
      return Some(summary);
    }

    let schema = self.graph.get(&base_name)?;
    if !schema.is_object() && schema.properties.is_empty() {
      return None;
    }

    let struct_result = self
      .struct_converter
      .convert_struct(&base_name, schema, None, cache)
      .ok()?;

    if let RustType::Struct(s) = struct_result.result {
      let summary = StructSummary::from(&s);
      summary_cache.insert(base_name, summary.clone());
      Some(summary)
    } else {
      None
    }
  }

  pub(crate) fn build_enum_from_values(
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
          variants[existing_idx].add_alias(normalized.rename_value);
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
}

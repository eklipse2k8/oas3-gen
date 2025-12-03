use std::{
  collections::{BTreeMap, BTreeSet},
  sync::Arc,
};

use anyhow::Context;
use oas3::spec::ObjectSchema;

use super::{
  CodegenConfig, cache::SharedSchemaCache, common::SchemaExt, field_optionality::FieldOptionalityPolicy, metadata,
  string_enum_optimizer::StringEnumOptimizer, structs::StructConverter, type_resolver::TypeResolver,
};
use crate::generator::{
  ast::{
    DiscriminatedEnumDef, DiscriminatedVariant, EnumDef, EnumMethod, EnumMethodKind, EnumToken, EnumVariantToken,
    RustType, SerdeAttribute, StructDef, TypeRef, VariantContent, VariantDef,
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
  /// Creates a new EnumConverter instance.
  pub(crate) fn new(graph: &Arc<SchemaRegistry>, type_resolver: TypeResolver, config: CodegenConfig) -> Self {
    let struct_converter = StructConverter::new(graph, config, None, FieldOptionalityPolicy::standard());
    Self {
      graph: graph.clone(),
      type_resolver,
      struct_converter,
      preserve_case_variants: config.preserve_case_variants,
      case_insensitive_enums: config.case_insensitive_enums,
      no_helpers: config.no_helpers,
    }
  }

  /// Converts a simple enum (list of values) into a Rust Enum.
  pub(crate) fn convert_simple_enum(
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

    let enum_def = self.build_simple_enum(name, schema, strategy);

    if let (Some(c), RustType::Enum(e)) = (cache, &enum_def) {
      c.register_enum(enum_values, e.name.to_string());
      c.mark_name_used(e.name.to_string());
    }

    Some(enum_def)
  }

  /// Converts a union (oneOf/anyOf) into a Rust Enum.
  pub(crate) fn convert_union_enum(
    &self,
    name: &str,
    schema: &ObjectSchema,
    kind: UnionKind,
    mut cache: Option<&mut SharedSchemaCache>,
  ) -> anyhow::Result<Vec<RustType>> {
    if kind == UnionKind::AnyOf {
      let optimizer = StringEnumOptimizer::new(&self.graph, self.case_insensitive_enums);
      if let Some(result) = optimizer.try_convert(name, schema, cache.as_deref_mut()) {
        return Ok(result);
      }
    }

    let result = self.process_union(name, schema, kind, cache.as_deref_mut())?;

    if let Some(c) = cache
      && let Some(values) = extract_enum_values(schema)
      && let Some(RustType::Enum(e)) = result.last()
    {
      c.register_enum(values, e.name.to_string());
    }

    Ok(result)
  }

  fn build_simple_enum(&self, name: &str, schema: &ObjectSchema, strategy: CollisionStrategy) -> RustType {
    let mut variants: Vec<VariantDef> = vec![];
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
          let unique_name = format!("{}{i}", normalized.name);
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
      name: EnumToken::from_raw(name),
      docs: metadata::extract_docs(schema.description.as_ref()),
      variants,
      case_insensitive: self.case_insensitive_enums,
      ..Default::default()
    })
  }

  fn push_variant(variants: &mut Vec<VariantDef>, name: impl Into<EnumVariantToken>, rename: &str) {
    variants.push(VariantDef {
      name: name.into(),
      content: VariantContent::Unit,
      serde_attrs: vec![SerdeAttribute::Rename(rename.to_string())],
      deprecated: false,
      ..Default::default()
    });
  }

  fn process_union(
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

    let mut inline_types = vec![];
    let mut variants = vec![];
    let mut seen_names = BTreeSet::new();

    for (i, variant_ref) in variants_src.iter().enumerate() {
      let resolved = variant_ref
        .resolve(self.graph.spec())
        .with_context(|| format!("Schema resolution failed for union variant {i}"))?;

      if resolved.is_null() {
        continue;
      }

      let ref_name_opt = ReferenceExtractor::extract_ref_name_from_obj_ref(variant_ref).or_else(|| {
        if resolved.all_of.len() == 1 {
          ReferenceExtractor::extract_ref_name_from_obj_ref(&resolved.all_of[0])
        } else {
          None
        }
      });

      let (variant, mut generated) = if let Some(schema_name) = ref_name_opt {
        self.create_ref_variant(&schema_name, &resolved, &mut seen_names)
      } else {
        self.create_inline_variant(i, &resolved, name, &mut seen_names, cache.as_deref_mut())?
      };

      variants.push(variant);
      inline_types.append(&mut generated);
    }

    strip_common_affixes(&mut variants);

    let methods = if self.no_helpers {
      vec![]
    } else {
      self.generate_methods(&variants, &inline_types, name, cache)
    };

    let main_enum = Self::build_union_enum_def(name, schema, kind, variants, methods);
    inline_types.push(main_enum);

    Ok(inline_types)
  }

  /// Generates helper methods for creating enum variants with default or single-parameter constructors.
  fn generate_methods(
    &self,
    variants: &[VariantDef],
    inline_types: &[RustType],
    enum_name: &str,
    mut cache: Option<&mut SharedSchemaCache>,
  ) -> Vec<EnumMethod> {
    let enum_name = to_rust_type_name(enum_name);
    let struct_map: BTreeMap<_, _> = inline_types
      .iter()
      .filter_map(|t| match t {
        RustType::Struct(s) => Some((s.name.to_string(), s)),
        _ => None,
      })
      .collect();

    let eligible: Vec<_> = variants
      .iter()
      .filter_map(|v| {
        let type_ref = v.single_wrapped_type()?;
        let method_kind = self.get_method_kind_for_type(type_ref, &v.name, &struct_map, cache.as_deref_mut())?;
        Some((v.name.clone(), method_kind))
      })
      .collect();

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
        EnumMethod::new(method_name, &variant_name, kind)
      })
      .collect()
  }

  fn get_method_kind_for_type(
    &self,
    type_ref: &TypeRef,
    variant_name: &EnumVariantToken,
    struct_map: &BTreeMap<String, &StructDef>,
    cache: Option<&mut SharedSchemaCache>,
  ) -> Option<EnumMethodKind> {
    let base_name = type_ref.unboxed_base_type_name();
    let struct_def = if let Some(&s) = struct_map.get(&base_name) {
      Some(s.clone())
    } else {
      self.lookup_struct_def(type_ref, cache)
    };

    let s = struct_def.as_ref()?;
    if !s.has_default() || type_ref.is_array {
      return None;
    }

    let required: Vec<_> = s.required_fields().collect();
    match required.len() {
      0 => Some(EnumMethodKind::SimpleConstructor {
        variant_name: variant_name.clone(),
        wrapped_type: type_ref.clone(),
      }),
      1 => Some(EnumMethodKind::ParameterizedConstructor {
        variant_name: variant_name.clone(),
        wrapped_type: type_ref.clone(),
        param_name: required[0].name.to_string(),
        param_type: required[0].rust_type.clone(),
      }),
      _ => None,
    }
  }

  fn lookup_struct_def(&self, type_ref: &TypeRef, cache: Option<&mut SharedSchemaCache>) -> Option<StructDef> {
    let schema_name = type_ref.unboxed_base_type_name();
    let schema = self.graph.get_schema(&schema_name)?;

    if !schema.is_object() && schema.properties.is_empty() {
      return None;
    }

    let struct_result = self
      .struct_converter
      .convert_struct(&schema_name, schema, None, cache)
      .ok()?;
    match struct_result.result {
      RustType::Struct(s) => Some(s),
      _ => None,
    }
  }

  fn create_ref_variant(
    &self,
    schema_name: &str,
    resolved_schema: &ObjectSchema,
    seen_names: &mut BTreeSet<String>,
  ) -> (VariantDef, Vec<RustType>) {
    let rust_type_name = to_rust_type_name(schema_name);
    let mut type_ref = TypeRef::new(&rust_type_name);

    if self.graph.is_cyclic(schema_name) {
      type_ref = type_ref.with_boxed();
    }

    let variant_name = ensure_unique(&rust_type_name, seen_names);

    let variant = VariantDef {
      name: EnumVariantToken::from(variant_name),
      docs: metadata::extract_docs(resolved_schema.description.as_ref()),
      content: VariantContent::Tuple(vec![type_ref]),
      deprecated: resolved_schema.deprecated.unwrap_or(false),
      ..Default::default()
    };

    (variant, vec![])
  }

  fn create_inline_variant(
    &self,
    index: usize,
    resolved_schema: &ObjectSchema,
    enum_name: &str,
    seen_names: &mut BTreeSet<String>,
    mut cache: Option<&mut SharedSchemaCache>,
  ) -> anyhow::Result<(VariantDef, Vec<RustType>)> {
    let mut resolved_schema_merged = resolved_schema.clone();
    if !resolved_schema.all_of.is_empty() {
      resolved_schema_merged = self.type_resolver.merge_all_of_schema(resolved_schema)?;
    }
    let resolved_schema = &resolved_schema_merged;

    if let Some(const_value) = &resolved_schema.const_value {
      let normalized = VariantNameNormalizer::normalize(const_value)
        .ok_or_else(|| anyhow::anyhow!("Unsupported const value type: {const_value}"))?;

      let variant_name = ensure_unique(&normalized.name, seen_names);

      let variant = VariantDef {
        name: EnumVariantToken::from(variant_name),
        docs: metadata::extract_docs(resolved_schema.description.as_ref()),
        content: VariantContent::Unit,
        serde_attrs: vec![SerdeAttribute::Rename(normalized.rename_value)],
        deprecated: resolved_schema.deprecated.unwrap_or(false),
      };

      return Ok((variant, vec![]));
    }

    let base_name = resolved_schema
      .title
      .as_ref()
      .map_or_else(|| infer_variant_name(resolved_schema, index), |t| to_rust_type_name(t));
    let variant_name = ensure_unique(&base_name, seen_names);

    let (content, generated_types) = if resolved_schema.properties.is_empty() {
      let mut array_conversion = None;
      if resolved_schema.is_array() {
        array_conversion =
          self
            .type_resolver
            .resolve_nullable_array_union(&variant_name, resolved_schema, cache.as_deref_mut())?;
      }

      if let Some(conversion) = array_conversion {
        (VariantContent::Tuple(vec![conversion.result]), conversion.inline_types)
      } else if !resolved_schema.one_of.is_empty() || !resolved_schema.any_of.is_empty() {
        let uses_one_of = !resolved_schema.one_of.is_empty();
        let result = self.type_resolver.resolve_inline_union_type(
          enum_name,
          &variant_name,
          resolved_schema,
          uses_one_of,
          cache,
        )?;
        (VariantContent::Tuple(vec![result.result]), result.inline_types)
      } else {
        let type_ref = self.type_resolver.resolve_type(resolved_schema)?;
        (VariantContent::Tuple(vec![type_ref]), vec![])
      }
    } else {
      let struct_name_prefix = format!("{enum_name}{variant_name}");
      let result = self
        .struct_converter
        .convert_struct(&struct_name_prefix, resolved_schema, None, cache)?;
      let (struct_def, mut inline_types) = (result.result, result.inline_types);

      let struct_name = match &struct_def {
        RustType::Struct(s) => s.name.clone(),
        _ => unreachable!("convert_struct must return a Struct"),
      };

      inline_types.push(struct_def);
      (VariantContent::Tuple(vec![TypeRef::new(struct_name)]), inline_types)
    };

    let variant = VariantDef {
      name: EnumVariantToken::from(variant_name),
      docs: metadata::extract_docs(resolved_schema.description.as_ref()),
      content,
      serde_attrs: vec![],
      deprecated: resolved_schema.deprecated.unwrap_or(false),
    };

    Ok((variant, generated_types))
  }

  fn build_union_enum_def(
    name: &str,
    schema: &ObjectSchema,
    _kind: UnionKind,
    variants: Vec<VariantDef>,
    methods: Vec<EnumMethod>,
  ) -> RustType {
    if let Some(discriminator) = &schema.discriminator
      && let Some(mapping) = &discriminator.mapping
      && Self::all_variants_are_refs(&variants, mapping)
    {
      let disc_variants = Self::build_discriminated_variants(&variants, mapping);
      return RustType::DiscriminatedEnum(DiscriminatedEnumDef {
        name: EnumToken::from_raw(name),
        docs: metadata::extract_docs(schema.description.as_ref()),
        discriminator_field: discriminator.property_name.clone(),
        variants: disc_variants,
        ..Default::default()
      });
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
    mapping
      .iter()
      .filter_map(|(disc_value, ref_path)| {
        let expected_type = Self::ref_path_to_type_name(ref_path)?;
        let variant = Self::find_variant_by_type(variants, &expected_type)?;
        let type_ref = variant.single_wrapped_type()?;

        Some(DiscriminatedVariant {
          discriminator_value: disc_value.clone(),
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
}

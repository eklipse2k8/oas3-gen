use std::{
  collections::{BTreeMap, BTreeSet},
  sync::Arc,
};

use anyhow::Context;
use oas3::spec::{ObjectSchema, SchemaType, SchemaTypeSet};

use super::{
  CodegenConfig, cache::SharedSchemaCache, field_optionality::FieldOptionalityPolicy, metadata,
  string_enum_optimizer::StringEnumOptimizer, structs::StructConverter, type_resolver::TypeResolver,
};
use crate::generator::{
  ast::{
    DeriveTrait, EnumDef, EnumMethod, EnumMethodKind, EnumToken, EnumVariantToken, FieldDef, RustType, SerdeAttribute,
    TypeRef, VariantContent, VariantDef, default_enum_derives,
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

struct EligibleVariant {
  variant_name: EnumVariantToken,
  type_name: String,
  docs: Vec<String>,
  first_required_field: Option<(String, String)>,
  has_required_fields: bool,
}

impl EnumConverter {
  /// Creates a new EnumConverter instance.
  pub(crate) fn new(graph: &Arc<SchemaRegistry>, type_resolver: TypeResolver, config: CodegenConfig) -> Self {
    let struct_converter = StructConverter::new(graph.clone(), config, None, FieldOptionalityPolicy::standard());
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

    let discriminator_map = if kind == UnionKind::OneOf {
      schema
        .discriminator
        .as_ref()
        .and_then(|d| d.mapping.as_ref())
        .map(|mapping| {
          mapping
            .iter()
            .filter_map(|(val, ref_path)| SchemaRegistry::extract_ref_name(ref_path).map(|name| (name, val.clone())))
            .collect()
        })
        .unwrap_or_default()
    } else {
      BTreeMap::new()
    };

    let result = self.process_union(name, schema, kind, &discriminator_map, cache.as_deref_mut())?;

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
      name: EnumToken::from_raw(name),
      docs: metadata::extract_docs(schema.description.as_ref()),
      variants,
      discriminator: None,
      derives: default_enum_derives(true),
      serde_attrs: vec![],
      outer_attrs: vec![],
      case_insensitive: self.case_insensitive_enums,
      methods: vec![],
    })
  }

  fn push_variant(variants: &mut Vec<VariantDef>, name: impl Into<EnumVariantToken>, rename: &str) {
    variants.push(VariantDef {
      name: name.into(),
      docs: vec![],
      content: VariantContent::Unit,
      serde_attrs: vec![SerdeAttribute::Rename(rename.to_string())],
      deprecated: false,
    });
  }

  fn process_union(
    &self,
    name: &str,
    schema: &ObjectSchema,
    kind: UnionKind,
    discriminator_map: &BTreeMap<String, String>,
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

      if resolved.schema_type == Some(SchemaTypeSet::Single(SchemaType::Null)) {
        continue;
      }

      let (variant, mut generated) =
        if let Some(schema_name) = ReferenceExtractor::extract_ref_name_from_obj_ref(variant_ref) {
          self.create_ref_variant(&schema_name, &resolved, discriminator_map, &mut seen_names)
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
      self.generate_methods(&variants, &inline_types, name)
    };

    let main_enum = Self::build_union_enum_def(name, schema, kind, variants, methods);
    inline_types.push(main_enum);

    Ok(inline_types)
  }

  /// Generates helper methods for creating enum variants with default or single-parameter constructors.
  ///
  /// For each variant wrapping a struct with Default derive:
  /// - If all fields are optional: generates simple constructor (e.g., `enum.variant_name()`)
  /// - If exactly one field is required: generates parameterized constructor (e.g., `enum.variant_name(value)`)
  /// - If multiple fields are required: skips method generation (too complex for helpers)
  fn generate_methods(&self, variants: &[VariantDef], inline_types: &[RustType], enum_name: &str) -> Vec<EnumMethod> {
    let enum_name = to_rust_type_name(enum_name);

    let struct_map: BTreeMap<_, _> = inline_types
      .iter()
      .filter_map(|t| match t {
        RustType::Struct(s) => Some((s.name.to_string(), s)),
        _ => None,
      })
      .collect();

    let eligible_variants: Vec<EligibleVariant> = variants
      .iter()
      .filter_map(|variant| {
        let VariantContent::Tuple(types) = &variant.content else {
          return None;
        };

        if types.len() != 1 {
          return None;
        }

        let type_ref = &types[0];
        let type_name = type_ref.to_rust_type();

        let struct_info = if let Some(&struct_def) = struct_map.get(&type_name) {
          Some((
            struct_def.derives.contains(&DeriveTrait::Default),
            struct_def.fields.clone(),
            struct_def.docs.clone(),
          ))
        } else {
          self.try_analyze_referenced_struct(&type_name)
        };

        let (has_default, fields, docs) = struct_info?;

        if !has_default {
          return None;
        }

        let required_fields: Vec<_> = fields
          .iter()
          .filter(|f| f.default_value.is_none() && !f.rust_type.nullable)
          .collect();

        if required_fields.len() > 1 {
          return None;
        }

        let first_required_field = if required_fields.len() == 1 {
          let field = required_fields[0];
          Some((field.name.clone(), field.rust_type.to_rust_type()))
        } else {
          None
        };

        Some(EligibleVariant {
          variant_name: variant.name.clone(),
          type_name,
          docs,
          first_required_field,
          has_required_fields: !required_fields.is_empty(),
        })
      })
      .collect();

    if eligible_variants.is_empty() {
      return vec![];
    }

    let variant_names: Vec<String> = eligible_variants.iter().map(|v| v.variant_name.to_string()).collect();
    let derived_names = derive_method_names(&enum_name, &variant_names);

    let mut seen_names = BTreeSet::new();
    eligible_variants
      .into_iter()
      .zip(derived_names)
      .map(|(variant_info, base_method_name)| {
        let method_name = ensure_unique(&base_method_name, &seen_names);
        seen_names.insert(method_name.clone());

        let method_docs = Self::generate_method_docs(
          &variant_info.variant_name.to_string(),
          &variant_info.docs,
          !variant_info.has_required_fields,
          variant_info
            .first_required_field
            .as_ref()
            .map(|(name, _)| name.as_str()),
        );

        if let Some((param_name, param_type)) = variant_info.first_required_field {
          EnumMethod {
            name: method_name,
            docs: method_docs,
            kind: EnumMethodKind::ParameterizedConstructor {
              variant_name: variant_info.variant_name.to_string(),
              wrapped_type: variant_info.type_name,
              param_name,
              param_type,
            },
          }
        } else {
          EnumMethod {
            name: method_name,
            docs: method_docs,
            kind: EnumMethodKind::SimpleConstructor {
              variant_name: variant_info.variant_name.to_string(),
              wrapped_type: variant_info.type_name,
            },
          }
        }
      })
      .collect()
  }

  /// Analyzes a referenced struct type to extract its Default derive status, fields, and documentation.
  ///
  /// This is used for generating helper methods when a variant wraps a referenced type (not inline).
  /// Returns None if the type is not a struct or cannot be converted.
  fn try_analyze_referenced_struct(&self, type_name: &str) -> Option<(bool, Vec<FieldDef>, Vec<String>)> {
    let schema_name = type_name.trim_start_matches("Box<").trim_end_matches('>');
    let schema = self.graph.get_schema(schema_name)?;

    if schema.schema_type != Some(oas3::spec::SchemaTypeSet::Single(oas3::spec::SchemaType::Object))
      && schema.properties.is_empty()
    {
      return None;
    }

    let struct_result = self
      .struct_converter
      .convert_struct(schema_name, schema, None, None)
      .ok()?;

    match &struct_result.result {
      RustType::Struct(s) => Some((
        s.derives.contains(&DeriveTrait::Default),
        s.fields.clone(),
        s.docs.clone(),
      )),
      _ => None,
    }
  }

  fn generate_method_docs(
    variant_name: &str,
    struct_docs: &[String],
    is_simple: bool,
    param_name: Option<&str>,
  ) -> Vec<String> {
    if is_simple {
      vec![format!("Creates a `{variant_name}` variant with default values.")]
    } else if let Some(param) = param_name {
      vec![format!(
        "Creates a `{variant_name}` variant with the specified `{param}`."
      )]
    } else {
      struct_docs.to_vec()
    }
  }

  fn create_ref_variant(
    &self,
    schema_name: &str,
    resolved_schema: &ObjectSchema,
    discriminator_map: &BTreeMap<String, String>,
    seen_names: &mut BTreeSet<String>,
  ) -> (VariantDef, Vec<RustType>) {
    let rust_type_name = to_rust_type_name(schema_name);
    let mut type_ref = TypeRef::new(&rust_type_name);

    if self.graph.is_cyclic(schema_name) {
      type_ref = type_ref.with_boxed();
    }

    let variant_name = ensure_unique(&rust_type_name, seen_names);

    let mut serde_attrs = vec![];
    if let Some(disc_value) = discriminator_map.get(schema_name) {
      serde_attrs.push(SerdeAttribute::Rename(disc_value.clone()));
    }

    let variant = VariantDef {
      name: EnumVariantToken::from(variant_name),
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
    enum_name: &str,
    seen_names: &mut BTreeSet<String>,
    cache: Option<&mut SharedSchemaCache>,
  ) -> anyhow::Result<(VariantDef, Vec<RustType>)> {
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
      let type_ref = self.type_resolver.schema_to_type_ref(resolved_schema)?;
      (VariantContent::Tuple(vec![type_ref]), vec![])
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
    kind: UnionKind,
    variants: Vec<VariantDef>,
    methods: Vec<EnumMethod>,
  ) -> RustType {
    let has_discriminator = schema.discriminator.is_some();

    let (serde_attrs, derives) = if kind == UnionKind::AnyOf && !has_discriminator {
      (vec![SerdeAttribute::Untagged], default_enum_derives(false))
    } else {
      (vec![], default_enum_derives(false))
    };

    RustType::Enum(EnumDef {
      name: EnumToken::from_raw(name),
      docs: metadata::extract_docs(schema.description.as_ref()),
      variants,
      discriminator: schema.discriminator.as_ref().map(|d| d.property_name.clone()),
      derives,
      serde_attrs,
      outer_attrs: vec![],
      case_insensitive: false,
      methods,
    })
  }
}

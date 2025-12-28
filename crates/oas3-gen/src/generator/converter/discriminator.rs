use std::{
  collections::{BTreeMap, BTreeSet},
  sync::Arc,
};

use oas3::spec::ObjectSchema;
use string_cache::DefaultAtom;

use super::SchemaExt;
use crate::generator::{
  ast::{
    DiscriminatedEnumDef, DiscriminatedEnumDefBuilder, DiscriminatedVariant, Documentation, EnumMethod, EnumToken,
    RustType, TypeRef, VariantDef,
  },
  naming::identifiers::to_rust_type_name,
  schema_registry::SchemaRegistry,
};

#[derive(Debug, Clone)]
pub(crate) struct DiscriminatorInfo {
  pub value: Option<DefaultAtom>,
  pub is_base: bool,
  pub has_enum: bool,
}

impl DiscriminatorInfo {
  pub fn new(
    prop_name: &str,
    parent_schema: &ObjectSchema,
    prop_schema: &ObjectSchema,
    discriminator_mapping: Option<&(String, String)>,
  ) -> Option<Self> {
    let value = discriminator_mapping
      .filter(|(prop, _)| prop == prop_name)
      .map(|(_, v)| DefaultAtom::from(v.as_str()));

    let is_base_discriminator = parent_schema
      .discriminator
      .as_ref()
      .is_some_and(|d| d.property_name == prop_name);

    let is_child_discriminator = value.is_some();

    if !is_child_discriminator && !is_base_discriminator {
      return None;
    }

    Some(Self {
      value,
      is_base: is_base_discriminator && !is_child_discriminator,
      has_enum: prop_schema.has_enum_values(),
    })
  }

  pub fn should_hide(&self) -> bool {
    !self.has_enum && (self.value.is_some() || self.is_base)
  }
}

pub(crate) struct DiscriminatorHandler<'a> {
  graph: &'a Arc<SchemaRegistry>,
  reachable_schemas: Option<&'a Arc<BTreeSet<String>>>,
}

impl<'a> DiscriminatorHandler<'a> {
  pub(crate) fn new(graph: &'a Arc<SchemaRegistry>, reachable_schemas: Option<&'a Arc<BTreeSet<String>>>) -> Self {
    Self {
      graph,
      reachable_schemas,
    }
  }

  pub(crate) fn detect_discriminated_parent(&self, schema_name: &str) -> Option<(String, String, String)> {
    self.graph.get_discriminator_parent(schema_name).cloned()
  }

  pub(crate) fn create_discriminated_enum(
    &self,
    base_name: &str,
    schema: &ObjectSchema,
    base_struct_name: &str,
  ) -> anyhow::Result<RustType> {
    let Some(discriminator_field) = schema.discriminator.as_ref().map(|d| &d.property_name) else {
      anyhow::bail!("Failed to find discriminator property for schema '{base_name}'");
    };

    let children = self.extract_discriminator_children(schema);
    let enum_name = to_rust_type_name(base_name);

    let variants: Vec<_> = children
      .into_iter()
      .map(|(disc_values, child_schema_name)| {
        let child_type_name = to_rust_type_name(&child_schema_name);
        let variant_name = child_type_name
          .strip_prefix(&enum_name)
          .filter(|s| !s.is_empty())
          .map(|s| {
            let mut chars = s.chars();
            match chars.next() {
              None => String::new(),
              Some(first) => first.to_uppercase().chain(chars).collect(),
            }
          })
          .unwrap_or(child_type_name.clone());

        DiscriminatedVariant {
          discriminator_values: disc_values,
          variant_name,
          type_name: TypeRef::new(child_type_name).with_boxed(),
        }
      })
      .collect();

    let base_variant_name = to_rust_type_name(base_name.split('.').next_back().unwrap_or(base_name));
    let fallback = Some(DiscriminatedVariant {
      discriminator_values: vec![],
      variant_name: base_variant_name,
      type_name: TypeRef::new(base_struct_name).with_boxed(),
    });

    Ok(RustType::DiscriminatedEnum(
      DiscriminatedEnumDefBuilder::default()
        .name(enum_name)
        .docs(Documentation::from_optional(schema.description.as_ref()))
        .discriminator_field(discriminator_field.clone())
        .variants(variants)
        .fallback(fallback)
        .build()?,
    ))
  }

  /// Extracts child schemas from a discriminator mapping.
  ///
  /// Returns `(discriminator_values, schema_name)` pairs grouped by schema name.
  /// Multiple discriminator values that map to the same schema are collected together.
  /// Results are in alphabetical order by schema name for deterministic output.
  pub(crate) fn extract_discriminator_children(&self, schema: &ObjectSchema) -> Vec<(Vec<String>, String)> {
    let Some(mapping) = schema.discriminator.as_ref().and_then(|d| d.mapping.as_ref()) else {
      return vec![];
    };

    let mut schema_to_disc_values: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for (disc_value, ref_path) in mapping {
      let Some(schema_name) = SchemaRegistry::extract_ref_name(ref_path) else {
        continue;
      };

      if let Some(filter) = self.reachable_schemas
        && !filter.contains(&schema_name)
      {
        continue;
      }

      schema_to_disc_values
        .entry(schema_name)
        .or_default()
        .push(disc_value.clone());
    }

    schema_to_disc_values
      .into_iter()
      .map(|(name, values)| (values, name))
      .collect()
  }
}

pub(crate) fn try_build_discriminated_enum_from_variants(
  name: &str,
  schema: &ObjectSchema,
  variants: &[VariantDef],
  methods: Vec<EnumMethod>,
) -> Option<RustType> {
  let discriminator = schema.discriminator.as_ref()?;
  let mapping = discriminator.mapping.as_ref()?;

  if !all_variants_match_mapping(variants, mapping) {
    return None;
  }

  let disc_variants = build_discriminated_variants_from_mapping(variants, mapping);
  Some(RustType::DiscriminatedEnum(DiscriminatedEnumDef {
    name: EnumToken::from_raw(name),
    docs: Documentation::from_optional(schema.description.as_ref()),
    discriminator_field: discriminator.property_name.clone(),
    variants: disc_variants,
    methods,
    ..Default::default()
  }))
}

fn all_variants_match_mapping(variants: &[VariantDef], mapping: &BTreeMap<String, String>) -> bool {
  if variants.is_empty() || mapping.is_empty() {
    return false;
  }

  let variant_types: BTreeSet<String> = variants.iter().filter_map(VariantDef::unboxed_type_name).collect();

  mapping
    .values()
    .filter_map(|ref_path| SchemaRegistry::extract_ref_name(ref_path).map(|name| to_rust_type_name(&name)))
    .all(|type_name| variant_types.contains(&type_name))
}

fn build_discriminated_variants_from_mapping(
  variants: &[VariantDef],
  mapping: &BTreeMap<String, String>,
) -> Vec<DiscriminatedVariant> {
  let mut type_to_disc_values: BTreeMap<String, Vec<String>> = BTreeMap::new();
  for (disc_value, ref_path) in mapping {
    if let Some(expected_type) = SchemaRegistry::extract_ref_name(ref_path).map(|name| to_rust_type_name(&name)) {
      type_to_disc_values
        .entry(expected_type)
        .or_default()
        .push(disc_value.clone());
    }
  }

  type_to_disc_values
    .into_iter()
    .filter_map(|(expected_type, disc_values)| {
      let variant = variants
        .iter()
        .find(|v| v.unboxed_type_name().is_some_and(|name| name == expected_type))?;
      let type_ref = variant.single_wrapped_type()?;

      Some(DiscriminatedVariant {
        discriminator_values: disc_values,
        variant_name: variant.name.to_string(),
        type_name: type_ref.clone(),
      })
    })
    .collect()
}

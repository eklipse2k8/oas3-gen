use std::{
  collections::{BTreeMap, BTreeSet},
  rc::Rc,
};

use oas3::spec::ObjectSchema;
use string_cache::DefaultAtom;

use super::SchemaExt;
use crate::generator::{
  ast::{DiscriminatedVariant, Documentation, EnumMethod, EnumToken, EnumVariantToken, RustType, TypeRef, VariantDef},
  converter::ConverterContext,
  naming::identifiers::{split_pascal_case, strip_parent_prefix, to_rust_type_name},
  schema_registry::{ParentInfo, SchemaRegistry},
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

#[derive(Debug, Clone)]
pub(crate) struct DiscriminatorConverter {
  context: Rc<ConverterContext>,
}

impl DiscriminatorConverter {
  pub(crate) fn new(context: Rc<ConverterContext>) -> Self {
    Self { context }
  }

  pub(crate) fn detect_discriminated_parent(&self, schema_name: &str) -> Option<&ParentInfo> {
    self.context.graph().parent(schema_name)
  }

  /// Builds a discriminated enum from a base schema with discriminator mappings.
  ///
  /// Returns an error if the schema lacks a discriminator property.
  pub(crate) fn build_enum(&self, name: &str, schema: &ObjectSchema, fallback_type: &str) -> anyhow::Result<RustType> {
    let tag_field = schema
      .discriminator
      .as_ref()
      .map(|d| &d.property_name)
      .ok_or_else(|| anyhow::anyhow!("missing discriminator property for schema '{name}'"))?;

    let enum_name = to_rust_type_name(name);

    let to_variant = |(schema_name, tags): (String, Vec<String>)| {
      let type_name = to_rust_type_name(&schema_name);
      let variant_name = strip_parent_prefix(&enum_name, &type_name);

      DiscriminatedVariant::builder()
        .variant_name(EnumVariantToken::new(variant_name))
        .discriminator_values(tags)
        .type_name(TypeRef::new(type_name).with_boxed())
        .build()
    };

    let variants: Vec<_> = self
      .discriminator_mappings(schema)
      .into_iter()
      .map(to_variant)
      .collect();

    let fallback_name = split_pascal_case(&enum_name)
      .last()
      .cloned()
      .unwrap_or_else(|| enum_name.clone());

    let fallback = DiscriminatedVariant::builder()
      .variant_name(EnumVariantToken::new(fallback_name))
      .type_name(TypeRef::new(fallback_type).with_boxed())
      .build();

    Ok(
      RustType::discriminated_enum()
        .name(&EnumToken::from_raw(enum_name))
        .docs(Documentation::from_optional(schema.description.as_ref()))
        .discriminator_field(tag_field.clone())
        .variants(variants)
        .maybe_fallback(Some(fallback))
        .call(),
    )
  }

  /// Returns discriminator tag-to-schema mappings grouped by target schema.
  ///
  /// Each entry contains `(schema_name, tags)` where `tags` are the discriminator
  /// values that map to that schema. Results are ordered alphabetically by schema name.
  pub(crate) fn discriminator_mappings(&self, schema: &ObjectSchema) -> Vec<(String, Vec<String>)> {
    let Some(mapping) = schema.discriminator.as_ref().and_then(|d| d.mapping.as_ref()) else {
      return vec![];
    };

    let is_reachable = |name: &String| {
      self
        .context
        .reachable_schemas
        .as_ref()
        .is_none_or(|filter| filter.contains(name))
    };

    mapping
      .iter()
      .filter_map(|(tag, ref_path)| {
        let name = SchemaRegistry::parse_ref(ref_path)?;
        is_reachable(&name).then_some((tag.clone(), name))
      })
      .fold(BTreeMap::<String, Vec<String>>::new(), |mut acc, (tag, name)| {
        acc.entry(name).or_default().push(tag);
        acc
      })
      .into_iter()
      .collect()
  }

  /// Tries to convert existing union variants into a discriminated enum.
  ///
  /// Returns `None` if the schema lacks a discriminator mapping or if the
  /// variants don't match the mapping entries.
  pub(crate) fn try_from_variants(
    name: &str,
    schema: &ObjectSchema,
    variants: &[VariantDef],
    methods: Vec<EnumMethod>,
  ) -> Option<RustType> {
    let discriminator = schema.discriminator.as_ref()?;
    let mapping = discriminator.mapping.as_ref()?;

    if !Self::variants_cover_mapping(variants, mapping) {
      return None;
    }

    let discriminated_variants = Self::map_variants(variants, mapping);

    Some(
      RustType::discriminated_enum()
        .name(&EnumToken::from_raw(name))
        .docs(Documentation::from_optional(schema.description.as_ref()))
        .discriminator_field(discriminator.property_name.clone())
        .variants(discriminated_variants)
        .methods(methods)
        .call(),
    )
  }

  /// Returns true if all mapping entries have a corresponding variant type.
  fn variants_cover_mapping(variants: &[VariantDef], mapping: &BTreeMap<String, String>) -> bool {
    if variants.is_empty() || mapping.is_empty() {
      return false;
    }

    let known_types: BTreeSet<_> = variants.iter().filter_map(VariantDef::unboxed_type_name).collect();

    mapping.values().all(|ref_path| {
      SchemaRegistry::parse_ref(ref_path).is_some_and(|name| known_types.contains(&to_rust_type_name(&name)))
    })
  }

  /// Converts union variants to discriminated variants using the mapping.
  fn map_variants(variants: &[VariantDef], mapping: &BTreeMap<String, String>) -> Vec<DiscriminatedVariant> {
    let tags_by_type = mapping
      .iter()
      .filter_map(|(tag, ref_path)| {
        let type_name = SchemaRegistry::parse_ref(ref_path).map(|n| to_rust_type_name(&n))?;
        Some((type_name, tag.clone()))
      })
      .fold(BTreeMap::<String, Vec<String>>::new(), |mut acc, (type_name, tag)| {
        acc.entry(type_name).or_default().push(tag);
        acc
      });

    tags_by_type
      .into_iter()
      .filter_map(|(type_name, tags)| {
        let variant = variants
          .iter()
          .find(|v| v.unboxed_type_name() == Some(type_name.clone()))?;
        let inner_type = variant.single_wrapped_type()?;

        Some(
          DiscriminatedVariant::builder()
            .variant_name(variant.name.clone())
            .type_name(inner_type.clone())
            .discriminator_values(tags)
            .build(),
        )
      })
      .collect()
  }
}

use std::{
  collections::{BTreeMap, BTreeSet},
  rc::Rc,
};

use oas3::spec::ObjectSchema;

use crate::generator::{
  ast::{DiscriminatedVariant, Documentation, EnumMethod, EnumToken, EnumVariantToken, RustType, TypeRef, VariantDef},
  converter::ConverterContext,
  naming::identifiers::{split_pascal_case, strip_parent_prefix, to_rust_type_name},
  schema_registry::{ParentInfo, SchemaRegistry},
};

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
  pub(crate) fn build_base_discriminated_enum(
    &self,
    name: &str,
    schema: &ObjectSchema,
    fallback_type: &str,
  ) -> anyhow::Result<RustType> {
    let tag_field = schema
      .discriminator
      .as_ref()
      .map(|d| &d.property_name)
      .ok_or_else(|| anyhow::anyhow!("missing discriminator property for schema '{name}'"))?;

    let enum_name = to_rust_type_name(name);
    let variants = self.build_variants_from_mapping(&enum_name, schema);

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
  pub(crate) fn build_variants_from_mapping(
    &self,
    parent_name: &str,
    schema: &ObjectSchema,
  ) -> Vec<DiscriminatedVariant> {
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
      .map(|(schema_name, tags)| {
        let type_name = to_rust_type_name(&schema_name);
        let variant_name = strip_parent_prefix(parent_name, &type_name);

        DiscriminatedVariant::builder()
          .variant_name(EnumVariantToken::new(variant_name))
          .discriminator_values(tags)
          .type_name(TypeRef::new(type_name).with_boxed())
          .build()
      })
      .collect()
  }

  /// Tries to convert existing union variants into a discriminated enum.
  ///
  /// Returns `None` if the schema lacks a discriminator mapping or if the
  /// variants don't match the mapping entries.
  pub(crate) fn try_upgrade_to_discriminated(
    name: &str,
    schema: &ObjectSchema,
    variants: &[VariantDef],
    methods: Vec<EnumMethod>,
  ) -> Option<RustType> {
    let discriminator = schema.discriminator.as_ref()?;
    let mapping = discriminator.mapping.as_ref()?;

    if !Self::all_mappings_have_variants(variants, mapping) {
      return None;
    }

    let discriminated_variants = Self::convert_to_discriminated_variants(variants, mapping);

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
  fn all_mappings_have_variants(variants: &[VariantDef], mapping: &BTreeMap<String, String>) -> bool {
    if variants.is_empty() || mapping.is_empty() {
      return false;
    }

    let known_types = variants
      .iter()
      .filter_map(VariantDef::unboxed_type_name)
      .collect::<BTreeSet<_>>();

    mapping.values().all(|ref_path| {
      SchemaRegistry::parse_ref(ref_path).is_some_and(|name| known_types.contains(&to_rust_type_name(&name)))
    })
  }

  /// Converts union variants to discriminated variants using the mapping.
  fn convert_to_discriminated_variants(
    variants: &[VariantDef],
    mapping: &BTreeMap<String, String>,
  ) -> Vec<DiscriminatedVariant> {
    mapping
      .iter()
      .filter_map(|(tag, ref_path)| {
        let type_name = SchemaRegistry::parse_ref(ref_path).map(|n| to_rust_type_name(&n))?;
        Some((type_name, tag.clone()))
      })
      .fold(BTreeMap::<String, Vec<String>>::new(), |mut acc, (type_name, tag)| {
        acc.entry(type_name).or_default().push(tag);
        acc
      })
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

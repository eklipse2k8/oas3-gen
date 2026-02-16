use bon::Builder;
use itertools::Itertools;
use oas3::spec::{Discriminator, ObjectOrReference, ObjectSchema};

use crate::generator::ast::{
  DiscriminatedEnumDef, DiscriminatedVariant, Documentation, EnumDef, EnumMethod, EnumToken, EnumVariantToken,
  RustType, SerdeAttribute, VariantDef,
};

/// Represents a nested union that has been promoted to a flat variant list.
#[derive(Clone, Debug, PartialEq, Builder)]
pub(crate) struct FlattenedUnion {
  pub(crate) variants: Vec<ObjectOrReference<ObjectSchema>>,
  pub(crate) description: Option<String>,
  pub(crate) discriminator: Option<Discriminator>,
}

#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub(crate) enum CollisionStrategy {
  Preserve,
  Deduplicate,
}

/// Builds a sorted list of cache keys from variant definitions for type deduplication.
///
/// Uses each variant's serde name (the wire-format value) as the cache key,
/// sorted alphabetically to produce a canonical key that identifies equivalent
/// enum types regardless of their declaration order.
pub(crate) fn variants_to_cache_key(variants: &[VariantDef]) -> Vec<String> {
  variants.iter().map(VariantDef::serde_name).sorted().collect()
}

#[derive(Clone, Debug)]
pub(crate) struct UnionVariantSpec {
  pub(crate) variant_name: EnumVariantToken,
  pub(crate) resolved_schema: ObjectSchema,
  pub(crate) ref_name: Option<String>,
}

#[bon::bon]
impl RustType {
  /// Creates an untagged enum type for `oneOf`/`anyOf` unions without a discriminator.
  ///
  /// The generated enum uses `#[serde(untagged)]` for deserialization, which attempts
  /// each variant in declaration order until one successfully deserializes.
  #[builder]
  pub(crate) fn untagged_enum(
    name: &str,
    docs: Documentation,
    variants: Vec<VariantDef>,
    methods: Vec<EnumMethod>,
  ) -> Self {
    RustType::Enum(
      EnumDef::builder()
        .name(EnumToken::from_raw(name))
        .docs(docs)
        .variants(variants)
        .serde_attrs(vec![SerdeAttribute::Untagged])
        .case_insensitive(false)
        .methods(methods)
        .build(),
    )
  }

  /// Creates a discriminated enum type for OpenAPI unions with a discriminator property.
  ///
  /// The generated enum uses the discriminator field value to select the correct variant
  /// during deserialization, avoiding the overhead of trying each variant.
  #[builder]
  pub(crate) fn discriminated_enum(
    name: &EnumToken,
    docs: Option<Documentation>,
    schema: Option<&ObjectSchema>,
    discriminator_field: String,
    variants: Vec<DiscriminatedVariant>,
    methods: Option<Vec<EnumMethod>>,
    fallback: Option<DiscriminatedVariant>,
  ) -> Self {
    RustType::DiscriminatedEnum(
      DiscriminatedEnumDef::builder()
        .name(name.clone())
        .maybe_docs(docs.or_else(|| schema.map(|s| Documentation::from_optional(s.description.as_ref()))))
        .discriminator_field(discriminator_field)
        .variants(variants)
        .maybe_methods(methods)
        .maybe_fallback(fallback)
        .build(),
    )
  }
}

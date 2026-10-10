use bon::Builder;
use itertools::Itertools;
use oas3::spec::{Discriminator, ObjectSchema, Schema};

use crate::generator::ast::{
  DiscriminatedEnumDef, DiscriminatedVariant, Documentation, EnumDef, EnumDefault, EnumMethod, EnumToken,
  EnumVariantToken, RustType, SerdeAttribute, VariantContent, VariantDef,
};

/// Represents a nested union that has been promoted to a flat variant list.
#[derive(Clone, Debug, PartialEq, Builder)]
pub struct FlattenedUnion {
  pub variants: Vec<Schema>,
  pub description: Option<String>,
  pub discriminator: Option<Discriminator>,
  pub is_one_of: bool,
}

impl From<FlattenedUnion> for ObjectSchema {
  fn from(flattened: FlattenedUnion) -> Self {
    ObjectSchema {
      description: flattened.description,
      discriminator: flattened.discriminator,
      one_of: if flattened.is_one_of {
        flattened.variants.clone()
      } else {
        vec![]
      },
      any_of: if flattened.is_one_of {
        vec![]
      } else {
        flattened.variants
      },
      ..Default::default()
    }
  }
}

#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum CollisionStrategy {
  Preserve,
  Deduplicate,
}

/// Builds a sorted list of cache keys from variant definitions for type deduplication.
///
/// Uses each variant's serde name (the wire-format value) as the cache key,
/// sorted alphabetically so enums with identical value sets dedupe regardless
/// of their declaration order in the spec. The cache key is internal and does
/// not affect the order of variants emitted in generated code.
pub fn variants_to_cache_key(variants: &[VariantDef]) -> Vec<String> {
  variants.iter().map(|v| v.serde_name().to_string()).sorted().collect()
}

#[derive(Clone, Debug)]
pub struct UnionVariantSpec {
  pub variant_name: EnumVariantToken,
  pub resolved_schema: ObjectSchema,
  pub ref_name: Option<String>,
}

#[bon::bon]
impl RustType {
  /// Creates an untagged enum type for `oneOf`/`anyOf` unions without a discriminator.
  ///
  /// Payload variants deserialize untagged: serde attempts each in declaration order
  /// until one succeeds. Unit variants for `const` branches can't match their value
  /// untagged, so when a union has them they stay tagged and come first, and only
  /// the payload variants are marked `#[serde(untagged)]`. Otherwise the whole enum
  /// is `#[serde(untagged)]`.
  #[builder]
  pub fn untagged_enum(
    name: &str,
    docs: Documentation,
    variants: Vec<VariantDef>,
    methods: Vec<EnumMethod>,
    #[builder(default)] default_mode: EnumDefault,
  ) -> Self {
    let is_payload = |variant: &VariantDef| variant.content != VariantContent::Unit;
    let (serde_attrs, variants) = if variants.iter().all(is_payload) {
      (vec![SerdeAttribute::Untagged], variants)
    } else {
      let variants = variants
        .into_iter()
        .sorted_by_key(is_payload)
        .map(|mut variant| {
          if is_payload(&variant) {
            variant.serde_attrs.push(SerdeAttribute::Untagged);
          }
          variant
        })
        .collect();
      (vec![], variants)
    };

    RustType::Enum(
      EnumDef::builder()
        .name(EnumToken::from_raw(name))
        .docs(docs)
        .variants(variants)
        .serde_attrs(serde_attrs)
        .case_insensitive(false)
        .methods(methods)
        .default_mode(default_mode)
        .build(),
    )
  }

  /// Creates a discriminated enum type for OpenAPI unions with a discriminator property.
  ///
  /// The generated enum uses the discriminator field value to select the correct variant
  /// during deserialization, avoiding the overhead of trying each variant.
  #[builder]
  pub fn discriminated_enum(
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

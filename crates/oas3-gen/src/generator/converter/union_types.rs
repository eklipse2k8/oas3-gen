use oas3::spec::ObjectSchema;
use serde_json::Value;

use crate::generator::ast::{
  DiscriminatedEnumDef, DiscriminatedVariant, Documentation, EnumDef, EnumMethod, EnumToken, EnumVariantToken,
  RustType, SerdeAttribute, VariantDef,
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
pub(super) struct UnionVariantSpec {
  pub(super) variant_name: EnumVariantToken,
  pub(super) resolved_schema: ObjectSchema,
  pub(super) ref_name: Option<String>,
}

#[bon::bon]
impl RustType {
  #[builder]
  pub(crate) fn untagged_enum(
    name: &str,
    schema: &ObjectSchema,
    variants: Vec<VariantDef>,
    methods: Vec<EnumMethod>,
  ) -> Self {
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

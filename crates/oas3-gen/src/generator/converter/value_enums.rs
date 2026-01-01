use std::collections::BTreeMap;

use super::union_types::{CollisionStrategy, EnumValueEntry};
use crate::generator::{
  ast::{Documentation, EnumDef, EnumToken, EnumVariantToken, RustType, SerdeAttribute, VariantContent, VariantDef},
  naming::inference::NormalizedVariant,
};

#[derive(Clone, Debug)]
pub(crate) struct ValueEnumBuilder {
  case_insensitive: bool,
}

impl ValueEnumBuilder {
  pub(crate) fn new(case_insensitive: bool) -> Self {
    Self { case_insensitive }
  }

  pub(crate) fn build_enum_from_values(
    &self,
    name: &str,
    entries: &[EnumValueEntry],
    strategy: CollisionStrategy,
    docs: Documentation,
  ) -> RustType {
    let mut variants: Vec<VariantDef> = vec![];
    let mut seen_names: BTreeMap<String, usize> = BTreeMap::new();

    for (i, entry) in entries.iter().enumerate() {
      let Ok(normalized) = NormalizedVariant::try_from(&entry.value) else {
        continue;
      };

      match seen_names.get(&normalized.name) {
        Some(&existing_idx) if strategy == CollisionStrategy::Deduplicate => {
          variants[existing_idx].add_alias(normalized.rename_value);
        }
        Some(_) => {
          Self::handle_preserve_collision(&mut variants, &mut seen_names, &normalized, i, entry);
        }
        None => {
          Self::add_new_variant(&mut variants, &mut seen_names, normalized, entry);
        }
      }
    }

    RustType::Enum(
      EnumDef::builder()
        .name(EnumToken::from_raw(name))
        .docs(docs)
        .variants(variants)
        .case_insensitive(self.case_insensitive)
        .build(),
    )
  }

  fn handle_preserve_collision(
    variants: &mut Vec<VariantDef>,
    seen_names: &mut BTreeMap<String, usize>,
    normalized: &NormalizedVariant,
    index: usize,
    entry: &EnumValueEntry,
  ) {
    let unique_name = format!("{}{index}", normalized.name);
    let idx = variants.len();
    seen_names.insert(unique_name.clone(), idx);
    variants.push(
      VariantDef::builder()
        .name(EnumVariantToken::from(unique_name))
        .docs(entry.docs.clone())
        .content(VariantContent::Unit)
        .serde_attrs(vec![SerdeAttribute::Rename(normalized.rename_value.clone())])
        .deprecated(entry.deprecated)
        .build(),
    );
  }

  fn add_new_variant(
    variants: &mut Vec<VariantDef>,
    seen_names: &mut BTreeMap<String, usize>,
    normalized: NormalizedVariant,
    entry: &EnumValueEntry,
  ) {
    let idx = variants.len();
    seen_names.insert(normalized.name.clone(), idx);
    variants.push(
      VariantDef::builder()
        .name(EnumVariantToken::from(normalized.name))
        .docs(entry.docs.clone())
        .content(VariantContent::Unit)
        .serde_attrs(vec![SerdeAttribute::Rename(normalized.rename_value)])
        .deprecated(entry.deprecated)
        .build(),
    );
  }
}

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
  /// Creates a new builder for constructing value enums from OpenAPI `enum` arrays.
  ///
  /// When `case_insensitive` is `true`, the generated enum will deserialize values
  /// regardless of letter case (e.g., `"active"`, `"ACTIVE"`, and `"Active"` all
  /// deserialize to the same variant).
  pub(crate) fn new(case_insensitive: bool) -> Self {
    Self { case_insensitive }
  }

  /// Constructs a Rust enum from an array of OpenAPI enum values.
  ///
  /// Converts JSON string, number, and boolean values into PascalCase enum variants
  /// with appropriate `#[serde(rename = "...")]` attributes to preserve the original
  /// wire format.
  ///
  /// When multiple values normalize to the same Rust identifier (e.g., `"foo-bar"` and
  /// `"foo_bar"` both become `FooBar`), the `strategy` parameter controls the behavior:
  ///
  /// - [`CollisionStrategy::Deduplicate`]: Merges collisions into a single variant with
  ///   `#[serde(alias = "...")]` for additional values.
  /// - [`CollisionStrategy::Preserve`]: Creates distinct variants by appending the
  ///   entry index (e.g., `FooBar`, `FooBar1`).
  pub(crate) fn build_enum_from_values(
    &self,
    name: &str,
    entries: &[EnumValueEntry],
    strategy: CollisionStrategy,
    docs: Documentation,
  ) -> RustType {
    let (variants, _) = entries
      .iter()
      .enumerate()
      .filter_map(|(i, entry)| {
        NormalizedVariant::try_from(&entry.value)
          .ok()
          .map(|normalized| (i, entry, normalized))
      })
      .fold(
        (vec![], BTreeMap::<String, usize>::new()),
        |(mut variants, mut seen): (Vec<VariantDef>, BTreeMap<String, usize>), (i, entry, normalized)| {
          match seen.get(&normalized.name).copied() {
            Some(idx) if strategy == CollisionStrategy::Deduplicate => {
              variants[idx].add_alias(normalized.rename_value);
            }
            Some(_) => {
              let unique_name = format!("{}{i}", normalized.name);
              seen.insert(unique_name.clone(), variants.len());
              variants.push(Self::build_variant(unique_name, &normalized.rename_value, entry));
            }
            None => {
              seen.insert(normalized.name.clone(), variants.len());
              variants.push(Self::build_variant(normalized.name, &normalized.rename_value, entry));
            }
          }
          (variants, seen)
        },
      );

    RustType::Enum(
      EnumDef::builder()
        .name(EnumToken::from_raw(name))
        .docs(docs)
        .variants(variants)
        .case_insensitive(self.case_insensitive)
        .generate_display(true)
        .build(),
    )
  }

  /// Constructs a unit enum variant with serde rename and documentation attributes.
  ///
  /// The `variant_name` is the PascalCase identifier for the variant (e.g., `"FooBar"`).
  /// The `rename_value` is the original value to use in `#[serde(rename = "...")]`.
  fn build_variant(variant_name: String, rename_value: &str, entry: &EnumValueEntry) -> VariantDef {
    VariantDef::builder()
      .name(EnumVariantToken::from(variant_name))
      .docs(entry.docs.clone())
      .content(VariantContent::Unit)
      .serde_attrs(vec![SerdeAttribute::Rename(rename_value.to_owned())])
      .deprecated(entry.deprecated)
      .build()
  }
}

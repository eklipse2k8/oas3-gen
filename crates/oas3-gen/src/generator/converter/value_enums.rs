use std::collections::BTreeMap;

use super::union_types::CollisionStrategy;
use crate::generator::ast::{Documentation, EnumDef, EnumToken, EnumVariantToken, RustType, VariantDef};

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

  /// Constructs a Rust enum from pre-built variant definitions.
  ///
  /// When multiple variants share the same Rust identifier (e.g., `"foo-bar"` and
  /// `"foo_bar"` both become `FooBar`), the `strategy` parameter controls the behavior:
  ///
  /// - [`CollisionStrategy::Deduplicate`]: Merges collisions into a single variant with
  ///   `#[serde(alias = "...")]` for additional values.
  /// - [`CollisionStrategy::Preserve`]: Creates distinct variants by appending the
  ///   entry index (e.g., `FooBar`, `FooBar1`).
  pub(crate) fn build_enum_from_variants(
    &self,
    name: &str,
    variants: Vec<VariantDef>,
    strategy: CollisionStrategy,
    docs: Documentation,
  ) -> RustType {
    let (resolved_variants, _) = variants.into_iter().enumerate().fold(
      (vec![], BTreeMap::<String, usize>::new()),
      |(mut acc, mut seen): (Vec<VariantDef>, BTreeMap<String, usize>), (i, mut variant)| {
        let variant_name = variant.name.to_string();
        match seen.get(&variant_name).copied() {
          Some(idx) if strategy == CollisionStrategy::Deduplicate => {
            acc[idx].add_alias(variant.serde_name());
          }
          Some(_) => {
            let unique_name = format!("{variant_name}{i}");
            seen.insert(unique_name.clone(), acc.len());
            variant.name = EnumVariantToken::from(unique_name);
            acc.push(variant);
          }
          None => {
            seen.insert(variant_name, acc.len());
            acc.push(variant);
          }
        }
        (acc, seen)
      },
    );

    RustType::Enum(
      EnumDef::builder()
        .name(EnumToken::from_raw(name))
        .docs(docs)
        .variants(resolved_variants)
        .case_insensitive(self.case_insensitive)
        .generate_display(true)
        .build(),
    )
  }
}

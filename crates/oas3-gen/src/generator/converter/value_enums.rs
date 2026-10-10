use std::collections::{BTreeMap, BTreeSet};

use itertools::{AllEqualValueError, Itertools};
use oas3::spec::{ObjectSchema, SchemaType};

use super::union_types::CollisionStrategy;
use crate::{
  generator::{
    ast::{
      Documentation, EnumDef, EnumDefault, EnumDefaultValue, EnumToken, EnumVariantToken, RustPrimitive, RustType,
      SerdeAttribute, TypeRef, ValueKind, VariantContent, VariantDef,
    },
    naming::{constants::OTHER_ENUM_VARIANT, identifiers::ensure_unique, inference::NormalizedVariant},
  },
  utils::SchemaExt,
};

#[derive(Clone, Debug)]
pub(crate) struct ValueEnumBuilder {
  case_insensitive: bool,
  sort_variants: bool,
  strategy: CollisionStrategy,
}

impl ValueEnumBuilder {
  /// Creates a new builder for constructing value enums from OpenAPI `enum` arrays.
  ///
  /// When `case_insensitive` is `true`, the generated enum will deserialize values
  /// regardless of letter case (e.g., `"active"`, `"ACTIVE"`, and `"Active"` all
  /// deserialize to the same variant).
  ///
  /// When `sort_variants` is `true`, generated variants are emitted in alphabetical
  /// order by Rust variant name regardless of declaration order in the OpenAPI spec.
  ///
  /// When multiple variants share the same Rust identifier (e.g., `"foo-bar"` and
  /// `"foo_bar"` both become `FooBar`), `strategy` controls the behavior:
  ///
  /// - [`CollisionStrategy::Deduplicate`]: Merges collisions into a single variant with
  ///   `#[serde(alias = "...")]` for additional values.
  /// - [`CollisionStrategy::Preserve`]: Creates distinct variants by appending the
  ///   entry index (e.g., `FooBar`, `FooBar1`).
  pub(crate) fn new(case_insensitive: bool, sort_variants: bool, strategy: CollisionStrategy) -> Self {
    Self {
      case_insensitive,
      sort_variants,
      strategy,
    }
  }

  /// Constructs the Rust enum for `schema` from pre-built variant definitions.
  ///
  /// The enum takes its docs from the schema `description` and its wire representation
  /// from [`scalar_repr`]. When the schema `default` matches a variant's wire value
  /// (its rename or one of its aliases), that variant is marked as the enum's
  /// `Default`; otherwise the first variant remains the fallback default.
  ///
  /// When `non_exhaustive` is `true`, the enum is marked `#[non_exhaustive]` and gains
  /// a trailing `#[serde(untagged)] Other(String)` variant holding any value the spec
  /// doesn't list. A string `default` no variant matches then becomes the enum's
  /// `Default` through that variant, e.g. `Other("custom".to_string())`.
  pub(crate) fn build_enum_from_variants(
    &self,
    name: &str,
    schema: &ObjectSchema,
    variants: Vec<VariantDef>,
    non_exhaustive: bool,
  ) -> RustType {
    let (resolved_variants, _) = variants.into_iter().enumerate().fold(
      (vec![], BTreeMap::<String, usize>::new()),
      |(mut acc, mut seen): (Vec<VariantDef>, BTreeMap<String, usize>), (i, mut variant)| {
        let variant_name = variant.name.to_string();
        match seen.get(&variant_name).copied() {
          Some(idx) if self.strategy == CollisionStrategy::Deduplicate => {
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

    let mut resolved_variants = if self.sort_variants {
      resolved_variants
        .into_iter()
        .sorted_by(|a, b| a.name.as_str().cmp(b.name.as_str()))
        .collect()
    } else {
      resolved_variants
    };

    let default_value = schema.default.as_ref();
    let listed_default = default_value
      .and_then(|value| NormalizedVariant::try_from(value).ok())
      .and_then(|default| {
        resolved_variants
          .iter()
          .position(|v| variant_matches_value(v, &default.rename_value))
      });
    if let Some(index) = listed_default {
      resolved_variants[index].default = true;
    }

    let scalar_repr = scalar_repr(schema, &resolved_variants);
    let unlisted_default =
      default_value.filter(|value| non_exhaustive && listed_default.is_none() && value.is_string());
    if non_exhaustive {
      let mut other = other_variant(&resolved_variants);
      other.default = unlisted_default.is_some();
      resolved_variants.push(other);
    }
    let default_mode = unlisted_default.map_or(EnumDefault::Derive, |value| {
      EnumDefault::Value(EnumDefaultValue {
        value: value.clone(),
        inner_variant: None,
      })
    });

    RustType::Enum(
      EnumDef::builder()
        .name(EnumToken::from_raw(name))
        .docs(Documentation::from_optional(schema.description.as_ref()))
        .variants(resolved_variants)
        .case_insensitive(self.case_insensitive)
        .generate_display(true)
        .maybe_scalar_repr(scalar_repr)
        .non_exhaustive(non_exhaustive)
        .default_mode(default_mode)
        .build(),
    )
  }
}

/// Determines the primitive backing a value enum's wire values, or `None` for a
/// string-backed enum, which uses the derived serde path.
///
/// A declared `type: integer` or `type: number` picks a numeric primitive (honoring
/// any numeric `format`), and `type: string` keeps the enum string-backed. Otherwise
/// the listed values decide: all booleans give `bool`, all numbers give `i64` (or
/// `f64` when any is fractional), and a mix of JSON types gives `serde_json::Value`.
pub(crate) fn scalar_repr(schema: &ObjectSchema, variants: &[VariantDef]) -> Option<RustPrimitive> {
  match schema.single_type() {
    Some(SchemaType::Integer) => {
      return Some(RustPrimitive::with_format_override(
        RustPrimitive::I64,
        schema.format.as_deref(),
      ));
    }
    Some(SchemaType::Number) => {
      return Some(RustPrimitive::with_format_override(
        RustPrimitive::F64,
        schema.format.as_deref(),
      ));
    }
    Some(SchemaType::String) => return None,
    _ => {}
  }

  match variants.iter().map(|v| v.value_kind).all_equal_value() {
    Ok(ValueKind::String) | Err(AllEqualValueError(None)) => None,
    Ok(ValueKind::Bool) => Some(RustPrimitive::Bool),
    Ok(ValueKind::Number) if variants.iter().all(|v| v.serde_name().parse::<i64>().is_ok()) => Some(RustPrimitive::I64),
    Ok(ValueKind::Number) => Some(RustPrimitive::F64),
    Err(AllEqualValueError(Some(_))) => Some(RustPrimitive::Value),
  }
}

/// Builds the `#[serde(untagged)] Other(String)` catch-all of a non-exhaustive enum, numbered
/// when one of the `listed` variants already claims the name.
fn other_variant(listed: &[VariantDef]) -> VariantDef {
  let taken = listed.iter().map(|v| v.name.to_string()).collect::<BTreeSet<_>>();
  VariantDef::builder()
    .name(EnumVariantToken::new(ensure_unique(OTHER_ENUM_VARIANT, &taken)))
    .content(VariantContent::Tuple(vec![TypeRef::new(RustPrimitive::String)]))
    .serde_attrs(vec![SerdeAttribute::Untagged])
    .build()
}

/// Returns `true` when the variant serializes to or from `value`, either as its
/// primary `#[serde(rename)]` value or as one of its `#[serde(alias)]` values.
pub(crate) fn variant_matches_value(variant: &VariantDef, value: &str) -> bool {
  variant.wire_values().any(|wire_value| wire_value == value)
}

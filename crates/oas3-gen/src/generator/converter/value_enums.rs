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
pub struct ValueEnumBuilder {
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
  ///   entry index (e.g., `FooBar`, `FooBar1`), numbered again when an earlier variant
  ///   already holds the suffixed name. A suffixed variant keeps its original
  ///   value through `#[serde(rename)]`, so it still serializes and displays as the
  ///   spelling the spec lists.
  pub fn new(case_insensitive: bool, sort_variants: bool, strategy: CollisionStrategy) -> Self {
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
  pub fn build_enum_from_variants(
    &self,
    name: &str,
    schema: &ObjectSchema,
    variants: Vec<VariantDef>,
    non_exhaustive: bool,
  ) -> RustType {
    let resolved_variants = match self.strategy {
      CollisionStrategy::Deduplicate => merge_collisions(variants),
      CollisionStrategy::Preserve => suffix_collisions(variants),
    };

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

/// Folds each variant whose Rust identifier is already taken into the first variant
/// with that identifier, keeping its wire value as a `#[serde(alias)]`.
fn merge_collisions(variants: Vec<VariantDef>) -> Vec<VariantDef> {
  let mut merged = Vec::<VariantDef>::with_capacity(variants.len());
  let mut positions = BTreeMap::<String, usize>::new();
  for variant in variants {
    if let Some(&position) = positions.get(variant.name.as_str()) {
      merged[position].add_alias(variant.serde_name());
    } else {
      positions.insert(variant.name.to_string(), merged.len());
      merged.push(variant);
    }
  }
  merged
}

/// Keeps every variant, renaming each one whose Rust identifier is already taken to
/// that identifier plus its entry index, made unique against every earlier variant.
///
/// A renamed variant gains a `#[serde(rename)]` holding its original identifier when
/// it had none, so its wire value stays the spelling the spec lists.
fn suffix_collisions(variants: Vec<VariantDef>) -> Vec<VariantDef> {
  let mut preserved = Vec::with_capacity(variants.len());
  let mut taken = BTreeSet::<String>::new();
  for (index, mut variant) in variants.into_iter().enumerate() {
    if taken.contains(variant.name.as_str()) {
      let original = variant.name.to_string();
      variant.name = EnumVariantToken::from(ensure_unique(&format!("{original}{index}"), &taken));
      if !variant
        .serde_attrs
        .iter()
        .any(|attr| matches!(attr, SerdeAttribute::Rename(_)))
      {
        variant.serde_attrs.insert(0, SerdeAttribute::Rename(original));
      }
    }
    taken.insert(variant.name.to_string());
    preserved.push(variant);
  }
  preserved
}

/// Determines the primitive backing a value enum's wire values, or `None` for a
/// string-backed enum, which uses the derived serde path.
///
/// A declared `type: integer` or `type: number` picks a numeric primitive (honoring
/// any numeric `format`), and `type: string` keeps the enum string-backed. Otherwise
/// the listed values decide: all booleans give `bool`, all numbers give `i64` (or
/// `f64` when any is fractional), and a mix of JSON types gives `serde_json::Value`.
pub fn scalar_repr(schema: &ObjectSchema, variants: &[VariantDef]) -> Option<RustPrimitive> {
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
pub fn variant_matches_value(variant: &VariantDef, value: &str) -> bool {
  variant.wire_values().any(|wire_value| wire_value == value)
}

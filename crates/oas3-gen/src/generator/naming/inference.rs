use std::collections::{BTreeMap, BTreeSet};

use inflections::Inflect;
use oas3::spec::{ObjectOrReference, ObjectSchema, SchemaType, SchemaTypeSet};

use crate::generator::{
  ast::{EnumVariantToken, VariantDef},
  naming::{
    constants::{REQUEST_BODY_SUFFIX, RESPONSE_PREFIX, RESPONSE_SUFFIX},
    identifiers::{sanitize, split_pascal_case, to_rust_type_name},
  },
  schema_registry::{RefCollector, SchemaRegistry},
};

/// Returns an iterator over all union variants (`anyOf` and `oneOf`) in a schema.
///
/// # Example
/// ```text
/// schema.any_of = [A, B], schema.one_of = [C] => yields A, B, C
/// ```
///
/// # Complexity
/// O(1) - creates a chained iterator without allocation.
fn union_variants(schema: &ObjectSchema) -> impl Iterator<Item = &ObjectOrReference<ObjectSchema>> {
  schema.any_of.iter().chain(&schema.one_of)
}

#[must_use]
fn schema_single_type(schema: &ObjectSchema) -> Option<SchemaType> {
  match &schema.schema_type {
    Some(SchemaTypeSet::Single(t)) => Some(*t),
    Some(SchemaTypeSet::Multiple(types)) if types.len() == 2 && types.contains(&SchemaType::Null) => {
      types.iter().find(|t| **t != SchemaType::Null).copied()
    }
    _ => None,
  }
}

#[must_use]
fn schema_is_string(schema: &ObjectSchema) -> bool {
  matches!(schema_single_type(schema), Some(SchemaType::String))
}

pub(crate) struct CommonVariantName {
  pub(crate) name: String,
  pub(crate) has_suffix: bool,
}

/// Extracts a semantic name from union variant references by combining the first
/// common prefix segment with the common suffix.
///
/// For variants like `BetaResponseCharLocationCitation`, `BetaResponseUrlCitation`,
/// `BetaResponseFileCitation`, this returns `CommonVariantName { name: "BetaCitation", has_suffix: true }`.
///
/// For variants like `BetaTool`, `BetaBashTool20241022`, this returns `CommonVariantName { name: "Beta", has_suffix: false }`.
///
/// The `has_suffix` field indicates whether a common suffix was found. When true, the extracted name
/// is semantically complete and should be used as-is. When false, the property name should
/// be appended for clarity.
///
/// Returns `None` if fewer than 2 variants have references or no common prefix exists.
pub(crate) fn extract_common_variant_prefix(variants: &[ObjectOrReference<ObjectSchema>]) -> Option<CommonVariantName> {
  let ref_names: Vec<String> = variants.iter().filter_map(RefCollector::parse_schema_ref).collect();

  if ref_names.len() < 2 {
    return None;
  }

  let segments: Vec<Vec<String>> = ref_names.iter().map(|n| split_pascal_case(n)).collect();
  let first = segments.first().filter(|s| !s.is_empty())?;
  let rest = &segments[1..];

  let prefix_len = common_prefix_len(first, rest);
  if prefix_len == 0 {
    return None;
  }

  let suffix_len = common_suffix_len(first, rest);
  Some(build_common_variant_name(first, prefix_len, suffix_len))
}

/// Counts word segments shared at the start of all PascalCase-split name lists.
///
/// # Example
/// ```text
/// first = ["User", "Create", "Request"]
/// rest  = [["User", "Update", "Request"], ["User", "Delete", "Request"]]
/// => 1 (only "User" is common prefix)
/// ```
///
/// # Complexity
/// O(p * n) where p = prefix length, n = number of variants in `rest`.
#[must_use]
fn common_prefix_len(first: &[String], rest: &[Vec<String>]) -> usize {
  first
    .iter()
    .enumerate()
    .take_while(|(i, seg)| rest.iter().all(|other| other.get(*i) == Some(seg)))
    .count()
}

/// Counts word segments shared at the end of all PascalCase-split name lists.
///
/// # Example
/// ```text
/// first = ["Create", "User", "Response"]
/// rest  = [["Update", "User", "Response"], ["Delete", "User", "Response"]]
/// => 2 ("User", "Response" are common suffix)
/// ```
///
/// # Complexity
/// O(s * n) where s = suffix length, n = number of variants in `rest`.
#[must_use]
fn common_suffix_len(first: &[String], rest: &[Vec<String>]) -> usize {
  let min_len = std::iter::once(first.len())
    .chain(rest.iter().map(Vec::len))
    .min()
    .unwrap_or(0);

  (1..=min_len)
    .take_while(|&offset| {
      let seg = &first[first.len() - offset];
      rest.iter().all(|other| other.get(other.len() - offset) == Some(seg))
    })
    .count()
}

/// Constructs a `CommonVariantName` from word segments using prefix/suffix lengths.
///
/// If a suffix exists, combines the first segment with the suffix (e.g., "Beta" + "Citation").
/// Otherwise, joins all prefix segments.
///
/// # Example
/// ```text
/// segments = ["Beta", "Response", "Url", "Citation"], prefix_len = 1, suffix_len = 1
/// => CommonVariantName { name: "BetaCitation", has_suffix: true }
///
/// segments = ["Beta", "Tool"], prefix_len = 1, suffix_len = 0
/// => CommonVariantName { name: "Beta", has_suffix: false }
/// ```
///
/// # Complexity
/// O(k) where k = number of segments joined.
#[must_use]
fn build_common_variant_name(segments: &[String], prefix_len: usize, suffix_len: usize) -> CommonVariantName {
  if suffix_len > 0 {
    let suffix = segments[segments.len() - suffix_len..].join("");
    CommonVariantName {
      name: format!("{}{suffix}", segments[0]),
      has_suffix: true,
    }
  } else {
    CommonVariantName {
      name: segments[..prefix_len].join(""),
      has_suffix: false,
    }
  }
}

/// Returns true if schema is an unconstrained string type (no enum/const restrictions).
///
/// # Example
/// ```text
/// { "type": "string" }                    => true
/// { "type": "string", "enum": ["a"] }     => false
/// { "type": "string", "const": "x" }      => false
/// ```
///
/// # Complexity
/// O(1) - field access only.
#[must_use]
fn is_freeform_string(schema: &ObjectSchema) -> bool {
  schema_is_string(schema) && schema.enum_values.is_empty() && schema.const_value.is_none()
}

/// Returns true if schema has enum values or a const constraint.
///
/// # Example
/// ```text
/// { "enum": ["a", "b"] }  => true
/// { "const": "x" }        => true
/// { "type": "string" }    => false
/// ```
///
/// # Complexity
/// O(1) - field access only.
#[must_use]
fn is_constrained(schema: &ObjectSchema) -> bool {
  !schema.enum_values.is_empty() || schema.const_value.is_some()
}

/// Checks if a schema matches the "relaxed enum" pattern.
///
/// A relaxed enum is defined as having a freeform string variant (no enum values, no const)
/// alongside other variants that are constrained (enum values or const).
pub(crate) fn is_relaxed_enum_pattern(schema: &ObjectSchema) -> bool {
  has_mixed_string_variants(union_variants(schema))
}

/// Checks if variants contain both freeform strings and constrained strings.
///
/// Used to detect "relaxed enum" patterns where an API accepts known enum values
/// plus arbitrary strings for forward compatibility.
///
/// # Example
/// ```text
/// anyOf: [{ type: string }, { type: string, enum: ["a", "b"] }] => true
/// anyOf: [{ type: string, enum: ["a"] }, { type: string, enum: ["b"] }] => false
/// ```
///
/// # Complexity
/// O(n) where n = number of variants (single pass).
pub(crate) fn has_mixed_string_variants<'a>(
  variants: impl Iterator<Item = &'a ObjectOrReference<ObjectSchema>>,
) -> bool {
  let mut has_freeform = false;
  let mut has_constrained = false;

  for v in variants {
    if let ObjectOrReference::Object(s) = v {
      if is_freeform_string(s) {
        has_freeform = true;
      } else if is_constrained(s) {
        has_constrained = true;
      }
    }

    if has_freeform && has_constrained {
      return true;
    }
  }

  false
}

/// Extracts enum values from a schema, handling standard enums, oneOf/anyOf patterns,
/// and relaxed enum patterns (mixed freeform string and constants).
///
/// Returns `None` if no valid enum values could be extracted.
pub(crate) fn extract_enum_values(schema: &ObjectSchema) -> Option<Vec<String>> {
  if let Some(values) = extract_standard_enum_values(schema) {
    return Some(values);
  }

  let variants: Vec<_> = union_variants(schema).collect();
  if variants.is_empty() {
    return None;
  }

  let has_freeform = variants
    .iter()
    .any(|v| matches!(v, ObjectOrReference::Object(s) if is_freeform_string(s)));

  if has_freeform {
    return extract_relaxed_enum_values(&variants);
  }

  if !schema.one_of.is_empty() {
    return extract_oneof_const_values(&schema.one_of);
  }

  None
}

/// Extracts string values from a schema's direct `enum` field.
///
/// # Example
/// ```text
/// { "enum": ["active", "pending", 123] } => Some(["active", "pending"])
/// { "type": "string" }                   => None
/// ```
///
/// # Complexity
/// O(n log n) where n = number of enum values (due to sorting).
fn extract_standard_enum_values(schema: &ObjectSchema) -> Option<Vec<String>> {
  if schema.enum_values.is_empty() {
    return None;
  }

  let mut values: Vec<_> = schema
    .enum_values
    .iter()
    .filter_map(|v| v.as_str().map(String::from))
    .collect();

  if values.is_empty() {
    return None;
  }

  values.sort();
  Some(values)
}

/// Extracts all enum/const string values from relaxed enum variants.
///
/// Collects values from inline schemas' `enum` arrays and `const` fields,
/// ignoring `$ref` variants and freeform strings.
///
/// # Example
/// ```text
/// anyOf: [{ type: string }, { const: "a" }, { enum: ["b", "c"] }]
/// => Some(["a", "b", "c"])
/// ```
///
/// # Complexity
/// O(v * e) where v = variants, e = enum values per variant. Uses BTreeSet for deduplication.
fn extract_relaxed_enum_values(variants: &[&ObjectOrReference<ObjectSchema>]) -> Option<Vec<String>> {
  let values: BTreeSet<_> = variants
    .iter()
    .filter_map(|variant| match variant {
      ObjectOrReference::Object(s) => {
        let enum_values = s.enum_values.iter().filter_map(|v| v.as_str().map(String::from));
        let const_value = s.const_value.as_ref().and_then(|v| v.as_str().map(String::from));
        Some(enum_values.chain(const_value))
      }
      ObjectOrReference::Ref { .. } => None,
    })
    .flatten()
    .collect();

  if values.is_empty() {
    None
  } else {
    Some(values.into_iter().collect())
  }
}

/// Extracts const values from a oneOf where all variants are const strings.
///
/// Returns `None` if any variant is a `$ref` or lacks a string const value.
///
/// # Example
/// ```text
/// oneOf: [{ const: "a" }, { const: "b" }] => Some(["a", "b"])
/// oneOf: [{ const: "a" }, { $ref: "..." }] => None
/// ```
///
/// # Complexity
/// O(n log n) where n = number of oneOf variants (BTreeSet insertion).
fn extract_oneof_const_values(one_of: &[ObjectOrReference<ObjectSchema>]) -> Option<Vec<String>> {
  let mut const_values = BTreeSet::new();

  for variant in one_of {
    match variant {
      ObjectOrReference::Object(s) => {
        let const_str = s.const_value.as_ref().and_then(|v| v.as_str())?;
        const_values.insert(const_str.to_string());
      }
      ObjectOrReference::Ref { .. } => return None,
    }
  }

  if const_values.is_empty() {
    None
  } else {
    Some(const_values.into_iter().collect())
  }
}

/// Holds the result of normalizing a schema value into a Rust identifier.
pub struct NormalizedVariant {
  /// The valid Rust identifier (e.g., "Value10_5").
  pub name: String,
  /// The original value string for serialization (e.g., "10.5").
  pub rename_value: String,
}

/// Normalizes JSON values into valid Rust variant names.
///
/// Converts strings, numbers, and booleans into PascalCase identifiers
/// suitable for enum variants, preserving original values for serde rename.
pub struct VariantNameNormalizer;

impl VariantNameNormalizer {
  #[must_use]
  pub fn normalize(value: &serde_json::Value) -> Option<NormalizedVariant> {
    match value {
      serde_json::Value::String(str_val) => Some(NormalizedVariant {
        name: to_rust_type_name(str_val),
        rename_value: str_val.clone(),
      }),
      serde_json::Value::Number(num) => {
        let raw_str = if num.is_i64() {
          num.as_i64().unwrap().to_string()
        } else if num.is_f64() {
          num.as_f64().unwrap().to_string()
        } else {
          return None;
        };
        let safe_name = raw_str.replace(['.', '-'], "_");
        Some(NormalizedVariant {
          name: format!("Value{safe_name}"),
          rename_value: raw_str,
        })
      }
      serde_json::Value::Bool(bool_val) => Some(NormalizedVariant {
        name: if *bool_val { "True".into() } else { "False".into() },
        rename_value: bool_val.to_string(),
      }),
      _ => None,
    }
  }
}

/// Infers a variant name for an inline schema in a union.
#[must_use]
pub fn infer_variant_name(schema: &ObjectSchema, index: usize) -> String {
  if !schema.enum_values.is_empty() {
    return "Enum".to_string();
  }
  if let Some(typ) = schema_single_type(schema) {
    return match typ {
      SchemaType::String => "String".to_string(),
      SchemaType::Number => "Number".to_string(),
      SchemaType::Integer => "Integer".to_string(),
      SchemaType::Boolean => "Boolean".to_string(),
      SchemaType::Array => "Array".to_string(),
      SchemaType::Object => infer_object_variant_name(schema),
      SchemaType::Null => "Null".to_string(),
    };
  }
  if schema.schema_type.is_some() {
    return "Mixed".to_string();
  }
  let variants = if schema.one_of.is_empty() {
    &schema.any_of
  } else {
    &schema.one_of
  };

  extract_common_variant_prefix(variants).map_or_else(|| format!("Variant{index}"), |c| c.name)
}

pub(crate) fn infer_union_variant_label(schema: &ObjectSchema, ref_name: Option<&str>, index: usize) -> String {
  if let Some(const_value) = &schema.const_value
    && let Some(normalized) = VariantNameNormalizer::normalize(const_value)
  {
    return normalized.name;
  }

  if let Some(schema_name) = ref_name {
    return to_rust_type_name(schema_name);
  }

  if let Some(title) = &schema.title {
    return to_rust_type_name(title);
  }

  infer_variant_name(schema, index)
}

fn infer_object_variant_name(schema: &ObjectSchema) -> String {
  if schema.properties.is_empty() {
    return "Object".to_string();
  }

  if let Some(name) = infer_name_from_required_fields(schema) {
    return name;
  }

  if let Some(name) = infer_name_from_ref_properties(schema) {
    return name;
  }

  if let Some(name) = infer_name_from_single_property(schema) {
    return name;
  }

  "Object".to_string()
}

fn infer_name_from_required_fields(schema: &ObjectSchema) -> Option<String> {
  if schema.required.len() == 1 {
    return Some(schema.required[0].to_pascal_case());
  }
  None
}

fn infer_name_from_ref_properties(schema: &ObjectSchema) -> Option<String> {
  let mut ref_names = schema.properties.values().filter_map(|prop| {
    if let ObjectOrReference::Ref { ref_path, .. } = prop {
      SchemaRegistry::parse_ref(ref_path)
    } else {
      None
    }
  });

  // Check for exactly one ref name
  if let Some(first) = ref_names.next()
    && ref_names.next().is_none()
  {
    return Some(first.to_pascal_case());
  }

  None
}

fn infer_name_from_single_property(schema: &ObjectSchema) -> Option<String> {
  if schema.properties.len() == 1 {
    return schema.properties.keys().next().map(|name| name.to_pascal_case());
  }
  None
}

/// Strips common PascalCase word segments from variant names to make them concise.
///
/// This function identifies word boundaries in PascalCase names, finds segments
/// shared by ALL variants at the beginning (prefix) and end (suffix), then removes
/// them. Changes are only applied if all resulting names remain non-empty and unique.
///
/// # Algorithm
///
/// 1. Split each name into PascalCase word segments
///    - `"CreateUserResponse"` -> `["Create", "User", "Response"]`
/// 2. Find the longest common prefix (word segments shared at the start)
/// 3. Find the longest common suffix (word segments shared at the end)
/// 4. Strip both from each name, rejoining the remaining segments
/// 5. Validate: abort if any name becomes empty or duplicates arise
///
/// # Examples
///
/// **Shared suffix:**
/// - Input: `["CreateUserResponse", "UpdateUserResponse", "DeleteUserResponse"]`
/// - Common prefix: 0 words (Create != Update != Delete)
/// - Common suffix: 2 words (User, Response)
/// - Output: `["Create", "Update", "Delete"]`
///
/// **Shared prefix and suffix:**
/// - Input: `["UserCreateRequest", "UserUpdateRequest", "UserDeleteRequest"]`
/// - Common prefix: 1 word (User)
/// - Common suffix: 1 word (Request)
/// - Output: `["Create", "Update", "Delete"]`
///
/// **No change (would create duplicates):**
/// - Input: `["GetUserRequest", "GetUserResponse"]`
/// - Common prefix: 2 words (Get, User)
/// - Common suffix: 0 words
/// - Stripped: `["Request", "Response"]` - valid, so applied
///
/// **No change (would empty a name):**
/// - Input: `["User", "UserProfile"]`
/// - Common prefix: 1 word (User)
/// - Stripping would empty the first variant, so no changes applied
pub fn strip_common_affixes(variants: &mut [VariantDef]) {
  if variants.len() < 2 {
    return;
  }

  let word_segments: Vec<Vec<String>> = variants
    .iter()
    .map(|v| split_pascal_case(&v.name.to_string()))
    .collect();
  let first = &word_segments[0];
  let rest = &word_segments[1..];

  let common_prefix_len = common_prefix_len(first, rest);
  let common_suffix_len = common_suffix_len(first, rest);

  let stripped_names: Vec<String> = word_segments
    .iter()
    .map(|segments| extract_middle_segments(segments, common_prefix_len, common_suffix_len))
    .collect();

  if !all_non_empty_and_unique(&stripped_names) {
    return;
  }

  for (variant, new_name) in variants.iter_mut().zip(stripped_names) {
    variant.name = EnumVariantToken::from(new_name);
  }
}

/// Joins word segments after stripping common prefix and suffix.
///
/// Returns the full joined string if stripping would produce an empty result.
///
/// # Example
/// ```text
/// segments = ["User", "Create", "Response"], prefix_len = 1, suffix_len = 1
/// => "Create"
///
/// segments = ["Response"], prefix_len = 1, suffix_len = 0
/// => "Response" (unchanged, stripping would empty it)
/// ```
///
/// # Complexity
/// O(k) where k = number of segments joined.
#[must_use]
fn extract_middle_segments(segments: &[String], prefix_len: usize, suffix_len: usize) -> String {
  let end_idx = segments.len().saturating_sub(suffix_len);
  if prefix_len < end_idx {
    segments[prefix_len..end_idx].join("")
  } else {
    segments.join("")
  }
}

/// Returns true if all strings are non-empty and unique.
///
/// Used to validate that affix stripping produces valid variant names.
///
/// # Example
/// ```text
/// ["Create", "Update", "Delete"] => true
/// ["Create", "Create"]           => false (duplicate)
/// ["Create", ""]                 => false (empty)
/// ```
///
/// # Complexity
/// O(n log n) where n = number of names (BTreeSet insertion).
#[must_use]
fn all_non_empty_and_unique(names: &[String]) -> bool {
  if names.iter().any(String::is_empty) {
    return false;
  }
  let unique: BTreeSet<&String> = names.iter().collect();
  unique.len() == names.len()
}

/// Derives method names for multiple enum variants, ensuring they remain unique
/// after filtering out common words with the enum name.
///
/// Algorithm:
/// 1. Split enum name into words (e.g., `"MyEnum"` -> `["My", "Enum"]`).
/// 2. For each variant, split into words.
/// 3. Remove words present in the enum name from the variant name.
/// 4. If the result is unique and non-empty, use it.
/// 5. Otherwise, fall back to the full variant name (snake_cased).
pub(crate) fn derive_method_names(enum_name: &str, variant_names: &[String]) -> Vec<String> {
  if variant_names.is_empty() {
    return vec![];
  }

  let enum_words: BTreeSet<_> = split_pascal_case(enum_name).into_iter().collect();

  let candidates: Vec<(String, String)> = variant_names
    .iter()
    .map(|variant_name| {
      let variant_words = split_pascal_case(variant_name);
      let unique_words: Vec<_> = variant_words
        .iter()
        .filter(|word| !enum_words.contains(*word))
        .cloned()
        .collect();

      let original = variant_name.to_snake_case();
      let simplified = if unique_words.is_empty() {
        original.clone()
      } else {
        unique_words.join("").to_snake_case()
      };
      (original, simplified)
    })
    .collect();

  let mut simplified_counts = BTreeMap::new();
  for (_, simplified) in &candidates {
    *simplified_counts.entry(simplified.clone()).or_insert(0) += 1;
  }

  candidates
    .into_iter()
    .map(|(original, simplified)| {
      if simplified_counts[&simplified] > 1 {
        original
      } else {
        simplified
      }
    })
    .collect()
}

/// Infers a name for an inline schema based on its context (path, operation).
///
/// Checks in order: title, single property name, path segments.
pub(crate) fn infer_name_from_context(schema: &ObjectSchema, path: &str, context: &str) -> String {
  let is_request = context == REQUEST_BODY_SUFFIX;

  let with_suffix = |base: &str| {
    let sanitized_base = sanitize(base);
    if is_request {
      format!("{sanitized_base}{REQUEST_BODY_SUFFIX}")
    } else {
      format!("{sanitized_base}{RESPONSE_SUFFIX}")
    }
  };

  let with_context_suffix = |base: &str| {
    let sanitized_base = sanitize(base);
    if is_request {
      format!("{sanitized_base}{REQUEST_BODY_SUFFIX}")
    } else {
      format!("{sanitized_base}{context}{RESPONSE_SUFFIX}")
    }
  };

  if let Some(title) = &schema.title {
    return with_suffix(title);
  }

  if schema.properties.len() == 1
    && let Some((prop_name, _)) = schema.properties.iter().next()
  {
    let singular = cruet::to_singular(prop_name);
    return with_suffix(&singular);
  }

  let segments: Vec<_> = path
    .split('/')
    .filter(|s| !s.is_empty() && !s.starts_with('{'))
    .collect();

  segments
    .last()
    .map(|&s| with_context_suffix(&cruet::to_singular(s)))
    .or_else(|| segments.first().map(|&s| with_context_suffix(s)))
    .unwrap_or_else(|| {
      if is_request {
        REQUEST_BODY_SUFFIX.to_string()
      } else {
        format!("{RESPONSE_PREFIX}{context}")
      }
    })
}

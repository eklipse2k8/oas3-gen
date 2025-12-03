use std::collections::{BTreeMap, BTreeSet};

use inflections::Inflect;
use oas3::spec::{ObjectOrReference, ObjectSchema, SchemaType};

use crate::generator::{
  ast::{EnumVariantToken, VariantDef},
  converter::{SchemaExt, hashing},
  naming::{
    constants::{REQUEST_BODY_SUFFIX, RESPONSE_PREFIX, RESPONSE_SUFFIX},
    identifiers::{FORBIDDEN_IDENTIFIERS, ensure_unique, sanitize, split_pascal_case, to_rust_type_name},
  },
  schema_registry::{ReferenceExtractor, SchemaRegistry},
};

const RESERVED_TYPE_NAMES: &[&str] = &["Enum", "Struct", "Type", "Object"];

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
  let ref_names: Vec<String> = variants
    .iter()
    .filter_map(ReferenceExtractor::extract_ref_name_from_obj_ref)
    .collect();

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
  schema.is_string() && schema.enum_values.is_empty() && schema.const_value.is_none()
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
/// O(n) where n = number of variants (two passes).
fn has_mixed_string_variants<'a>(variants: impl Iterator<Item = &'a ObjectOrReference<ObjectSchema>>) -> bool {
  let variants: Vec<_> = variants.collect();

  if variants.is_empty() {
    return false;
  }

  let has_freeform = variants
    .iter()
    .any(|v| matches!(v, ObjectOrReference::Object(s) if is_freeform_string(s)));
  let has_constrained = variants
    .iter()
    .any(|v| matches!(v, ObjectOrReference::Object(s) if is_constrained(s)));

  has_freeform && has_constrained
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
  if let Some(typ) = schema.single_type() {
    return match typ {
      SchemaType::String => "String".to_string(),
      SchemaType::Number => "Number".to_string(),
      SchemaType::Integer => "Integer".to_string(),
      SchemaType::Boolean => "Boolean".to_string(),
      SchemaType::Array => "Array".to_string(),
      SchemaType::Object => "Object".to_string(),
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
/// Used when a schema doesn't have a title or ref name.
pub(crate) fn infer_name_from_context(schema: &ObjectSchema, path: &str, context: &str) -> String {
  let is_request = context == REQUEST_BODY_SUFFIX;

  let with_suffix = |base: &str| {
    let sanitized_base = sanitize(base);
    if is_request {
      format!("{sanitized_base}{REQUEST_BODY_SUFFIX}")
    } else {
      format!("{sanitized_base}{context}{RESPONSE_SUFFIX}")
    }
  };

  if schema.properties.len() == 1
    && let Some((prop_name, _)) = schema.properties.iter().next()
  {
    let singular = cruet::to_singular(prop_name);
    let sanitized_singular = sanitize(&singular);
    return if is_request {
      sanitized_singular
    } else {
      format!("{sanitized_singular}{RESPONSE_SUFFIX}")
    };
  }

  let segments: Vec<_> = path
    .split('/')
    .filter(|s| !s.is_empty() && !s.starts_with('{'))
    .collect();

  segments
    .last()
    .map(|&s| with_suffix(&cruet::to_singular(s)))
    .or_else(|| segments.first().map(|&s| with_suffix(s)))
    .unwrap_or_else(|| {
      if is_request {
        REQUEST_BODY_SUFFIX.to_string()
      } else {
        format!("{RESPONSE_PREFIX}{context}")
      }
    })
}

/// Scans the schema graph to discover and name inline types (enums, objects) ahead of time.
///
/// This helps to avoid name collisions and ensures consistent naming for reused inline schemas.
pub(crate) struct InlineTypeScanner<'a> {
  graph: &'a SchemaRegistry,
}

#[derive(Default)]
pub(crate) struct ScanResult {
  pub(crate) names: BTreeMap<String, String>,
  pub(crate) enum_names: BTreeMap<Vec<String>, String>,
}

impl<'a> InlineTypeScanner<'a> {
  pub(crate) fn new(graph: &'a SchemaRegistry) -> Self {
    Self { graph }
  }

  /// Scans the graph and computes unique names for all inline schemas.
  ///
  /// Returns a map of schema hash -> name, and enum values -> name.
  pub(crate) fn scan_and_compute_names(&self) -> anyhow::Result<ScanResult> {
    type NameCandidates = BTreeSet<(String, bool)>;

    let mut inline_schema_candidates: BTreeMap<String, NameCandidates> = BTreeMap::new();
    let mut enum_value_candidates: BTreeMap<Vec<String>, NameCandidates> = BTreeMap::new();

    self.collect_all_naming_candidates(&mut inline_schema_candidates, &mut enum_value_candidates)?;

    let mut used_names = self.get_existing_names();

    let final_enum_names = Self::resolve_enum_names(enum_value_candidates, &mut used_names);
    let final_schema_names = Self::resolve_schema_names(inline_schema_candidates, &mut used_names);

    Ok(ScanResult {
      names: final_schema_names,
      enum_names: final_enum_names,
    })
  }

  /// Iterates all schemas in the registry, collecting naming candidates for inline types.
  ///
  /// Populates both schema hash -> name candidates and enum values -> name candidates maps.
  /// Marks candidates from top-level schemas with `is_from_schema = true` for priority.
  ///
  /// # Complexity
  /// O(s * p) where s = schemas, p = properties per schema (recursive).
  fn collect_all_naming_candidates(
    &self,
    inline_schema_candidates: &mut BTreeMap<String, BTreeSet<(String, bool)>>,
    enum_value_candidates: &mut BTreeMap<Vec<String>, BTreeSet<(String, bool)>>,
  ) -> anyhow::Result<()> {
    for schema_name in self.graph.schema_names() {
      let Some(schema) = self.graph.get_schema(schema_name) else {
        continue;
      };

      if Self::is_inline_target(schema)
        && let Some(enum_values) = extract_enum_values(schema)
      {
        let mut rust_name = to_rust_type_name(schema_name);

        if Self::is_string_enum_optimizer_pattern(schema) {
          rust_name.push_str("Known");
        }

        let is_from_schema = true;
        enum_value_candidates
          .entry(enum_values)
          .or_default()
          .insert((rust_name, is_from_schema));
      }

      Self::collect_inline_candidates(schema_name, schema, inline_schema_candidates, enum_value_candidates)?;
    }

    Ok(())
  }

  /// Resolves final names for enums, selecting the best candidate for each enum value set.
  ///
  /// Updates `used_names` to prevent collisions with subsequently resolved names.
  ///
  /// # Complexity
  /// O(e * c) where e = enum sets, c = candidates per set.
  fn resolve_enum_names(
    enum_value_candidates: BTreeMap<Vec<String>, BTreeSet<(String, bool)>>,
    used_names: &mut BTreeSet<String>,
  ) -> BTreeMap<Vec<String>, String> {
    let mut final_enum_names = BTreeMap::new();

    for (enum_values, name_candidates) in enum_value_candidates {
      let best_name = Self::compute_best_name(&name_candidates, used_names);
      used_names.insert(best_name.clone());
      final_enum_names.insert(enum_values, best_name);
    }

    final_enum_names
  }

  /// Resolves final names for inline schemas by hash, selecting best candidate per hash.
  ///
  /// Updates `used_names` to prevent collisions with subsequently resolved names.
  ///
  /// # Complexity
  /// O(h * c) where h = schema hashes, c = candidates per hash.
  fn resolve_schema_names(
    inline_schema_candidates: BTreeMap<String, BTreeSet<(String, bool)>>,
    used_names: &mut BTreeSet<String>,
  ) -> BTreeMap<String, String> {
    let mut final_schema_names = BTreeMap::new();

    for (schema_hash, name_candidates) in inline_schema_candidates {
      let best_name = Self::compute_best_name(&name_candidates, used_names);
      used_names.insert(best_name.clone());
      final_schema_names.insert(schema_hash, best_name);
    }

    final_schema_names
  }

  /// Collects all existing schema names as Rust type names for collision detection.
  ///
  /// # Complexity
  /// O(n) where n = number of schemas in the registry.
  fn get_existing_names(&self) -> BTreeSet<String> {
    self
      .graph
      .schema_names()
      .iter()
      .map(|name| to_rust_type_name(name))
      .collect()
  }

  /// Recursively collects naming candidates from inline object properties and allOf schemas.
  ///
  /// For each inline schema, generates a candidate name from `{parent}{PropName}` and
  /// records both the schema hash and any enum values for deduplication.
  ///
  /// # Example
  /// ```text
  /// parent = "User", property = "status" with inline enum
  /// => candidate "UserStatus" added to both schema and enum candidate maps
  /// ```
  ///
  /// # Complexity
  /// O(p * d) where p = properties, d = nesting depth (recursive).
  fn collect_inline_candidates(
    parent_name: &str,
    schema: &ObjectSchema,
    candidates: &mut BTreeMap<String, BTreeSet<(String, bool)>>,
    enum_candidates: &mut BTreeMap<Vec<String>, BTreeSet<(String, bool)>>,
  ) -> anyhow::Result<()> {
    for (prop_name, prop_schema_ref) in &schema.properties {
      let prop_schema = match prop_schema_ref {
        ObjectOrReference::Ref { .. } => continue,
        ObjectOrReference::Object(s) => s,
      };

      if Self::is_inline_target(prop_schema) {
        let hash = hashing::hash_schema(prop_schema)?;
        let candidate_name = format!("{parent_name}{}", prop_name.to_pascal_case());
        let rust_name = to_rust_type_name(&candidate_name);

        candidates.entry(hash).or_default().insert((rust_name.clone(), false));

        if let Some(values) = extract_enum_values(prop_schema) {
          enum_candidates.entry(values).or_default().insert((rust_name, false));
        }
      }

      if !prop_schema.properties.is_empty() {
        let next_parent = format!("{parent_name}{}", prop_name.to_pascal_case());
        Self::collect_inline_candidates(&next_parent, prop_schema, candidates, enum_candidates)?;
      }
    }

    for sub in schema.all_of.iter().filter_map(|r| match r {
      ObjectOrReference::Object(s) => Some(s),
      ObjectOrReference::Ref { .. } => None,
    }) {
      Self::collect_inline_candidates(parent_name, sub, candidates, enum_candidates)?;
    }

    Ok(())
  }

  /// Detects the StringEnumOptimizer pattern: anyOf with freeform + constrained strings.
  ///
  /// These schemas get a "Known" suffix to distinguish the enum of known values.
  ///
  /// # Example
  /// ```text
  /// anyOf: [{ type: string }, { type: string, enum: ["a", "b"] }] => true
  /// oneOf: [{ type: string }, { type: string, enum: ["a", "b"] }] => false (oneOf, not anyOf)
  /// ```
  ///
  /// # Complexity
  /// O(n) where n = number of anyOf variants.
  fn is_string_enum_optimizer_pattern(schema: &ObjectSchema) -> bool {
    !schema.any_of.is_empty() && has_mixed_string_variants(schema.any_of.iter())
  }

  /// Returns true if schema should be tracked as an inline type candidate.
  ///
  /// Inline targets include: enums, oneOf/anyOf unions, and objects without additionalProperties.
  ///
  /// # Complexity
  /// O(1) - field access only.
  fn is_inline_target(schema: &ObjectSchema) -> bool {
    !schema.enum_values.is_empty()
      || !schema.one_of.is_empty()
      || !schema.any_of.is_empty()
      || (!schema.properties.is_empty() && schema.additional_properties.is_none())
  }

  /// Computes the best name for a set of candidates, avoiding collisions with used names.
  ///
  /// Strategy:
  /// 1. If any candidate is an explicit schema name (is_from_schema=true), prefer it.
  /// 2. If there's only one candidate, use it (uniquified).
  /// 3. If there are multiple, try to find a Longest Common Suffix (LCS).
  /// 4. If LCS is valid, use it.
  /// 5. Fallback to the first candidate.
  pub(crate) fn compute_best_name(candidates: &BTreeSet<(String, bool)>, used_names: &BTreeSet<String>) -> String {
    if let Some((name, _)) = candidates.iter().find(|(_, is_from_schema)| *is_from_schema) {
      return name.clone();
    }

    let candidate_vec: Vec<&String> = candidates.iter().map(|(n, _)| n).collect();
    if candidate_vec.is_empty() {
      return "UnknownType".to_string();
    }

    if candidate_vec.len() == 1 {
      return ensure_unique(candidate_vec[0], used_names);
    }

    let lcs = Self::longest_common_suffix(&candidate_vec);

    if Self::is_valid_common_name(&lcs) {
      let unique_lcs = ensure_unique(&lcs, used_names);
      return unique_lcs;
    }

    ensure_unique(candidate_vec[0], used_names)
  }

  /// Finds the longest common suffix among a set of strings.
  ///
  /// Example: `["CreateUserRequest", "UpdateUserRequest"]` -> `"UserRequest"`
  pub(crate) fn longest_common_suffix(strings: &[&String]) -> String {
    let [first, rest @ ..] = strings else {
      return String::new();
    };

    let first_chars: Vec<char> = first.chars().collect();
    let rest_chars: Vec<Vec<char>> = rest.iter().map(|s| s.chars().collect()).collect();

    let min_len = std::iter::once(first_chars.len())
      .chain(rest_chars.iter().map(Vec::len))
      .min()
      .unwrap_or(0);

    let suffix_len = (1..=min_len)
      .take_while(|&offset| {
        let ch = first_chars[first_chars.len() - offset];
        rest_chars.iter().all(|other| other[other.len() - offset] == ch)
      })
      .count();

    first_chars[first_chars.len() - suffix_len..].iter().collect()
  }

  /// Checks if a name is a valid common name (not reserved, follows conventions).
  pub(crate) fn is_valid_common_name(name: &str) -> bool {
    if name.len() < 4 {
      return false;
    }
    if RESERVED_TYPE_NAMES.contains(&name) {
      return false;
    }
    if !name.chars().next().is_some_and(char::is_uppercase) {
      return false;
    }
    if FORBIDDEN_IDENTIFIERS.contains(name) {
      return false;
    }

    true
  }
}

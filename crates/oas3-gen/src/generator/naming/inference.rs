use std::collections::{BTreeMap, BTreeSet};

use inflections::Inflect;
use oas3::spec::{ObjectOrReference, ObjectSchema, SchemaType, SchemaTypeSet};

use crate::generator::{
  converter::{
    constants::{REQUEST_BODY_SUFFIX, RESPONSE_PREFIX, RESPONSE_SUFFIX},
    hashing,
    type_resolver::TypeResolver,
  },
  naming::identifiers::{FORBIDDEN_IDENTIFIERS, sanitize, to_rust_type_name},
  schema_graph::SchemaGraph,
};

/// Scans the schema graph to discover and name inline types (enums, objects) ahead of time.
///
/// This helps to avoid name collisions and ensures consistent naming for reused inline schemas.
pub(crate) struct InlineTypeScanner<'a> {
  graph: &'a SchemaGraph,
  #[allow(dead_code)]
  type_resolver: TypeResolver<'a>,
}

#[derive(Default)]
pub(crate) struct ScanResult {
  pub(crate) names: BTreeMap<String, String>,
  pub(crate) enum_names: BTreeMap<Vec<String>, String>,
}

/// Checks if a schema matches the "relaxed enum" pattern.
///
/// A relaxed enum is defined as having a freeform string variant (no enum values, no const)
/// alongside other variants that are constrained (enum values or const).
pub(crate) fn is_relaxed_enum_pattern(schema: &ObjectSchema) -> bool {
  let variants: Vec<_> = schema.any_of.iter().chain(&schema.one_of).collect();

  if variants.is_empty() {
    return false;
  }

  let has_freeform_string = variants.iter().any(|variant| match variant {
    ObjectOrReference::Object(s) => {
      s.schema_type == Some(SchemaTypeSet::Single(SchemaType::String))
        && s.enum_values.is_empty()
        && s.const_value.is_none()
    }
    ObjectOrReference::Ref { .. } => false,
  });

  let has_constrained_variant = variants.iter().any(|variant| match variant {
    ObjectOrReference::Object(s) => !s.enum_values.is_empty() || s.const_value.is_some(),
    ObjectOrReference::Ref { .. } => false,
  });

  has_freeform_string && has_constrained_variant
}

/// Extracts enum values from a schema, handling standard enums, oneOf/anyOf patterns,
/// and relaxed enum patterns (mixed freeform string and constants).
///
/// Returns `None` if no valid enum values could be extracted.
pub(crate) fn extract_enum_values(schema: &ObjectSchema) -> Option<Vec<String>> {
  if !schema.enum_values.is_empty() {
    let string_values: Vec<_> = schema
      .enum_values
      .iter()
      .filter_map(|v| v.as_str().map(String::from))
      .collect();

    if !string_values.is_empty() {
      let mut sorted = string_values;
      sorted.sort();
      return Some(sorted);
    }
  }

  let variants: Vec<_> = schema.any_of.iter().chain(&schema.one_of).collect();

  if variants.is_empty() {
    return None;
  }

  let has_freeform_string = variants.iter().any(|variant| match variant {
    ObjectOrReference::Object(s) => {
      s.schema_type == Some(SchemaTypeSet::Single(SchemaType::String))
        && s.enum_values.is_empty()
        && s.const_value.is_none()
    }
    ObjectOrReference::Ref { .. } => false,
  });

  if has_freeform_string {
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

    return if values.is_empty() {
      None
    } else {
      Some(values.into_iter().collect())
    };
  }

  if !schema.one_of.is_empty() {
    let mut const_values = BTreeSet::new();

    for variant in &schema.one_of {
      match variant {
        ObjectOrReference::Object(s) => {
          if let Some(const_str) = s.const_value.as_ref().and_then(|v| v.as_str()) {
            const_values.insert(const_str.to_string());
          } else {
            return None;
          }
        }
        ObjectOrReference::Ref { .. } => return None,
      }
    }

    return if const_values.is_empty() {
      None
    } else {
      Some(const_values.into_iter().collect())
    };
  }

  None
}

impl<'a> InlineTypeScanner<'a> {
  pub(crate) fn new(graph: &'a SchemaGraph, type_resolver: TypeResolver<'a>) -> Self {
    Self { graph, type_resolver }
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

  fn get_existing_names(&self) -> BTreeSet<String> {
    self
      .graph
      .schema_names()
      .iter()
      .map(|name| to_rust_type_name(name))
      .collect()
  }

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

  fn is_string_enum_optimizer_pattern(schema: &ObjectSchema) -> bool {
    if schema.any_of.is_empty() {
      return false;
    }

    let has_freeform_string = schema.any_of.iter().any(|variant| match variant {
      ObjectOrReference::Object(s) => {
        s.schema_type == Some(SchemaTypeSet::Single(SchemaType::String))
          && s.enum_values.is_empty()
          && s.const_value.is_none()
      }
      ObjectOrReference::Ref { .. } => false,
    });

    let has_constrained_variant = schema.any_of.iter().any(|variant| match variant {
      ObjectOrReference::Object(s) => !s.enum_values.is_empty() || s.const_value.is_some(),
      ObjectOrReference::Ref { .. } => false,
    });

    has_freeform_string && has_constrained_variant
  }

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

    let first_reversed: Vec<char> = first.chars().rev().collect();

    let common_length = first_reversed
      .iter()
      .enumerate()
      .take_while(|(index, char_from_first)| {
        rest.iter().all(|other_string| {
          other_string
            .chars()
            .rev()
            .nth(*index)
            .is_some_and(|c| c == **char_from_first)
        })
      })
      .count();

    first_reversed.into_iter().take(common_length).rev().collect()
  }

  /// Checks if a name is a valid common name (not reserved, follows conventions).
  pub(crate) fn is_valid_common_name(name: &str) -> bool {
    if name.len() < 4 {
      return false;
    }
    if matches!(name, "Enum" | "Struct" | "Type" | "Object") {
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

/// Ensures a name is unique within a set of used names, appending a numeric suffix if needed.
pub(crate) fn ensure_unique(base_name: &str, used_names: &BTreeSet<String>) -> String {
  if !used_names.contains(base_name) {
    return base_name.to_string();
  }
  let mut i = 2;
  loop {
    let new_name = format!("{base_name}{i}");
    if !used_names.contains(&new_name) {
      return new_name;
    }
    i += 1;
  }
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

  // Pre-calculate candidates: (original_snake, simplified_snake)
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

  // Count occurrences of simplified names to detect collisions
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

/// Splits a PascalCase string into words.
/// Handles adjacent uppercase letters correctly (e.g., `"XMLParser"` -> `["XML", "Parser"]`).
pub(crate) fn split_pascal_case(name: &str) -> Vec<String> {
  if name.is_empty() {
    return vec![];
  }

  let mut words = vec![];
  let mut current_word = String::new();
  let chars: Vec<char> = name.chars().collect();

  for (i, &ch) in chars.iter().enumerate() {
    if ch.is_uppercase() && !current_word.is_empty() {
      // Check boundary conditions for splitting
      let prev_is_lower = i > 0 && chars[i - 1].is_lowercase();
      let next_is_lower = i + 1 < chars.len() && chars[i + 1].is_lowercase();

      if prev_is_lower || next_is_lower {
        words.push(std::mem::take(&mut current_word));
      }
    }
    current_word.push(ch);
  }

  if !current_word.is_empty() {
    words.push(current_word);
  }

  words
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

  // If schema has exactly one property, try to name it after that property
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

  // Otherwise, derive from path segments
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

use std::collections::{BTreeMap, BTreeSet, HashSet};

use inflections::Inflect;
use oas3::spec::{ObjectOrReference, ObjectSchema, SchemaType, SchemaTypeSet};

use super::{cache::SharedSchemaCache, type_resolver::TypeResolver};
use crate::{
  generator::schema_graph::SchemaGraph,
  reserved::{FORBIDDEN_IDENTIFIERS, to_rust_type_name},
};

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

impl<'a> InlineTypeScanner<'a> {
  pub(crate) fn new(graph: &'a SchemaGraph, type_resolver: TypeResolver<'a>) -> Self {
    Self { graph, type_resolver }
  }

  pub(crate) fn scan_and_compute_names(&self) -> anyhow::Result<ScanResult> {
    // Map<Hash, Set<CandidateName>>
    let mut candidates: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    // Map<EnumValues, Set<CandidateName>>
    let mut enum_candidates: BTreeMap<Vec<String>, BTreeSet<String>> = BTreeMap::new();

    // Iterate over all schemas in the graph
    for schema_name in self.graph.schema_names() {
      if let Some(schema) = self.graph.get_schema(schema_name) {
        // Also collect candidates from the root schema itself (e.g. if it's a named enum)
        if Self::is_inline_target(schema)
          && let Some(values) = Self::extract_enum_values(schema)
        {
          let rust_name = to_rust_type_name(schema_name);
          enum_candidates.entry(values).or_default().insert(rust_name);
        }

        Self::collect_inline_candidates(schema_name, schema, &mut candidates, &mut enum_candidates)?;
      }
    }

    let mut final_names = BTreeMap::new();
    let mut final_enum_names = BTreeMap::new();
    let mut used_names = self.get_existing_names();

    // Process enums first to claim best names
    for (values, name_candidates) in enum_candidates {
      let best_name = Self::compute_best_name(&name_candidates, &used_names);
      used_names.insert(best_name.clone());
      final_enum_names.insert(values, best_name);
    }

    // Process schema-based names
    for (hash, name_candidates) in candidates {
      // If this schema hash corresponds to an enum we already named, we could try to align them.
      // But for now, just compute independent best name (collision handling in ensure_unique will respect used_names)
      // If the simple enum is processed via handle_inline_enum, it will check get_enum_name first.
      let best_name = Self::compute_best_name(&name_candidates, &used_names);
      used_names.insert(best_name.clone());
      final_names.insert(hash, best_name);
    }

    Ok(ScanResult {
      names: final_names,
      enum_names: final_enum_names,
    })
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
    candidates: &mut BTreeMap<String, BTreeSet<String>>,
    enum_candidates: &mut BTreeMap<Vec<String>, BTreeSet<String>>,
  ) -> anyhow::Result<()> {
    // Check properties
    for (prop_name, prop_schema_ref) in &schema.properties {
      let prop_schema = match prop_schema_ref {
        ObjectOrReference::Ref { .. } => continue,
        ObjectOrReference::Object(s) => s,
      };

      if Self::is_inline_target(prop_schema) {
        let hash = SharedSchemaCache::hash_schema(prop_schema)?;
        let candidate_name = format!("{}{}", parent_name, prop_name.to_pascal_case());
        let rust_name = to_rust_type_name(&candidate_name);

        candidates.entry(hash).or_default().insert(rust_name.clone());

        if let Some(values) = Self::extract_enum_values(prop_schema) {
          enum_candidates.entry(values).or_default().insert(rust_name);
        }
      }

      // Recurse if it's an object
      if !prop_schema.properties.is_empty() {
        let next_parent = format!("{}{}", parent_name, prop_name.to_pascal_case());
        Self::collect_inline_candidates(&next_parent, prop_schema, candidates, enum_candidates)?;
      }
    }

    // Check all_of
    for sub_ref in &schema.all_of {
      if let ObjectOrReference::Object(sub) = sub_ref {
        Self::collect_inline_candidates(parent_name, sub, candidates, enum_candidates)?;
      }
    }

    // Check one_of/any_of
    for sub_ref in schema.one_of.iter().chain(schema.any_of.iter()) {
      if let ObjectOrReference::Object(_sub) = sub_ref {
        // Skip recursion
      }
    }

    Ok(())
  }

  fn extract_enum_values(schema: &ObjectSchema) -> Option<Vec<String>> {
    if !schema.enum_values.is_empty() {
      let mut values: Vec<String> = schema
        .enum_values
        .iter()
        .filter_map(|v| v.as_str().map(String::from))
        .collect();
      values.sort();
      if !values.is_empty() {
        return Some(values);
      }
    }

    // Check for relaxed string enum pattern in anyOf/oneOf
    // (String + Enum)
    let has_string = schema.any_of.iter().chain(schema.one_of.iter()).any(|v| {
           matches!(v, ObjectOrReference::Object(s) if s.schema_type == Some(SchemaTypeSet::Single(SchemaType::String)) && s.enum_values.is_empty())
      });

    if has_string {
      let mut values = HashSet::new();
      for variant in schema.any_of.iter().chain(schema.one_of.iter()) {
        if let ObjectOrReference::Object(s) = variant {
          for val in &s.enum_values {
            if let Some(str_val) = val.as_str() {
              values.insert(str_val.to_string());
            }
          }
          if let Some(const_val) = s.const_value.as_ref().and_then(|v| v.as_str()) {
            values.insert(const_val.to_string());
          }
        }
      }
      if !values.is_empty() {
        let mut sorted: Vec<_> = values.into_iter().collect();
        sorted.sort();
        return Some(sorted);
      }
    }

    None
  }
  fn is_inline_target(schema: &ObjectSchema) -> bool {
    // Check for inline enum
    if !schema.enum_values.is_empty() {
      return true;
    }

    // Check for inline union
    if !schema.one_of.is_empty() || !schema.any_of.is_empty() {
      // Basic heuristic: if it's complex enough to need a name
      return true;
    }

    // Check for inline struct (nested object)
    // Matches logic in StructConverter::is_inline_struct
    // If it has properties and isn't just a map
    if !schema.properties.is_empty() && schema.additional_properties.is_none() {
      return true;
    }

    false
  }
  fn compute_best_name(candidates: &BTreeSet<String>, used_names: &BTreeSet<String>) -> String {
    // 1. Convert to vector for indexing
    let candidate_vec: Vec<&String> = candidates.iter().collect();
    if candidate_vec.is_empty() {
      return "UnknownType".to_string(); // Should not happen
    }

    // 2. If only one candidate, use it (handling collisions)
    if candidate_vec.len() == 1 {
      let name = candidate_vec[0];
      if !used_names.contains(name) || candidates.contains(name) {
        return name.clone();
      }
      return ensure_unique(name, used_names);
    }

    // Prioritize candidates that match existing schema names (e.g. "Status" vs "InlineStatus")
    // If we have a candidate that is already "used" (and it is in our candidate set), it means
    // this group includes that named schema, so we should use that name.
    for candidate in &candidate_vec {
      if used_names.contains(*candidate) {
        return (*candidate).clone();
      }
    }

    // 3. Try to find Longest Common Suffix      // "CreateUserRole", "UpdateUserRole" -> "UserRole"
    // "ABStatus", "CDStatus" -> "Status"

    let lcs = Self::longest_common_suffix(&candidate_vec);

    // Heuristic: Common suffix must be at least X chars and not just "Enum" or generic
    // And it should probably start with an uppercase letter (it will if inputs are PascalCase)

    if Self::is_valid_common_name(&lcs) {
      // Check if the LCS itself is a valid name (not reserved, not empty)
      // And check uniqueness
      if !used_names.contains(&lcs) || candidates.contains(&lcs) {
        return lcs;
      }
      let unique_lcs = ensure_unique(&lcs, used_names);
      return unique_lcs;
    }

    // 4. Fallback: Pick the "first" candidate (alphabetically)
    // They are already sorted in BTreeSet
    let first = candidate_vec[0];
    if !used_names.contains(first) || candidates.contains(first) {
      return first.clone();
    }
    ensure_unique(first, used_names)
  }
  fn longest_common_suffix(strings: &[&String]) -> String {
    if strings.is_empty() {
      return String::new();
    }
    let first = strings[0];
    let mut longest_suffix = String::new();

    for i in 1..=first.len() {
      let suffix = &first[first.len() - i..];
      if strings.iter().all(|s| s.ends_with(suffix)) {
        longest_suffix = suffix.to_string();
      } else {
        break;
      }
    }
    longest_suffix
  }

  fn is_valid_common_name(name: &str) -> bool {
    if name.len() < 4 {
      return false;
    } // Too short

    if name == "Enum" || name == "Struct" || name == "Type" {
      return false;
    } // Too generic

    if !name.chars().next().unwrap().is_uppercase() {
      return false;
    } // Must be PascalCase

    if FORBIDDEN_IDENTIFIERS.contains(name) {
      return false;
    }

    true
  }
}

pub(crate) fn ensure_unique(base_name: &str, used_names: &BTreeSet<String>) -> String {
  if !used_names.contains(base_name) {
    return base_name.to_string();
  }

  // Collision resolution: append suffix
  // This is slightly imperfect because "Name2" might collide with an existing "Name2"
  // but we iterate
  let mut i = 2;
  loop {
    let new_name = format!("{base_name}{i}");
    if !used_names.contains(&new_name) {
      return new_name;
    }
    i += 1;
  }
}

#[cfg(test)]
mod tests {
  use std::collections::BTreeSet;

  use super::*;

  #[test]
  fn test_ensure_unique_handles_collisions() {
    let mut used = BTreeSet::new();
    used.insert("UserResponse".to_string());

    let result = ensure_unique("UserResponse", &used);

    assert_eq!(result, "UserResponse2");
  }

  #[test]
  fn test_ensure_unique_handles_multiple_collisions() {
    let mut used = BTreeSet::new();
    used.insert("UserResponse".to_string());
    used.insert("UserResponse2".to_string());
    used.insert("UserResponse3".to_string());

    let result = ensure_unique("UserResponse", &used);

    assert_eq!(result, "UserResponse4");
  }

  #[test]
  fn test_ensure_unique_with_empty_string() {
    let used = BTreeSet::new();
    let result = ensure_unique("", &used);
    assert_eq!(result, "");
  }

  #[test]
  fn test_ensure_unique_with_suffixed_collision() {
    let mut used = BTreeSet::new();
    used.insert("Name2".to_string());
    let result = ensure_unique("Name", &used);
    assert_eq!(result, "Name");
  }

  #[test]
  fn test_ensure_unique_no_collision() {
    let used = BTreeSet::new();
    let result = ensure_unique("UniqueName", &used);
    assert_eq!(result, "UniqueName");
  }

  #[test]
  fn test_ensure_unique_skips_to_available_suffix() {
    let mut used = BTreeSet::new();
    used.insert("Value".to_string());
    used.insert("Value3".to_string());
    let result = ensure_unique("Value", &used);
    assert_eq!(result, "Value2");
  }
}

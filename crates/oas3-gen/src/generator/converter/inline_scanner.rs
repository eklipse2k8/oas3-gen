use std::collections::{BTreeMap, BTreeSet};

use anyhow::Context;
use inflections::Inflect;
use oas3::spec::{Discriminator, ObjectOrReference, ObjectSchema, Schema, SchemaTypeSet, Spec};

use super::hashing;
use crate::generator::{
  naming::{
    identifiers::{FORBIDDEN_IDENTIFIERS, ensure_unique, to_rust_type_name},
    inference::{extract_enum_values, has_mixed_string_variants},
  },
  schema_registry::{MergedSchema, SchemaRegistry},
};

const RESERVED_TYPE_NAMES: &[&str] = &["Enum", "Struct", "Type", "Object"];

#[derive(Default)]
pub(crate) struct ScanResult {
  pub(crate) names: BTreeMap<String, String>,
  pub(crate) enum_names: BTreeMap<Vec<String>, String>,
}

pub(crate) struct InlineTypeScanner<'a> {
  graph: &'a SchemaRegistry,
}

impl<'a> InlineTypeScanner<'a> {
  pub(crate) fn new(graph: &'a SchemaRegistry) -> Self {
    Self { graph }
  }

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
    !schema.any_of.is_empty() && has_mixed_string_variants(schema.any_of.iter())
  }

  fn is_inline_target(schema: &ObjectSchema) -> bool {
    !schema.enum_values.is_empty()
      || !schema.one_of.is_empty()
      || !schema.any_of.is_empty()
      || (!schema.properties.is_empty() && schema.additional_properties.is_none())
  }

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

pub(crate) struct InlineSchemaMerger<'a> {
  spec: &'a Spec,
  merged_schemas: &'a BTreeMap<String, MergedSchema>,
}

impl<'a> InlineSchemaMerger<'a> {
  pub fn new(spec: &'a Spec, merged_schemas: &'a BTreeMap<String, MergedSchema>) -> Self {
    Self { spec, merged_schemas }
  }

  pub fn merge_inline(&self, schema: &ObjectSchema) -> anyhow::Result<ObjectSchema> {
    if schema.all_of.is_empty() {
      return Ok(schema.clone());
    }

    let mut merged_properties = BTreeMap::new();
    let mut merged_required = BTreeSet::new();
    let mut merged_discriminator: Option<Discriminator> = None;
    let mut merged_schema_type: Option<SchemaTypeSet> = None;
    let mut merged_additional = None;

    for all_of_ref in &schema.all_of {
      match all_of_ref {
        ObjectOrReference::Ref { ref_path, .. } => {
          if let Some(name) = SchemaRegistry::extract_ref_name(ref_path)
            && let Some(merged) = self.merged_schemas.get(&name)
          {
            Self::collect_from(
              &merged.schema,
              &mut merged_properties,
              &mut merged_required,
              &mut merged_discriminator,
              &mut merged_schema_type,
              &mut merged_additional,
            );
            continue;
          }

          let resolved = all_of_ref
            .resolve(self.spec)
            .context("Schema resolution failed for inline allOf reference")?;
          Self::collect_from(
            &resolved,
            &mut merged_properties,
            &mut merged_required,
            &mut merged_discriminator,
            &mut merged_schema_type,
            &mut merged_additional,
          );
        }
        ObjectOrReference::Object(inline) => {
          let inner_merged = self.merge_inline(inline)?;
          Self::collect_from(
            &inner_merged,
            &mut merged_properties,
            &mut merged_required,
            &mut merged_discriminator,
            &mut merged_schema_type,
            &mut merged_additional,
          );
        }
      }
    }

    Self::collect_from(
      schema,
      &mut merged_properties,
      &mut merged_required,
      &mut merged_discriminator,
      &mut merged_schema_type,
      &mut merged_additional,
    );

    let mut result = schema.clone();
    result.properties = merged_properties;
    result.required = merged_required.into_iter().collect();
    result.discriminator = merged_discriminator;
    if merged_schema_type.is_some() {
      result.schema_type = merged_schema_type;
    }
    result.all_of.clear();

    if result.additional_properties.is_none() {
      result.additional_properties = merged_additional;
    }

    Ok(result)
  }

  fn collect_from(
    source: &ObjectSchema,
    properties: &mut BTreeMap<String, ObjectOrReference<ObjectSchema>>,
    required: &mut BTreeSet<String>,
    discriminator: &mut Option<Discriminator>,
    schema_type: &mut Option<SchemaTypeSet>,
    additional_properties: &mut Option<Schema>,
  ) {
    for (name, prop) in &source.properties {
      properties.insert(name.clone(), prop.clone());
    }
    required.extend(source.required.iter().cloned());
    if source.discriminator.is_some() {
      discriminator.clone_from(&source.discriminator);
    }
    if source.schema_type.is_some() {
      schema_type.clone_from(&source.schema_type);
    }
    if additional_properties.is_none() {
      additional_properties.clone_from(&source.additional_properties);
    }
  }
}

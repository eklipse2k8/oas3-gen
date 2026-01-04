use std::collections::{BTreeMap, BTreeSet};

use inflections::Inflect;
use oas3::spec::{ObjectOrReference, ObjectSchema};

use super::{
  identifiers::{FORBIDDEN_IDENTIFIERS, ensure_unique, to_rust_type_name},
  inference::InferenceExt,
};
use crate::generator::{
  converter::{SchemaExt, hashing::CanonicalSchema},
  naming::constants::KNOWN_ENUM_VARIANT,
};

const RESERVED_TYPE_NAMES: &[&str] = &["Enum", "Struct", "Type", "Object"];

type NameCandidates = BTreeSet<(String, bool)>;

#[derive(Default)]
pub struct ScanResult {
  pub names: BTreeMap<CanonicalSchema, String>,
  pub enum_names: BTreeMap<Vec<String>, String>,
}

#[derive(Default)]
struct CandidateIndex {
  schemas: BTreeMap<CanonicalSchema, NameCandidates>,
  enums: BTreeMap<Vec<String>, NameCandidates>,
}

impl CandidateIndex {
  fn merge(mut self, other: Self) -> Self {
    for (key, candidates) in other.schemas {
      self.schemas.entry(key).or_default().extend(candidates);
    }
    for (key, candidates) in other.enums {
      self.enums.entry(key).or_default().extend(candidates);
    }
    self
  }

  fn add_schema_candidate(&mut self, canonical: CanonicalSchema, name: String, is_from_schema: bool) {
    self
      .schemas
      .entry(canonical)
      .or_default()
      .insert((name, is_from_schema));
  }

  fn add_enum_candidate(&mut self, values: Vec<String>, name: String, is_from_schema: bool) {
    self.enums.entry(values).or_default().insert((name, is_from_schema));
  }
}

pub struct TypeNameIndex<'a> {
  schemas: &'a BTreeMap<String, ObjectSchema>,
}

impl<'a> TypeNameIndex<'a> {
  pub fn new(schemas: &'a BTreeMap<String, ObjectSchema>) -> Self {
    Self { schemas }
  }

  pub fn scan_and_compute_names(&self) -> anyhow::Result<ScanResult> {
    let candidates = self.collect_all_candidates()?;
    let mut used_names = self.existing_rust_names();

    let enum_names = resolve_names(candidates.enums, &mut used_names);
    let names = resolve_names(candidates.schemas, &mut used_names);

    Ok(ScanResult { names, enum_names })
  }

  fn collect_all_candidates(&self) -> anyhow::Result<CandidateIndex> {
    self
      .schemas
      .iter()
      .try_fold(CandidateIndex::default(), |acc, (name, schema)| {
        let mut index = Self::collect_top_level_candidates(name, schema);
        let inline = collect_inline_candidates(name, schema)?;
        index = index.merge(inline);
        Ok(acc.merge(index))
      })
  }

  fn collect_top_level_candidates(schema_name: &str, schema: &ObjectSchema) -> CandidateIndex {
    let mut index = CandidateIndex::default();

    if schema.requires_type_definition()
      && let Some(enum_values) = schema.extract_enum_values()
    {
      let mut rust_name = to_rust_type_name(schema_name);
      if schema.has_relaxed_anyof_enum() {
        rust_name.push_str(KNOWN_ENUM_VARIANT);
      }
      index.add_enum_candidate(enum_values, rust_name, true);
    }

    index
  }

  fn existing_rust_names(&self) -> BTreeSet<String> {
    self.schemas.keys().map(|name| to_rust_type_name(name)).collect()
  }
}

fn collect_inline_candidates(parent_name: &str, schema: &ObjectSchema) -> anyhow::Result<CandidateIndex> {
  let mut index = CandidateIndex::default();

  for (prop_name, prop_schema_ref) in &schema.properties {
    let ObjectOrReference::Object(prop_schema) = prop_schema_ref else {
      continue;
    };

    let next_parent = format!("{parent_name}{}", prop_name.to_pascal_case());

    if prop_schema.requires_type_definition() {
      let canonical = CanonicalSchema::from_schema(prop_schema)?;
      let rust_name = to_rust_type_name(&next_parent);

      index.add_schema_candidate(canonical, rust_name.clone(), false);

      if let Some(values) = prop_schema.extract_enum_values() {
        index.add_enum_candidate(values, rust_name, false);
      }
    }

    if !prop_schema.properties.is_empty() {
      index = index.merge(collect_inline_candidates(&next_parent, prop_schema)?);
    }
  }

  for sub in schema.all_of.iter().filter_map(|r| match r {
    ObjectOrReference::Object(s) => Some(s),
    ObjectOrReference::Ref { .. } => None,
  }) {
    index = index.merge(collect_inline_candidates(parent_name, sub)?);
  }

  Ok(index)
}

fn resolve_names<K: Ord>(
  candidates: BTreeMap<K, NameCandidates>,
  used_names: &mut BTreeSet<String>,
) -> BTreeMap<K, String> {
  candidates
    .into_iter()
    .map(|(key, name_candidates)| {
      let best = compute_best_name(&name_candidates, used_names);
      used_names.insert(best.clone());
      (key, best)
    })
    .collect()
}

pub fn compute_best_name(candidates: &NameCandidates, used_names: &BTreeSet<String>) -> String {
  if let Some((name, _)) = candidates.iter().find(|(_, is_from_schema)| *is_from_schema) {
    return name.clone();
  }

  let candidate_vec: Vec<&String> = candidates.iter().map(|(n, _)| n).collect();

  match candidate_vec.as_slice() {
    [] => "UnknownType".to_string(),
    [single] => ensure_unique(single, used_names),
    _ => {
      let lcs = longest_common_suffix(&candidate_vec);
      if is_valid_common_name(&lcs) {
        ensure_unique(&lcs, used_names)
      } else {
        ensure_unique(candidate_vec[0], used_names)
      }
    }
  }
}

pub fn longest_common_suffix(strings: &[&String]) -> String {
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

pub fn is_valid_common_name(name: &str) -> bool {
  name.len() >= 4
    && !RESERVED_TYPE_NAMES.contains(&name)
    && name.starts_with(char::is_uppercase)
    && !FORBIDDEN_IDENTIFIERS.contains(name)
}

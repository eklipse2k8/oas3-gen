use std::{
  cmp::Reverse,
  collections::{BTreeSet, HashMap, HashSet},
};

use oas3::spec::{ObjectSchema, SchemaType, SchemaTypeSet};

use super::metadata::FieldMetadata;
use crate::{
  generator::{
    ast::{FieldDef, TypeRef, VariantDef},
    schema_graph::SchemaGraph,
  },
  reserved::to_rust_field_name,
};

#[derive(Copy, Clone)]
pub(crate) enum InlinePolicy {
  InlineUnions,
}

const STRUCT_DERIVES: &[&str] = &[
  "Debug",
  "Clone",
  "PartialEq",
  "validator::Validate",
  "oas3_gen_support::Default",
];

const SIMPLE_ENUM_DERIVES: &[&str] = &[
  "Debug",
  "Clone",
  "PartialEq",
  "Eq",
  "Hash",
  "Serialize",
  "Deserialize",
  "oas3_gen_support::Default",
];
const COMPLEX_ENUM_DERIVES: &[&str] = &[
  "Debug",
  "Clone",
  "PartialEq",
  "Serialize",
  "Deserialize",
  "oas3_gen_support::Default",
];

pub(crate) fn derives_for_struct(_all_read_only: bool, _all_write_only: bool) -> Vec<String> {
  STRUCT_DERIVES.iter().map(|s| (*s).to_string()).collect()
}

pub(crate) fn derives_for_enum(is_simple: bool) -> Vec<String> {
  let base = if is_simple {
    SIMPLE_ENUM_DERIVES
  } else {
    COMPLEX_ENUM_DERIVES
  };
  base.iter().map(|s| (*s).to_string()).collect()
}

pub(crate) fn container_outer_attrs(_fields: &[FieldDef]) -> Vec<String> {
  Vec::new()
}

pub(crate) fn is_discriminated_base_type(schema: &ObjectSchema) -> bool {
  schema
    .discriminator
    .as_ref()
    .and_then(|d| d.mapping.as_ref().map(|m| !m.is_empty()))
    .unwrap_or(false)
    && !schema.properties.is_empty()
}

pub(crate) fn extract_discriminator_children(
  graph: &SchemaGraph,
  schema: &ObjectSchema,
  reachable_filter: Option<&std::collections::BTreeSet<String>>,
) -> Vec<(String, String)> {
  let Some(mapping) = schema.discriminator.as_ref().and_then(|d| d.mapping.as_ref()) else {
    return vec![];
  };

  let mut children: Vec<_> = mapping
    .iter()
    .filter_map(|(val, ref_path)| SchemaGraph::extract_ref_name(ref_path).map(|name| (val.clone(), name)))
    .filter(|(_, name)| {
      if let Some(filter) = reachable_filter {
        filter.contains(name)
      } else {
        true
      }
    })
    .collect();

  let mut depth_memo = HashMap::new();
  children.sort_by_key(|(_, name)| Reverse(compute_inheritance_depth(graph, name, &mut depth_memo)));
  children
}

fn compute_inheritance_depth(graph: &SchemaGraph, schema_name: &str, memo: &mut HashMap<String, usize>) -> usize {
  if let Some(&depth) = memo.get(schema_name) {
    return depth;
  }
  let Some(schema) = graph.get_schema(schema_name) else {
    return 0;
  };

  let depth = if schema.all_of.is_empty() {
    0
  } else {
    schema
      .all_of
      .iter()
      .filter_map(SchemaGraph::extract_ref_name_from_ref)
      .map(|parent| compute_inheritance_depth(graph, &parent, memo))
      .max()
      .unwrap_or(0)
      + 1
  };

  memo.insert(schema_name.to_string(), depth);
  depth
}

pub(crate) fn infer_variant_name(schema: &ObjectSchema, index: usize) -> String {
  if !schema.enum_values.is_empty() {
    return "Enum".to_string();
  }
  if let Some(ref schema_type) = schema.schema_type {
    match schema_type {
      SchemaTypeSet::Single(typ) => match typ {
        SchemaType::String => "String".to_string(),
        SchemaType::Number => "Number".to_string(),
        SchemaType::Integer => "Integer".to_string(),
        SchemaType::Boolean => "Boolean".to_string(),
        SchemaType::Array => "Array".to_string(),
        SchemaType::Object => "Object".to_string(),
        SchemaType::Null => "Null".to_string(),
      },
      SchemaTypeSet::Multiple(_) => "Mixed".to_string(),
    }
  } else {
    format!("Variant{index}")
  }
}

pub(crate) fn strip_common_affixes(variants: &mut [VariantDef]) {
  let variant_names: Vec<_> = variants.iter().map(|v| v.name.clone()).collect();
  if variant_names.len() < 2 {
    return;
  }

  let split_names: Vec<Vec<String>> = variant_names.iter().map(|n| split_pascal_case(n)).collect();

  let common_prefix_len = find_common_prefix_len(&split_names);
  let common_suffix_len = find_common_suffix_len(&split_names);

  let mut stripped_names = Vec::new();
  for words in &split_names {
    let start = common_prefix_len;
    let end = words.len().saturating_sub(common_suffix_len);
    if start >= end {
      stripped_names.push(words.join(""));
    } else {
      stripped_names.push(words[start..end].join(""));
    }
  }

  let mut seen = BTreeSet::new();
  if stripped_names.iter().any(|n| n.is_empty() || !seen.insert(n)) {
    return;
  }

  for (variant, new_name) in variants.iter_mut().zip(stripped_names) {
    variant.name = new_name;
  }
}

fn find_common_prefix_len(split_names: &[Vec<String>]) -> usize {
  let Some(first) = split_names.first() else {
    return 0;
  };
  let mut len = 0;
  'outer: for (i, word) in first.iter().enumerate() {
    for other in &split_names[1..] {
      if other.get(i) != Some(word) {
        break 'outer;
      }
    }
    len = i + 1;
  }
  len
}

fn find_common_suffix_len(split_names: &[Vec<String>]) -> usize {
  let Some(first) = split_names.first() else {
    return 0;
  };
  let mut len = 0;
  'outer: for i in 1..=first.len() {
    let word = &first[first.len() - i];
    for other in &split_names[1..] {
      if other.len() < i || &other[other.len() - i] != word {
        break 'outer;
      }
    }
    len = i;
  }
  len
}

fn split_pascal_case(name: &str) -> Vec<String> {
  let mut words = Vec::new();
  let mut current_word = String::new();
  for (i, ch) in name.chars().enumerate() {
    if ch.is_uppercase() && i > 0 && !current_word.is_empty() {
      words.push(std::mem::take(&mut current_word));
    }
    current_word.push(ch);
  }
  if !current_word.is_empty() {
    words.push(current_word);
  }
  words
}

pub(crate) fn unique_variant_name(base_name: &str, index: usize, seen_names: &mut BTreeSet<String>) -> String {
  if seen_names.insert(base_name.to_string()) {
    base_name.to_string()
  } else {
    let new_name = format!("{base_name}{index}");
    seen_names.insert(new_name.clone());
    new_name
  }
}

pub(crate) fn serde_renamed_if_needed(prop_name: &str) -> Vec<String> {
  let rust_field_name = to_rust_field_name(prop_name);
  if rust_field_name == prop_name {
    vec![]
  } else {
    vec![format!(r#"rename = "{}""#, prop_name)]
  }
}

pub(crate) fn apply_optionality(rust_type: TypeRef, is_optional: bool) -> TypeRef {
  if is_optional && !rust_type.nullable {
    rust_type.with_option()
  } else {
    rust_type
  }
}

pub(crate) fn deduplicate_field_names(fields: &mut Vec<FieldDef>) {
  let mut name_counts: HashMap<String, usize> = HashMap::new();
  for field in &*fields {
    *name_counts.entry(field.name.clone()).or_default() += 1;
  }

  let mut indices_to_remove = HashSet::<usize>::new();
  for (name, _count) in name_counts.into_iter().filter(|(_, c)| *c > 1) {
    let colliding_indices: Vec<_> = fields
      .iter()
      .enumerate()
      .filter(|(_, f)| f.name == name)
      .map(|(i, _)| i)
      .collect();

    let (deprecated, non_deprecated): (Vec<&usize>, Vec<&usize>) =
      colliding_indices.iter().partition(|&&i| fields[i].deprecated);

    if !deprecated.is_empty() && !non_deprecated.is_empty() {
      indices_to_remove.extend(deprecated);
    } else {
      for (i, &idx) in colliding_indices.iter().enumerate().skip(1) {
        fields[idx].name = format!("{name}_{}", i + 1);
      }
    }
  }

  if !indices_to_remove.is_empty() {
    let mut i = 0;
    fields.retain(|_| {
      let keep = !indices_to_remove.contains(&i);
      i += 1;
      keep
    });
  }
}

pub(crate) fn build_field_def(
  prop_name: &str,
  rust_type: TypeRef,
  serde_attrs: Vec<String>,
  metadata: FieldMetadata,
  regex_validation: Option<String>,
  extra_attrs: Vec<String>,
) -> FieldDef {
  FieldDef {
    name: to_rust_field_name(prop_name),
    docs: metadata.docs,
    rust_type,
    serde_attrs,
    extra_attrs,
    validation_attrs: metadata.validation_attrs,
    regex_validation,
    default_value: metadata.default_value,
    read_only: metadata.read_only,
    write_only: metadata.write_only,
    deprecated: metadata.deprecated,
    multiple_of: metadata.multiple_of,
  }
}

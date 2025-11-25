use std::collections::BTreeSet;

use oas3::spec::{ObjectSchema, SchemaType, SchemaTypeSet};

use crate::generator::{ast::VariantDef, naming::identifiers::to_rust_type_name};

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
      serde_json::Value::Number(num) if num.is_i64() => {
        let num_val = num.as_i64().unwrap();
        Some(NormalizedVariant {
          name: format!("Value{num_val}"),
          rename_value: num_val.to_string(),
        })
      }
      serde_json::Value::Number(num) if num.is_f64() => {
        let num_val = num.as_f64().unwrap();
        let raw_str = num_val.to_string();
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
#[inline]
pub fn infer_variant_name(schema: &ObjectSchema, index: usize) -> String {
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

/// Strips common prefixes and suffixes from variant names to make them concise.
///
/// Useful for generated union variants that might share long names like
/// `CreateUserResponse`, `UpdateUserResponse` -> `Create`, `Update`.
pub fn strip_common_affixes(variants: &mut [VariantDef]) {
  let variant_names: Vec<_> = variants.iter().map(|v| v.name.clone()).collect();
  if variant_names.len() < 2 {
    return;
  }

  let split_names: Vec<Vec<String>> = variant_names.iter().map(|n| split_pascal_case(n)).collect();

  let common_prefix_len = find_common_prefix_len(&split_names);
  let common_suffix_len = find_common_suffix_len(&split_names);

  let mut stripped_names = vec![];
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

#[must_use]
#[inline]
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

#[must_use]
#[inline]
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

#[must_use]
#[inline]
fn split_pascal_case(name: &str) -> Vec<String> {
  let mut words = vec![];
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

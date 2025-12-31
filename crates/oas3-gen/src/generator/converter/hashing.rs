use std::{cmp::Ordering, hash::Hash};

use anyhow::Context;
use json_canon::to_string as to_canonical_json;
use oas3::spec::ObjectSchema;
use serde_json::Value;

/// Opaque representation of a schema's canonical form.
///
/// Used for caching and deduplication of generated types.
/// Normalizes fields like `required`, `type`, `enum` to ensure semantically
/// identical schemas produce the same canonical representation.
#[derive(Debug, Clone, Eq)]
pub struct CanonicalSchema(String);

impl CanonicalSchema {
  pub fn from_schema(schema: &ObjectSchema) -> anyhow::Result<Self> {
    let mut value = serde_json::to_value(schema).context("Failed to serialize schema for canonicalization")?;

    normalize_schema_semantics(&mut value);

    let canonical_json = to_canonical_json(&value).context("Failed to create canonical JSON string")?;

    Ok(CanonicalSchema(canonical_json))
  }
}

impl PartialEq for CanonicalSchema {
  fn eq(&self, other: &Self) -> bool {
    self.0 == other.0
  }
}

impl PartialOrd for CanonicalSchema {
  fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
    Some(self.cmp(other))
  }
}

impl Ord for CanonicalSchema {
  fn cmp(&self, other: &Self) -> Ordering {
    self.0.cmp(&other.0)
  }
}

impl Hash for CanonicalSchema {
  fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
    let hash = blake3::hash(self.0.as_bytes());
    hash.as_bytes().hash(state);
  }
}

/// Normalizes a JSON schema `Value` in-place to ensure that
/// semantically identical schemas produce the same hash.
///
/// This function specifically handles fields where order does not
/// matter, like the `required` and `type` arrays.
fn normalize_schema_semantics(value: &mut Value) {
  match value {
    Value::Object(map) => {
      if let Some(Value::Array(arr)) = map.get_mut("required") {
        sort_string_array_in_place(arr);
      }

      if let Some(Value::Array(arr)) = map.get_mut("type") {
        sort_string_array_in_place(arr);
      }

      if let Some(Value::Array(arr)) = map.get_mut("enum") {
        sort_string_array_in_place(arr);
      }

      for value in map.values_mut() {
        normalize_schema_semantics(value);
      }
    }
    Value::Array(arr) => {
      for item in arr {
        normalize_schema_semantics(item);
      }
    }
    _ => {}
  }
}

/// Helper to sort a `Vec<Value>` in-place, if and only if
/// it contains entirely string elements.
fn sort_string_array_in_place(arr: &mut Vec<Value>) {
  let mut strings: Vec<String> = arr.iter().filter_map(|v| v.as_str().map(String::from)).collect();

  if strings.len() == arr.len() {
    strings.sort_unstable();
    *arr = strings.into_iter().map(Value::String).collect();
  }
}

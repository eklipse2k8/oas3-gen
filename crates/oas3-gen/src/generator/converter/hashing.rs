use anyhow::Context;
use blake3::Hasher;
use json_canon::to_string as to_canonical_json;
use oas3::spec::ObjectSchema;
use serde_json::Value;

use super::ConversionResult;

/// Computes a deterministic hash of an `ObjectSchema`.
///
/// Used for caching generated types to avoid duplication.
/// Normalizes fields like `required`, `type`, `enum` to ensure consistent hashing.
pub(crate) fn hash_schema(schema: &ObjectSchema) -> ConversionResult<String> {
  let mut value = serde_json::to_value(schema).context("Failed to serialize schema for hashing")?;

  normalize_schema_semantics(&mut value);

  let canonical_json = to_canonical_json(&value).context("Failed to create canonical JSON string")?;

  let mut hasher = Hasher::new();
  hasher.update(canonical_json.as_bytes());
  let hash = hasher.finalize();

  Ok(hash.to_hex().to_string())
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

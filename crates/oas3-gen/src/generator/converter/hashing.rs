use std::{cmp::Ordering, hash::Hash};

use anyhow::Context;
use json_canon::to_string as to_canonical_json;
use oas3::spec::ObjectSchema;
use serde_json::{Number, Value};

/// Opaque representation of a schema's canonical form.
///
/// Used for caching and deduplication of generated types.
/// Normalizes fields like `required`, `type`, `enum` to ensure semantically
/// identical schemas produce the same canonical representation.
#[derive(Debug, Clone, Eq)]
pub struct CanonicalSchema(String);

impl CanonicalSchema {
  /// Creates a canonical representation of an OpenAPI schema for cache-key equality.
  ///
  /// Serializes the schema to JSON, normalizes order-independent arrays (`required`,
  /// `type`, `enum`) by sorting them alphabetically, then converts to RFC 8785
  /// canonical JSON. Two schemas that differ only in array element ordering or
  /// JSON key ordering will produce identical `CanonicalSchema` values.
  pub fn from_schema(schema: &ObjectSchema) -> anyhow::Result<Self> {
    let mut value = serde_json::to_value(schema).context("Failed to serialize schema for canonicalization")?;

    normalize_schema_semantics(&mut value);

    let canonical_json = to_canonical_json(&value).context("Failed to create canonical JSON string")?;

    Ok(CanonicalSchema(canonical_json))
  }
}

impl PartialEq for CanonicalSchema {
  /// Compares two canonical schemas for equality by their canonical JSON string representation.
  fn eq(&self, other: &Self) -> bool {
    self.0 == other.0
  }
}

impl PartialOrd for CanonicalSchema {
  /// Provides partial ordering by delegating to the total ordering implementation.
  fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
    Some(self.cmp(other))
  }
}

impl Ord for CanonicalSchema {
  /// Orders canonical schemas lexicographically by their canonical JSON string representation.
  fn cmp(&self, other: &Self) -> Ordering {
    self.0.cmp(&other.0)
  }
}

impl Hash for CanonicalSchema {
  /// Computes a BLAKE3 hash of the canonical JSON string and feeds it to the hasher.
  ///
  /// Uses BLAKE3 for fast, collision-resistant hashing of potentially large schema
  /// representations.
  fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
    let hash = blake3::hash(self.0.as_bytes());
    hash.as_bytes().hash(state);
  }
}

/// Normalizes JSON Schema values for canonical comparison.
///
/// Recursively traverses the JSON value and:
/// - Alphabetically sorts order-independent arrays (`required`, `type`, `enum`)
/// - Clamps large integers to the IEEE 754 safe integer range
///
/// This ensures schemas like `{"required": ["b", "a"]}` and `{"required": ["a", "b"]}`
/// produce identical canonical representations, and schemas with large numbers
/// (outside ±2^53) can be canonicalized without RFC 8785 errors.
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
    Value::Number(n) => {
      if let Some(clamped) = clamp_number_to_safe_range(n) {
        *value = Value::Number(clamped);
      }
    }
    _ => {}
  }
}

const MAX_SAFE_INTEGER: i64 = 9_007_199_254_740_991;
const MIN_SAFE_INTEGER: i64 = -9_007_199_254_740_991;

/// Clamps a JSON number to the IEEE 754 safe integer range (±2^53).
///
/// RFC 8785 (JSON Canonicalization Scheme) requires numbers to be representable
/// as IEEE 754 double-precision floats. Integers outside the range ±2^53 cannot
/// be exactly represented and will cause canonicalization to fail.
///
/// Returns `Some(clamped_number)` if the number was outside the safe range,
/// or `None` if no clamping was needed.
#[allow(clippy::cast_precision_loss)]
fn clamp_number_to_safe_range(n: &serde_json::Number) -> Option<serde_json::Number> {
  if let Some(i) = n.as_i64() {
    return match i {
      i if i > MAX_SAFE_INTEGER => Some(MAX_SAFE_INTEGER.into()),
      i if i < MIN_SAFE_INTEGER => Some(MIN_SAFE_INTEGER.into()),
      _ => None,
    };
  }

  if let Some(u) = n.as_u64() {
    return (u > MAX_SAFE_INTEGER as u64).then_some(MAX_SAFE_INTEGER.into());
  }

  if let Some(f) = n.as_f64() {
    let (min_f, max_f) = (MIN_SAFE_INTEGER as f64, MAX_SAFE_INTEGER as f64);
    if f < min_f || f > max_f {
      return Number::from_f64(f.clamp(min_f, max_f));
    }
  }

  None
}

/// Sorts a JSON array in-place if all elements are strings; otherwise leaves it unchanged.
///
/// Extracts string values, sorts them alphabetically, and reconstructs the array.
/// Arrays containing any non-string elements (numbers, objects, etc.) are preserved
/// in their original order to avoid corrupting schema structures like `oneOf` or `anyOf`.
fn sort_string_array_in_place(arr: &mut Vec<Value>) {
  let mut strings: Vec<String> = arr.iter().filter_map(|v| v.as_str().map(String::from)).collect();

  if strings.len() == arr.len() {
    strings.sort_unstable();
    *arr = strings.into_iter().map(Value::String).collect();
  }
}

use std::collections::{HashMap, HashSet};

use anyhow::Context;
use blake3::Hasher;
use json_canon::to_string as to_canonical_json;
use oas3::spec::ObjectSchema;
use serde_json::Value;

use super::{REQUEST_BODY_SUFFIX, RESPONSE_PREFIX, RESPONSE_SUFFIX, SchemaConverter, error::ConversionResult};
use crate::{
  generator::ast::{RustType, StructKind},
  reserved::to_rust_type_name,
};

pub(crate) struct SharedSchemaCache {
  schema_to_type: HashMap<String, String>,
  generated_types: Vec<RustType>,
  used_names: HashSet<String>,
}

impl SharedSchemaCache {
  pub(crate) fn new() -> Self {
    Self {
      schema_to_type: HashMap::new(),
      generated_types: Vec::new(),
      used_names: HashSet::new(),
    }
  }

  pub(crate) fn get_or_create_type(
    &mut self,
    schema: &ObjectSchema,
    converter: &SchemaConverter,
    path: &str,
    context: &str,
    kind: StructKind,
  ) -> ConversionResult<String> {
    let schema_hash = Self::hash_schema(schema)?;

    if let Some(existing_type) = self.schema_to_type.get(&schema_hash) {
      return Ok(existing_type.clone());
    }

    let base_name = Self::infer_name_from_context(schema, path, context);
    let type_name = self.make_unique_name(base_name);
    let rust_type_name = to_rust_type_name(&type_name);

    let (body_struct, mut nested_types) = converter.convert_struct(&type_name, schema, Some(kind))?;

    self.generated_types.append(&mut nested_types);
    self.generated_types.push(body_struct);
    self.schema_to_type.insert(schema_hash, rust_type_name.clone());
    self.used_names.insert(rust_type_name.clone());

    Ok(rust_type_name)
  }

  fn infer_name_from_context(schema: &ObjectSchema, path: &str, context: &str) -> String {
    let is_request = context == REQUEST_BODY_SUFFIX;

    let with_suffix = |base: &str| {
      if is_request {
        format!("{base}{REQUEST_BODY_SUFFIX}")
      } else {
        format!("{base}{context}{RESPONSE_SUFFIX}")
      }
    };

    if schema.properties.len() == 1
      && let Some((prop_name, _)) = schema.properties.iter().next()
    {
      let singular = cruet::to_singular(prop_name);
      return if is_request {
        singular
      } else {
        format!("{singular}{RESPONSE_SUFFIX}")
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

  fn make_unique_name(&mut self, base: String) -> String {
    let rust_name = to_rust_type_name(&base);
    if !self.used_names.contains(&rust_name) {
      return base;
    }

    let mut suffix = 2;
    while self.used_names.contains(&to_rust_type_name(&format!("{base}{suffix}"))) {
      suffix += 1;
    }
    format!("{base}{suffix}")
  }

  pub(crate) fn hash_schema(schema: &ObjectSchema) -> ConversionResult<String> {
    let mut value = serde_json::to_value(schema).context("Failed to serialize schema for hashing")?;

    Self::normalize_schema_semantics(&mut value);

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
          Self::sort_string_array_in_place(arr);
        }

        if let Some(Value::Array(arr)) = map.get_mut("type") {
          Self::sort_string_array_in_place(arr);
        }

        for value in map.values_mut() {
          Self::normalize_schema_semantics(value);
        }
      }
      Value::Array(arr) => {
        for item in arr {
          Self::normalize_schema_semantics(item);
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

  pub(crate) fn into_types(self) -> Vec<RustType> {
    self.generated_types
  }
}

#[cfg(test)]
mod tests {
  use oas3::spec::ObjectSchema;

  use super::*;

  #[test]
  fn test_hash_schema_deterministic() {
    let schema = ObjectSchema {
      required: vec!["name".to_string(), "id".to_string()],
      ..Default::default()
    };

    let hash1 = SharedSchemaCache::hash_schema(&schema).expect("hash should succeed");
    let hash2 = SharedSchemaCache::hash_schema(&schema).expect("hash should succeed");
    let hash3 = SharedSchemaCache::hash_schema(&schema).expect("hash should succeed");

    assert_eq!(hash1, hash2, "Hash should be deterministic across calls");
    assert_eq!(hash2, hash3, "Hash should be deterministic across calls");
    assert!(!hash1.is_empty(), "Hash should not be empty");
  }

  #[test]
  fn test_hash_schema_different_for_different_schemas() {
    let schema1 = ObjectSchema {
      required: vec!["id".to_string()],
      ..Default::default()
    };

    let schema2 = ObjectSchema {
      required: vec!["name".to_string()],
      ..Default::default()
    };

    let hash1 = SharedSchemaCache::hash_schema(&schema1).expect("hash should succeed");
    let hash2 = SharedSchemaCache::hash_schema(&schema2).expect("hash should succeed");

    assert_ne!(hash1, hash2, "Different schemas should produce different hashes");
  }

  #[test]
  fn test_hash_schema_order_independent() {
    let schema1 = ObjectSchema {
      required: vec!["id".to_string(), "name".to_string()],
      ..Default::default()
    };

    let schema2 = ObjectSchema {
      required: vec!["name".to_string(), "id".to_string()],
      ..Default::default()
    };

    let hash1 = SharedSchemaCache::hash_schema(&schema1).expect("hash should succeed");
    let hash2 = SharedSchemaCache::hash_schema(&schema2).expect("hash should succeed");

    assert_eq!(
      hash1, hash2,
      "Required array order should not affect hash due to RFC 8785 canonicalization"
    );
  }
}

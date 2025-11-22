use std::collections::BTreeMap;

use oas3::spec::{ObjectOrReference, ObjectSchema, SchemaType, SchemaTypeSet};
use serde_json::json;

use super::common::{create_test_graph, default_config};
use crate::generator::{
  ast::RustType,
  converter::{
    FieldOptionalityPolicy, SchemaConverter, cache::SharedSchemaCache, enums::EnumConverter, hashing,
    string_enum_optimizer::StringEnumOptimizer, type_resolver::TypeResolver,
  },
};

#[test]
fn test_hash_schema_deterministic() {
  let schema = ObjectSchema {
    required: vec!["name".to_string(), "id".to_string()],
    ..Default::default()
  };

  let hash1 = hashing::hash_schema(&schema).expect("hash should succeed");
  let hash2 = hashing::hash_schema(&schema).expect("hash should succeed");
  let hash3 = hashing::hash_schema(&schema).expect("hash should succeed");

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

  let hash1 = hashing::hash_schema(&schema1).expect("hash should succeed");
  let hash2 = hashing::hash_schema(&schema2).expect("hash should succeed");

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

  let hash1 = hashing::hash_schema(&schema1).expect("hash should succeed");
  let hash2 = hashing::hash_schema(&schema2).expect("hash should succeed");

  assert_eq!(
    hash1, hash2,
    "Required array order should not affect hash due to RFC 8785 canonicalization"
  );
}

#[test]
fn test_convert_simple_enum_registers_in_cache() {
  let schema = ObjectSchema {
    schema_type: Some(SchemaTypeSet::Single(SchemaType::String)),
    enum_values: vec![json!("value1"), json!("value2"), json!("value3")],
    ..Default::default()
  };

  let graph = create_test_graph(BTreeMap::new());
  let type_resolver = TypeResolver::new(&graph, default_config());
  let enum_converter = EnumConverter::new(&graph, type_resolver, default_config());
  let mut cache = SharedSchemaCache::new();

  let result = enum_converter.convert_simple_enum("TestEnum", &schema, Some(&mut cache));

  assert!(result.is_some(), "Should generate enum on first call");

  let enum_values = vec!["value1".to_string(), "value2".to_string(), "value3".to_string()];
  assert!(
    cache.is_enum_generated(&enum_values),
    "Enum should be registered in cache"
  );
  assert_eq!(
    cache.get_enum_name(&enum_values),
    Some("TestEnum".to_string()),
    "Cache should map enum values to the generated name"
  );
}

#[test]
fn test_convert_simple_enum_skips_duplicate() {
  let schema = ObjectSchema {
    schema_type: Some(SchemaTypeSet::Single(SchemaType::String)),
    enum_values: vec![json!("foo"), json!("bar")],
    ..Default::default()
  };

  let graph = create_test_graph(BTreeMap::new());
  let type_resolver = TypeResolver::new(&graph, default_config());
  let enum_converter = EnumConverter::new(&graph, type_resolver, default_config());
  let mut cache = SharedSchemaCache::new();

  let result1 = enum_converter.convert_simple_enum("FirstEnum", &schema, Some(&mut cache));
  assert!(result1.is_some(), "First enum should be generated");

  let result2 = enum_converter.convert_simple_enum("DuplicateEnum", &schema, Some(&mut cache));
  assert!(
    result2.is_none(),
    "Second enum with same values should not be generated"
  );
}

#[test]
fn test_string_enum_optimizer_reuses_cached_enum() {
  let enum_schema = ObjectSchema {
    schema_type: Some(SchemaTypeSet::Single(SchemaType::String)),
    enum_values: vec![json!("alpha"), json!("beta"), json!("gamma")],
    ..Default::default()
  };

  let anyof_schema = ObjectSchema {
    any_of: vec![
      ObjectOrReference::Object(ObjectSchema {
        schema_type: Some(SchemaTypeSet::Single(SchemaType::String)),
        ..Default::default()
      }),
      ObjectOrReference::Object(enum_schema.clone()),
    ],
    ..Default::default()
  };

  let graph = create_test_graph(BTreeMap::from([
    ("SimpleEnum".to_string(), enum_schema),
    ("OptimizedEnum".to_string(), anyof_schema.clone()),
  ]));

  let converter = SchemaConverter::new(&graph, FieldOptionalityPolicy::standard(), default_config());
  let mut cache = SharedSchemaCache::new();

  let simple_result = converter
    .convert_schema("SimpleEnum", graph.get_schema("SimpleEnum").unwrap(), Some(&mut cache))
    .expect("Should convert simple enum");
  assert_eq!(simple_result.len(), 1, "Simple enum should generate one type");

  let optimizer = StringEnumOptimizer::new(&graph, false);
  let optimized_result = optimizer.try_convert("OptimizedEnum", &anyof_schema, Some(&mut cache));

  assert!(
    optimized_result.is_some(),
    "StringEnumOptimizer should handle anyOf pattern"
  );
  let types = optimized_result.unwrap();
  assert_eq!(types.len(), 1, "Should only generate outer enum, reusing inner");

  let outer_enum = &types[0];
  if let RustType::Enum(e) = outer_enum {
    let known_variant = e.variants.iter().find(|v| v.name == "Known");
    assert!(known_variant.is_some(), "Should have Known variant");
  } else {
    panic!("Expected enum type");
  }
}

#[test]
fn test_full_schema_conversion_with_deduplication() {
  let chat_model_enum = ObjectSchema {
    schema_type: Some(SchemaTypeSet::Single(SchemaType::String)),
    enum_values: vec![json!("gpt-4"), json!("gpt-3.5-turbo")],
    ..Default::default()
  };

  let model_ids_shared = ObjectSchema {
    any_of: vec![
      ObjectOrReference::Object(ObjectSchema {
        schema_type: Some(SchemaTypeSet::Single(SchemaType::String)),
        ..Default::default()
      }),
      ObjectOrReference::Ref {
        ref_path: "#/components/schemas/ChatModel".to_string(),
        summary: None,
        description: None,
      },
    ],
    ..Default::default()
  };

  let graph = create_test_graph(BTreeMap::from([
    ("ChatModel".to_string(), chat_model_enum),
    ("ModelIdsShared".to_string(), model_ids_shared.clone()),
  ]));

  let converter = SchemaConverter::new(&graph, FieldOptionalityPolicy::standard(), default_config());
  let mut cache = SharedSchemaCache::new();

  let chat_model_result = converter
    .convert_schema("ChatModel", graph.get_schema("ChatModel").unwrap(), Some(&mut cache))
    .expect("Should convert ChatModel");
  assert_eq!(chat_model_result.len(), 1);

  let model_ids_result = converter
    .convert_schema(
      "ModelIdsShared",
      graph.get_schema("ModelIdsShared").unwrap(),
      Some(&mut cache),
    )
    .expect("Should convert ModelIdsShared");

  assert_eq!(
    model_ids_result.len(),
    1,
    "Should only generate outer enum, not duplicate inner"
  );

  if let RustType::Enum(outer) = &model_ids_result[0] {
    assert_eq!(outer.name, "ModelIdsShared");
    let known_variant = outer.variants.iter().find(|v| v.name == "Known");
    assert!(known_variant.is_some(), "Should have Known variant");
  } else {
    panic!("Expected enum type for ModelIdsShared");
  }
}

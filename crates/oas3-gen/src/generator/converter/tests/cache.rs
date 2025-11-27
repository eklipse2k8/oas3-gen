use std::{
  collections::{BTreeMap, BTreeSet},
  sync::Arc,
};

use oas3::spec::{ObjectOrReference, ObjectSchema, SchemaType, SchemaTypeSet};
use serde_json::json;

use crate::{
  generator::{
    ast::{EnumDef, EnumToken, RustType},
    converter::{
      FieldOptionalityPolicy, SchemaConverter, cache::SharedSchemaCache, enums::EnumConverter, hashing,
      string_enum_optimizer::StringEnumOptimizer, type_resolver::TypeResolver,
    },
    schema_registry::SchemaRegistry,
  },
  tests::common::{create_test_graph, default_config},
};

fn make_string_enum_schema(values: &[&str]) -> ObjectSchema {
  ObjectSchema {
    schema_type: Some(SchemaTypeSet::Single(SchemaType::String)),
    enum_values: values.iter().map(|v| json!(v)).collect(),
    ..Default::default()
  }
}

fn create_test_converter(graph: &Arc<SchemaRegistry>) -> SchemaConverter {
  SchemaConverter::new(graph, FieldOptionalityPolicy::standard(), default_config())
}

#[test]
fn test_hash_schema_behavior() {
  let schema1 = ObjectSchema {
    required: vec!["name".to_string(), "id".to_string()],
    ..Default::default()
  };

  let schema2 = ObjectSchema {
    required: vec!["id".to_string(), "name".to_string()],
    ..Default::default()
  };

  let schema3 = ObjectSchema {
    required: vec!["different".to_string()],
    ..Default::default()
  };

  let first_hash = hashing::hash_schema(&schema1).expect("hash should succeed");
  let repeated_hash = hashing::hash_schema(&schema1).expect("hash should succeed");
  let reordered_hash = hashing::hash_schema(&schema2).expect("hash should succeed");
  let different_hash = hashing::hash_schema(&schema3).expect("hash should succeed");

  assert_eq!(first_hash, repeated_hash, "Hash should be deterministic across calls");
  assert!(!first_hash.is_empty(), "Hash should not be empty");
  assert_eq!(
    first_hash, reordered_hash,
    "Required array order should not affect hash due to RFC 8785 canonicalization"
  );
  assert_ne!(
    first_hash, different_hash,
    "Different schemas should produce different hashes"
  );
}

#[test]
fn test_convert_simple_enum_with_cache() {
  let schema = make_string_enum_schema(&["value1", "value2", "value3"]);

  let graph = create_test_graph(BTreeMap::new());
  let type_resolver = TypeResolver::new(&graph, default_config());
  let enum_converter = EnumConverter::new(&graph, type_resolver, default_config());
  let mut cache = SharedSchemaCache::new();

  let result1 = enum_converter.convert_simple_enum("TestEnum", &schema, Some(&mut cache));
  assert!(result1.is_some(), "Should generate enum on first call");

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

  let result2 = enum_converter.convert_simple_enum("DuplicateEnum", &schema, Some(&mut cache));
  assert!(
    result2.is_none(),
    "Second enum with same values should not be generated"
  );
}

#[test]
fn test_string_enum_optimizer_reuses_cached_enum() {
  let enum_schema = make_string_enum_schema(&["alpha", "beta", "gamma"]);

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

  let converter = create_test_converter(&graph);
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
  let chat_model_enum = make_string_enum_schema(&["gpt-4", "gpt-3.5-turbo"]);

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

  let converter = create_test_converter(&graph);
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
    assert_eq!(outer.name.to_string(), "ModelIdsShared");
    let known_variant = outer.variants.iter().find(|v| v.name == "Known");
    assert!(known_variant.is_some(), "Should have Known variant");
  } else {
    panic!("Expected enum type for ModelIdsShared");
  }
}

#[test]
fn test_name_uniqueness() {
  let mut cache = SharedSchemaCache::new();

  cache.mark_name_used("User".to_string());
  let unique_name = cache.make_unique_name("User");
  assert_ne!(
    unique_name, "User",
    "Should generate unique name when name is already used"
  );
  assert!(unique_name.starts_with("User"), "Should maintain base name prefix");

  cache.mark_name_used("Item".to_string());
  let name1 = cache.make_unique_name("Item");
  cache.mark_name_used(name1.clone());
  let name2 = cache.make_unique_name("Item");
  cache.mark_name_used(name2.clone());
  let name3 = cache.make_unique_name("Item");

  let unique_names = [&name1, &name2, &name3];
  for (i, current) in unique_names.iter().enumerate() {
    assert!(current.starts_with("Item"), "Name {i} should maintain base name");
    for (j, other) in unique_names.iter().enumerate() {
      if i != j {
        assert_ne!(current, other, "Names {i} and {j} should be different");
      }
    }
  }
}

#[test]
fn test_precomputed_names() {
  let schema = ObjectSchema {
    required: vec!["id".to_string()],
    ..Default::default()
  };

  let schema_hash = hashing::hash_schema(&schema).expect("hash should succeed");
  let mut precomputed_names = BTreeMap::new();
  precomputed_names.insert(schema_hash, "CustomName".to_string());

  let enum_values = vec!["alpha".to_string(), "beta".to_string()];
  let mut precomputed_enum_names = BTreeMap::new();
  precomputed_enum_names.insert(enum_values.clone(), "PrecomputedEnum".to_string());

  let mut cache = SharedSchemaCache::new();
  cache.set_precomputed_names(precomputed_names, precomputed_enum_names);

  let preferred_name = cache
    .get_preferred_name(&schema, "DefaultName")
    .expect("should get preferred name");
  assert_eq!(preferred_name, "CustomName", "Should use precomputed schema name");

  let found_enum_name = cache.get_enum_name(&enum_values);
  assert_eq!(
    found_enum_name,
    Some("PrecomputedEnum".to_string()),
    "Should find precomputed enum name"
  );
}

#[test]
fn test_cache_operations() {
  let mut cache = SharedSchemaCache::new();

  let enum_values = vec!["red".to_string(), "green".to_string(), "blue".to_string()];
  assert!(
    !cache.is_enum_generated(&enum_values),
    "Enum should not be generated initially"
  );
  cache.register_enum(enum_values.clone(), "Color".to_string());
  assert!(
    cache.is_enum_generated(&enum_values),
    "Enum should be marked as generated"
  );
  assert_eq!(
    cache.get_enum_name(&enum_values),
    Some("Color".to_string()),
    "Should retrieve registered enum name"
  );

  let new_schema = ObjectSchema {
    required: vec!["name".to_string()],
    ..Default::default()
  };
  let result = cache.get_type_name(&new_schema).expect("should succeed");
  assert_eq!(result, None, "Should return None for uncached schema");

  let schema1 = make_string_enum_schema(&["a", "b"]);
  let schema2 = make_string_enum_schema(&["x", "y"]);

  let enum1 = RustType::Enum(EnumDef {
    name: EnumToken::new("FirstEnum"),
    docs: vec![],
    variants: vec![],
    discriminator: None,
    derives: BTreeSet::new(),
    serde_attrs: vec![],
    outer_attrs: vec![],
    case_insensitive: false,
    methods: vec![],
  });

  let enum2 = RustType::Enum(EnumDef {
    name: EnumToken::new("SecondEnum"),
    docs: vec![],
    variants: vec![],
    discriminator: None,
    derives: BTreeSet::new(),
    serde_attrs: vec![],
    outer_attrs: vec![],
    case_insensitive: false,
    methods: vec![],
  });

  let mut type_cache = SharedSchemaCache::new();
  type_cache
    .register_type(&schema1, "FirstEnum", vec![], enum1)
    .expect("Should register first enum");
  type_cache
    .register_type(&schema2, "SecondEnum", vec![], enum2)
    .expect("Should register second enum");

  let types = type_cache.into_types();
  assert_eq!(types.len(), 2, "Should return all generated types");
}

use std::{collections::BTreeMap, sync::Arc};

use oas3::spec::{ObjectOrReference, ObjectSchema, SchemaType, SchemaTypeSet};
use serde_json::json;

use crate::{
  generator::{
    ast::{EnumDef, EnumToken, EnumVariantToken, RustType},
    converter::{
      SchemaConverter, cache::SharedSchemaCache, hashing::CanonicalSchema, type_resolver::TypeResolver,
      union_types::UnionKind, unions::UnionConverter,
    },
    naming::constants::KNOWN_ENUM_VARIANT,
    schema_registry::SchemaRegistry,
  },
  tests::common::{create_test_context, create_test_graph, default_config},
};

fn make_string_enum_schema(values: &[&str]) -> ObjectSchema {
  ObjectSchema {
    schema_type: Some(SchemaTypeSet::Single(SchemaType::String)),
    enum_values: values.iter().map(|v| json!(v)).collect(),
    ..Default::default()
  }
}

fn create_test_converter(graph: &Arc<SchemaRegistry>) -> SchemaConverter {
  let context = create_test_context(graph.clone(), default_config());
  SchemaConverter::new(&context)
}

#[test]
fn test_canonical_schema_equality_and_ordering() {
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

  let first = CanonicalSchema::from_schema(&schema1).expect("should succeed");
  let repeated = CanonicalSchema::from_schema(&schema1).expect("should succeed");
  let reordered = CanonicalSchema::from_schema(&schema2).expect("should succeed");
  let different = CanonicalSchema::from_schema(&schema3).expect("should succeed");

  assert_eq!(first, repeated, "CanonicalSchema should be deterministic across calls");
  assert_eq!(
    first, reordered,
    "Required array order should not affect equality due to RFC 8785 canonicalization"
  );
  assert_ne!(
    first, different,
    "Different schemas should produce different CanonicalSchemas"
  );

  assert!(first <= reordered, "Equal schemas should satisfy <= ordering");
  assert!(first >= reordered, "Equal schemas should satisfy >= ordering");

  let mut schemas_diff = [different.clone(), first.clone()];
  schemas_diff.sort();
  assert_eq!(schemas_diff.len(), 2, "Sorting should preserve all elements");
}

#[test]
fn test_relaxed_enum_generates_known_variant() {
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

  let context = create_test_context(graph.clone(), default_config());
  let _type_resolver = TypeResolver::new(context.clone());
  let union_converter = UnionConverter::new(context);

  let optimized_output = union_converter
    .convert_union("OptimizedEnum", &anyof_schema, UnionKind::AnyOf)
    .expect("Should convert anyOf union");

  let optimized_result = optimized_output.into_vec();
  assert!(!optimized_result.is_empty(), "Should generate at least one type");

  let outer_enum = optimized_result
    .iter()
    .find(|t| matches!(t, RustType::Enum(e) if e.name == "OptimizedEnum"));
  assert!(outer_enum.is_some(), "Should generate OptimizedEnum");

  if let Some(RustType::Enum(e)) = outer_enum {
    let known_variant = e
      .variants
      .iter()
      .find(|v| v.name == EnumVariantToken::new(KNOWN_ENUM_VARIANT));
    assert!(
      known_variant.is_some(),
      "Should have Known variant for relaxed enum pattern"
    );
  }
}

#[test]
fn test_relaxed_enum_with_ref() {
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

  let chat_model_result = converter
    .convert_schema("ChatModel", graph.get("ChatModel").unwrap())
    .expect("Should convert ChatModel");
  assert_eq!(chat_model_result.len(), 1);

  let model_ids_result = converter
    .convert_schema("ModelIdsShared", graph.get("ModelIdsShared").unwrap())
    .expect("Should convert ModelIdsShared");

  assert!(!model_ids_result.is_empty(), "Should generate at least one type");

  let outer_enum = model_ids_result
    .iter()
    .find(|t| matches!(t, RustType::Enum(e) if e.name == "ModelIdsShared"));
  assert!(outer_enum.is_some(), "Should generate ModelIdsShared enum");

  if let Some(RustType::Enum(outer)) = outer_enum {
    let known_variant = outer
      .variants
      .iter()
      .find(|v| v.name == EnumVariantToken::new(KNOWN_ENUM_VARIANT));
    assert!(
      known_variant.is_some(),
      "Should have Known variant for relaxed enum pattern"
    );
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

  let canonical = CanonicalSchema::from_schema(&schema).expect("should succeed");
  let mut precomputed_names = BTreeMap::new();
  precomputed_names.insert(canonical, "CustomName".to_string());

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
    variants: vec![],
    discriminator: None,
    serde_attrs: vec![],
    outer_attrs: vec![],
    case_insensitive: false,
    methods: vec![],
    ..Default::default()
  });

  let enum2 = RustType::Enum(EnumDef {
    name: EnumToken::new("SecondEnum"),
    variants: vec![],
    discriminator: None,
    serde_attrs: vec![],
    outer_attrs: vec![],
    case_insensitive: false,
    methods: vec![],
    ..Default::default()
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

#[test]
fn test_canonical_schema_as_btreemap_key() {
  use std::collections::BTreeMap;

  let schema_a = ObjectSchema {
    required: vec!["alpha".to_string()],
    ..Default::default()
  };
  let schema_b = ObjectSchema {
    required: vec!["beta".to_string()],
    ..Default::default()
  };
  let schema_a_reordered = ObjectSchema {
    required: vec!["alpha".to_string()],
    ..Default::default()
  };

  let canonical_a = CanonicalSchema::from_schema(&schema_a).expect("should succeed");
  let canonical_b = CanonicalSchema::from_schema(&schema_b).expect("should succeed");
  let canonical_a_dup = CanonicalSchema::from_schema(&schema_a_reordered).expect("should succeed");

  let mut map: BTreeMap<CanonicalSchema, &str> = BTreeMap::new();
  map.insert(canonical_a.clone(), "first");
  map.insert(canonical_b.clone(), "second");

  assert_eq!(map.len(), 2, "Map should have two entries for different schemas");
  assert_eq!(map.get(&canonical_a), Some(&"first"));
  assert_eq!(map.get(&canonical_b), Some(&"second"));
  assert_eq!(
    map.get(&canonical_a_dup),
    Some(&"first"),
    "Lookup with equivalent schema should find same entry"
  );

  map.insert(canonical_a_dup, "overwritten");
  assert_eq!(map.len(), 2, "Map size should remain 2 after inserting duplicate key");
  assert_eq!(
    map.get(&canonical_a),
    Some(&"overwritten"),
    "Value should be overwritten for equivalent key"
  );
}

#[test]
fn test_canonical_schema_normalizes_enum_order() {
  let schema1 = ObjectSchema {
    enum_values: vec![json!("z"), json!("a"), json!("m")],
    ..Default::default()
  };
  let schema2 = ObjectSchema {
    enum_values: vec![json!("a"), json!("m"), json!("z")],
    ..Default::default()
  };

  let canonical1 = CanonicalSchema::from_schema(&schema1).expect("should succeed");
  let canonical2 = CanonicalSchema::from_schema(&schema2).expect("should succeed");

  assert_eq!(
    canonical1, canonical2,
    "Enum value order should not affect canonical equality"
  );
}

#[test]
fn test_canonical_schema_normalizes_type_array_order() {
  let schema1 = ObjectSchema {
    schema_type: Some(SchemaTypeSet::Multiple(vec![SchemaType::String, SchemaType::Null])),
    ..Default::default()
  };
  let schema2 = ObjectSchema {
    schema_type: Some(SchemaTypeSet::Multiple(vec![SchemaType::Null, SchemaType::String])),
    ..Default::default()
  };

  let canonical1 = CanonicalSchema::from_schema(&schema1).expect("should succeed");
  let canonical2 = CanonicalSchema::from_schema(&schema2).expect("should succeed");

  assert_eq!(
    canonical1, canonical2,
    "Type array order should not affect canonical equality"
  );
}

#[test]
fn test_canonical_schema_clone() {
  let schema = ObjectSchema {
    required: vec!["field".to_string()],
    ..Default::default()
  };

  let canonical = CanonicalSchema::from_schema(&schema).expect("should succeed");
  let cloned = canonical.clone();

  assert_eq!(canonical, cloned, "Cloned CanonicalSchema should equal original");
}

#[test]
fn test_canonical_schema_hash_consistency() {
  use std::{
    collections::hash_map::DefaultHasher,
    hash::{Hash, Hasher},
  };

  let schema = ObjectSchema {
    required: vec!["a".to_string(), "b".to_string()],
    ..Default::default()
  };

  let canonical = CanonicalSchema::from_schema(&schema).expect("should succeed");

  let mut hasher1 = DefaultHasher::new();
  canonical.hash(&mut hasher1);
  let hash1 = hasher1.finish();

  let mut hasher2 = DefaultHasher::new();
  canonical.hash(&mut hasher2);
  let hash2 = hasher2.finish();

  assert_eq!(hash1, hash2, "Hash should be consistent across calls");

  let canonical_dup = CanonicalSchema::from_schema(&schema).expect("should succeed");
  let mut hasher3 = DefaultHasher::new();
  canonical_dup.hash(&mut hasher3);
  let hash3 = hasher3.finish();

  assert_eq!(hash1, hash3, "Equal CanonicalSchemas should produce equal hashes");
}

#[test]
fn test_relaxed_enum_does_not_overwrite_inner_enum_registration() {
  let enum_schema = make_string_enum_schema(&["easy", "medium", "hard", "expert"]);

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
    ("FirstRelaxedEnum".to_string(), anyof_schema.clone()),
    ("SecondRelaxedEnum".to_string(), anyof_schema.clone()),
  ]));

  let context = create_test_context(graph.clone(), default_config());
  let union_converter = UnionConverter::new(context.clone());

  let first_output = union_converter
    .convert_union("FirstRelaxedEnum", &anyof_schema, UnionKind::AnyOf)
    .expect("Should convert first anyOf union");
  let first_result = first_output.into_vec();

  let first_outer = first_result
    .iter()
    .find(|t| matches!(t, RustType::Enum(e) if e.name == "FirstRelaxedEnum"))
    .expect("Should generate FirstRelaxedEnum");

  let inner_enum_name = if let RustType::Enum(e) = first_outer {
    let known_variant = e
      .variants
      .iter()
      .find(|v| v.name == EnumVariantToken::new(KNOWN_ENUM_VARIANT))
      .expect("Should have Known variant");
    if let crate::generator::ast::VariantContent::Tuple(refs) = &known_variant.content {
      refs[0].base_type.to_string()
    } else {
      panic!("Known variant should have tuple content");
    }
  } else {
    panic!("FirstRelaxedEnum should be an enum");
  };

  let second_output = union_converter
    .convert_union("SecondRelaxedEnum", &anyof_schema, UnionKind::AnyOf)
    .expect("Should convert second anyOf union");
  let second_result = second_output.into_vec();

  let second_outer = second_result
    .iter()
    .find(|t| matches!(t, RustType::Enum(e) if e.name == "SecondRelaxedEnum"))
    .expect("Should generate SecondRelaxedEnum");

  if let RustType::Enum(e) = second_outer {
    let known_variant = e
      .variants
      .iter()
      .find(|v| v.name == EnumVariantToken::new(KNOWN_ENUM_VARIANT))
      .expect("Should have Known variant");
    if let crate::generator::ast::VariantContent::Tuple(refs) = &known_variant.content {
      let second_inner_name = refs[0].base_type.to_string();
      assert_eq!(
        inner_enum_name, second_inner_name,
        "Both relaxed enums should reference the same inner known values enum. \
        First references '{inner_enum_name}', second references '{second_inner_name}'. \
        This is a regression of issue #57."
      );
    } else {
      panic!("Known variant should have tuple content");
    }
  } else {
    panic!("SecondRelaxedEnum should be an enum");
  }
}

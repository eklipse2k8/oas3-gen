use std::collections::BTreeMap;

use oas3::spec::{
  BooleanSchema, Components, Discriminator, Info, ObjectOrReference, ObjectSchema, Schema, SchemaType, SchemaTypeSet,
  Spec,
};
use serde_json::json;

use crate::{
  generator::{
    metrics::{GenerationStats, GenerationWarning},
    schema_registry::SchemaRegistry,
  },
  utils::parse_schema_ref_path,
};

const SCHEMA_REF_PREFIX: &str = "#/components/schemas/";

fn create_test_spec_with_schemas(schemas: BTreeMap<String, ObjectOrReference<ObjectSchema>>) -> Spec {
  Spec {
    openapi: "3.0.0".to_string(),
    info: Info {
      title: "Test".to_string(),
      summary: None,
      version: "1.0.0".to_string(),
      description: None,
      terms_of_service: None,
      contact: None,
      license: None,
      extensions: BTreeMap::default(),
    },
    servers: vec![],
    paths: Option::default(),
    webhooks: BTreeMap::default(),
    components: Some(Components {
      schemas,
      ..Default::default()
    }),
    security: vec![],
    tags: vec![],
    external_docs: None,
    extensions: BTreeMap::default(),
  }
}

fn make_simple_schema() -> ObjectSchema {
  ObjectSchema {
    schema_type: None,
    properties: BTreeMap::new(),
    ..Default::default()
  }
}

fn make_schema_with_ref(ref_name: &str) -> ObjectSchema {
  let mut properties = BTreeMap::new();
  properties.insert(
    "related".to_string(),
    ObjectOrReference::Ref {
      ref_path: format!("{SCHEMA_REF_PREFIX}{ref_name}"),
      summary: None,
      description: None,
    },
  );
  ObjectSchema {
    schema_type: None,
    properties,
    ..Default::default()
  }
}

fn make_ref(name: &str) -> ObjectOrReference<ObjectSchema> {
  ObjectOrReference::Ref {
    ref_path: format!("{SCHEMA_REF_PREFIX}{name}"),
    summary: None,
    description: None,
  }
}

#[test]
fn test_parse_ref() {
  let cases = [
    ("#/components/schemas/Corgi", Some("Corgi")),
    ("#/components/schemas/NestedCorgi", Some("NestedCorgi")),
    ("#/other/path", None),
    ("InvalidRef", None),
  ];
  for (input, expected) in cases {
    let result = parse_schema_ref_path(input);
    assert_eq!(result.as_deref(), expected, "failed for input {input:?}");
  }
}

#[test]
fn test_ref_collector() {
  let spec = create_test_spec_with_schemas(BTreeMap::new());
  let mut stats = GenerationStats::default();
  let registry = SchemaRegistry::new(&spec, &mut stats);
  let union_fingerprints = BTreeMap::new();

  let schema = make_schema_with_ref("Corgi");
  let refs = registry.collect(&schema, &union_fingerprints);
  assert_eq!(refs.len(), 1, "simple ref: expected 1 ref");
  assert!(refs.contains("Corgi"), "simple ref: should contain Corgi");

  let mut properties = BTreeMap::new();
  properties.insert("waddler".to_string(), make_ref("Corgi"));
  properties.insert("sploot".to_string(), make_ref("Sploot"));
  let schema = ObjectSchema {
    schema_type: None,
    properties,
    ..Default::default()
  };
  let refs = registry.collect(&schema, &union_fingerprints);
  assert_eq!(refs.len(), 2, "multiple refs: expected 2 refs");
  assert!(refs.contains("Corgi"), "multiple refs: should contain Corgi");
  assert!(refs.contains("Sploot"), "multiple refs: should contain Sploot");

  let schema = ObjectSchema {
    schema_type: None,
    one_of: vec![make_ref("Corgi")],
    any_of: vec![make_ref("Bark")],
    all_of: vec![make_ref("Frappe")],
    ..Default::default()
  };
  let refs = registry.collect(&schema, &union_fingerprints);
  assert_eq!(refs.len(), 3, "combinators: expected 3 refs");
  assert!(refs.contains("Corgi"), "combinators: should contain Corgi");
  assert!(refs.contains("Bark"), "combinators: should contain Bark");
  assert!(refs.contains("Frappe"), "combinators: should contain Frappe");
}

#[test]
fn test_schema_registry() {
  let mut schemas = BTreeMap::new();
  schemas.insert("Corgi".to_string(), ObjectOrReference::Object(make_simple_schema()));
  schemas.insert("Bark".to_string(), ObjectOrReference::Object(make_simple_schema()));

  let spec = create_test_spec_with_schemas(schemas);
  let mut stats = GenerationStats::default();
  let registry = SchemaRegistry::new(&spec, &mut stats);

  assert!(registry.get("Corgi").is_some(), "should have Corgi schema");
  assert!(registry.get("Bark").is_some(), "should have Bark schema");
  assert!(registry.get("NonExistent").is_none(), "should not have NonExistent");
  assert_eq!(registry.keys().len(), 2, "should have 2 schemas");

  let mut schemas = BTreeMap::new();
  schemas.insert("Corgi".to_string(), ObjectOrReference::Object(make_simple_schema()));
  schemas.insert(
    "Bark".to_string(),
    ObjectOrReference::Object(make_schema_with_ref("Corgi")),
  );

  let spec = create_test_spec_with_schemas(schemas);
  let mut stats = GenerationStats::default();
  let mut graph = SchemaRegistry::new(&spec, &mut stats);
  let union_fingerprints = BTreeMap::new();
  graph.build_dependencies(&union_fingerprints);

  assert_eq!(graph.keys().len(), 2, "build deps: should have 2 schemas");
  assert!(graph.get("Corgi").is_some(), "build deps: should have Corgi");
  assert!(graph.get("Bark").is_some(), "build deps: should have Bark");
}

#[test]
fn test_schema_graph_cycle_detection() {
  {
    let mut schemas = BTreeMap::new();
    schemas.insert("A".to_string(), ObjectOrReference::Object(make_simple_schema()));
    schemas.insert("B".to_string(), ObjectOrReference::Object(make_schema_with_ref("A")));
    let mut c_schema = make_simple_schema();
    c_schema.properties.insert("b".to_string(), make_ref("B"));
    schemas.insert("C".to_string(), ObjectOrReference::Object(c_schema));

    let spec = create_test_spec_with_schemas(schemas);
    let mut stats = GenerationStats::default();
    let mut graph = SchemaRegistry::new(&spec, &mut stats);
    let union_fingerprints = BTreeMap::new();
    graph.build_dependencies(&union_fingerprints);
    let cycles = graph.detect_cycles();

    assert!(cycles.is_empty(), "linear deps: should have no cycles");
    assert!(!graph.is_cyclic("A"), "linear deps: A should not be cyclic");
    assert!(!graph.is_cyclic("B"), "linear deps: B should not be cyclic");
    assert!(!graph.is_cyclic("C"), "linear deps: C should not be cyclic");
  }

  {
    let mut a_schema = make_simple_schema();
    a_schema.properties.insert("b".to_string(), make_ref("B"));
    let mut b_schema = make_simple_schema();
    b_schema.properties.insert("a".to_string(), make_ref("A"));

    let mut schemas = BTreeMap::new();
    schemas.insert("A".to_string(), ObjectOrReference::Object(a_schema));
    schemas.insert("B".to_string(), ObjectOrReference::Object(b_schema));

    let spec = create_test_spec_with_schemas(schemas);
    let mut stats = GenerationStats::default();
    let mut graph = SchemaRegistry::new(&spec, &mut stats);
    let union_fingerprints = BTreeMap::new();
    graph.build_dependencies(&union_fingerprints);
    let cycles = graph.detect_cycles();

    assert_eq!(cycles.len(), 1, "simple cycle: should detect 1 cycle");
    assert!(!cycles[0].is_empty(), "simple cycle: cycle should not be empty");
    assert!(graph.is_cyclic("A"), "simple cycle: A should be cyclic");
    assert!(graph.is_cyclic("B"), "simple cycle: B should be cyclic");
  }

  {
    let mut a_schema = make_simple_schema();
    a_schema.properties.insert("self_ref".to_string(), make_ref("A"));

    let mut schemas = BTreeMap::new();
    schemas.insert("A".to_string(), ObjectOrReference::Object(a_schema));

    let spec = create_test_spec_with_schemas(schemas);
    let mut stats = GenerationStats::default();
    let mut graph = SchemaRegistry::new(&spec, &mut stats);
    let union_fingerprints = BTreeMap::new();
    graph.build_dependencies(&union_fingerprints);
    let cycles = graph.detect_cycles();

    assert_eq!(cycles.len(), 1, "self-ref: should detect 1 cycle");
    assert!(graph.is_cyclic("A"), "self-ref: A should be cyclic");
  }

  {
    let mut user_schema = make_simple_schema();
    user_schema.properties.insert("posts".to_string(), make_ref("Post"));
    let mut post_schema = make_simple_schema();
    post_schema.properties.insert("author".to_string(), make_ref("User"));

    let mut schemas = BTreeMap::new();
    schemas.insert("User".to_string(), ObjectOrReference::Object(user_schema));
    schemas.insert("Post".to_string(), ObjectOrReference::Object(post_schema));

    let spec = create_test_spec_with_schemas(schemas);
    let mut stats = GenerationStats::default();
    let mut graph = SchemaRegistry::new(&spec, &mut stats);
    let union_fingerprints = BTreeMap::new();
    graph.build_dependencies(&union_fingerprints);
    let cycles = graph.detect_cycles();

    assert!(!cycles.is_empty(), "user-post cycle: should detect cycles");
    assert!(graph.is_cyclic("User"), "user-post cycle: User should be cyclic");
    assert!(graph.is_cyclic("Post"), "user-post cycle: Post should be cyclic");
  }
}

#[test]
fn test_schema_graph_integration() {
  let mut schemas = BTreeMap::new();
  schemas.insert("Corgi".to_string(), ObjectOrReference::Object(make_simple_schema()));
  schemas.insert(
    "Bark".to_string(),
    ObjectOrReference::Object(make_schema_with_ref("Corgi")),
  );

  let spec = create_test_spec_with_schemas(schemas);
  let mut stats = GenerationStats::default();
  let mut graph = SchemaRegistry::new(&spec, &mut stats);

  assert!(graph.get("Corgi").is_some(), "integration: should have Corgi");
  assert!(graph.get("Bark").is_some(), "integration: should have Bark");
  assert_eq!(graph.keys().len(), 2, "integration: should have 2 schemas");

  let union_fingerprints = BTreeMap::new();
  graph.build_dependencies(&union_fingerprints);
  let cycles = graph.detect_cycles();

  assert!(cycles.is_empty(), "integration: should have no cycles");
  assert!(!graph.is_cyclic("Corgi"), "integration: Corgi should not be cyclic");
}

#[test]
fn test_schema_registry_merges_all_of_properties_and_required() {
  let mut loaf = make_simple_schema();
  loaf.schema_type = Some(SchemaTypeSet::Single(SchemaType::Object));
  loaf.required.push("tag_id".to_string());
  loaf.properties.insert(
    "tag_id".to_string(),
    ObjectOrReference::Object(ObjectSchema {
      schema_type: Some(SchemaTypeSet::Single(SchemaType::Integer)),
      ..Default::default()
    }),
  );
  loaf.additional_properties = Some(Schema::Boolean(BooleanSchema(true)));

  let mut nugget = make_simple_schema();
  nugget.schema_type = Some(SchemaTypeSet::Single(SchemaType::Object));
  nugget.required.push("name".to_string());
  nugget.properties.insert(
    "name".to_string(),
    ObjectOrReference::Object(ObjectSchema {
      schema_type: Some(SchemaTypeSet::Single(SchemaType::String)),
      ..Default::default()
    }),
  );
  nugget.all_of.push(make_ref("Loaf"));

  let spec = create_test_spec_with_schemas(BTreeMap::from([
    ("Loaf".to_string(), ObjectOrReference::Object(loaf.clone())),
    ("Nugget".to_string(), ObjectOrReference::Object(nugget.clone())),
  ]));

  let mut stats = GenerationStats::default();
  let mut graph = SchemaRegistry::new(&spec, &mut stats);
  let union_fingerprints = BTreeMap::new();
  graph.build_dependencies(&union_fingerprints);
  graph.detect_cycles();

  let merged = graph.merged("Nugget").expect("merged schema should exist for Nugget");

  assert!(merged.schema.properties.contains_key("tag_id"));
  assert!(merged.schema.properties.contains_key("name"));
  assert!(merged.schema.required.contains(&"tag_id".to_string()));
  assert!(merged.schema.required.contains(&"name".to_string()));
  assert!(merged.schema.additional_properties.is_some());
}

#[test]
fn test_schema_registry_merges_and_tracks_discriminator_parents() {
  let mut loaf_schema = make_simple_schema();
  loaf_schema.properties.insert(
    "kind".to_string(),
    ObjectOrReference::Object(ObjectSchema {
      schema_type: Some(SchemaTypeSet::Single(SchemaType::String)),
      ..Default::default()
    }),
  );
  loaf_schema.discriminator = Some(Discriminator {
    property_name: "kind".to_string(),
    mapping: Some(BTreeMap::from([(
      "nugget".to_string(),
      format!("{SCHEMA_REF_PREFIX}Nugget"),
    )])),
  });

  let mut nugget_schema = make_simple_schema();
  nugget_schema.properties.insert(
    "nugget_prop".to_string(),
    ObjectOrReference::Object(ObjectSchema {
      schema_type: Some(SchemaTypeSet::Single(SchemaType::Integer)),
      ..Default::default()
    }),
  );
  nugget_schema.all_of.push(make_ref("Loaf"));

  let spec = create_test_spec_with_schemas(BTreeMap::from([
    ("Loaf".to_string(), ObjectOrReference::Object(loaf_schema.clone())),
    ("Nugget".to_string(), ObjectOrReference::Object(nugget_schema.clone())),
  ]));

  let mut stats = GenerationStats::default();
  let mut graph = SchemaRegistry::new(&spec, &mut stats);
  let union_fingerprints = BTreeMap::new();
  graph.build_dependencies(&union_fingerprints);
  graph.detect_cycles();

  let merged_nugget = graph.merged("Nugget").expect("merged schema should exist for Nugget");

  assert_eq!(merged_nugget.discriminator_parent.as_deref(), Some("Loaf"));
  assert!(merged_nugget.schema.properties.contains_key("kind"));
  assert!(merged_nugget.schema.properties.contains_key("nugget_prop"));

  let parent_name = graph.parent("Nugget").expect("discriminator parent should be tracked");

  assert_eq!(parent_name, "Loaf");

  let effective = graph.resolved("Nugget").unwrap();
  assert_eq!(effective.properties.len(), merged_nugget.schema.properties.len());
}

#[test]
fn schema_merger_merge_child_with_parent() {
  let mut loaf = make_simple_schema();
  loaf.schema_type = Some(SchemaTypeSet::Single(SchemaType::Object));
  loaf.properties.insert(
    "loaf_prop".to_string(),
    ObjectOrReference::Object(ObjectSchema {
      schema_type: Some(SchemaTypeSet::Single(SchemaType::String)),
      ..Default::default()
    }),
  );
  loaf.required.push("loaf_prop".to_string());

  let mut nugget = make_simple_schema();
  nugget.schema_type = Some(SchemaTypeSet::Single(SchemaType::Object));
  nugget.properties.insert(
    "nugget_prop".to_string(),
    ObjectOrReference::Object(ObjectSchema {
      schema_type: Some(SchemaTypeSet::Single(SchemaType::Integer)),
      ..Default::default()
    }),
  );
  nugget.all_of.push(make_ref("Loaf"));

  let spec = create_test_spec_with_schemas(BTreeMap::from([
    ("Loaf".to_string(), ObjectOrReference::Object(loaf)),
    ("Nugget".to_string(), ObjectOrReference::Object(nugget)),
  ]));

  let mut stats = GenerationStats::default();
  let mut graph = SchemaRegistry::new(&spec, &mut stats);
  let union_fingerprints = BTreeMap::new();
  graph.build_dependencies(&union_fingerprints);
  graph.detect_cycles();

  let merged = graph.merged("Nugget").expect("merged schema should exist for Nugget");

  assert!(
    merged.schema.properties.contains_key("loaf_prop"),
    "should have loaf_prop"
  );
  assert!(
    merged.schema.properties.contains_key("nugget_prop"),
    "should have nugget_prop"
  );
  assert!(
    merged.schema.required.contains(&"loaf_prop".to_string()),
    "loaf_prop should be required"
  );

  let effective = graph.resolved("Nugget").unwrap();
  assert_eq!(
    effective.properties.len(),
    merged.schema.properties.len(),
    "resolved should match merged"
  );
}

#[test]
fn schema_merger_conflict_resolution() {
  let mut loaf = make_simple_schema();
  loaf.schema_type = Some(SchemaTypeSet::Single(SchemaType::Object));
  loaf.properties.insert(
    "prop".to_string(),
    ObjectOrReference::Object(ObjectSchema {
      schema_type: Some(SchemaTypeSet::Single(SchemaType::String)),
      ..Default::default()
    }),
  );

  let mut nugget = make_simple_schema();
  nugget.schema_type = Some(SchemaTypeSet::Single(SchemaType::Object));
  nugget.properties.insert(
    "prop".to_string(),
    ObjectOrReference::Object(ObjectSchema {
      schema_type: Some(SchemaTypeSet::Single(SchemaType::Integer)),
      ..Default::default()
    }),
  );
  nugget.all_of.push(make_ref("Loaf"));

  let spec = create_test_spec_with_schemas(BTreeMap::from([
    ("Loaf".to_string(), ObjectOrReference::Object(loaf)),
    ("Nugget".to_string(), ObjectOrReference::Object(nugget)),
  ]));

  let mut stats = GenerationStats::default();
  let mut graph = SchemaRegistry::new(&spec, &mut stats);
  let union_fingerprints = BTreeMap::new();
  graph.build_dependencies(&union_fingerprints);
  graph.detect_cycles();

  let merged = graph.merged("Nugget").expect("merged schema should exist for Nugget");

  let prop = merged.schema.properties.get("prop").unwrap();
  if let ObjectOrReference::Object(schema) = prop {
    assert_eq!(
      schema.schema_type,
      Some(SchemaTypeSet::Single(SchemaType::Integer)),
      "nugget property should override loaf"
    );
  } else {
    panic!("Expected Object schema");
  }
}

#[test]
fn schema_merger_merge_multiple_all_of() {
  let mut corgi = make_simple_schema();
  corgi.schema_type = Some(SchemaTypeSet::Single(SchemaType::Object));
  corgi.properties.insert(
    "corgi_prop".to_string(),
    ObjectOrReference::Object(ObjectSchema {
      schema_type: Some(SchemaTypeSet::Single(SchemaType::String)),
      ..Default::default()
    }),
  );
  corgi.required.push("corgi_prop".to_string());

  let mut fluff = make_simple_schema();
  fluff.schema_type = Some(SchemaTypeSet::Single(SchemaType::Object));
  fluff.properties.insert(
    "fluff_prop".to_string(),
    ObjectOrReference::Object(ObjectSchema {
      schema_type: Some(SchemaTypeSet::Single(SchemaType::Integer)),
      ..Default::default()
    }),
  );

  let mut composite = make_simple_schema();
  composite.schema_type = Some(SchemaTypeSet::Single(SchemaType::Object));
  composite.all_of.push(make_ref("Corgi"));
  composite.all_of.push(make_ref("Fluff"));
  composite.properties.insert(
    "own_prop".to_string(),
    ObjectOrReference::Object(ObjectSchema {
      schema_type: Some(SchemaTypeSet::Single(SchemaType::Boolean)),
      ..Default::default()
    }),
  );

  let spec = create_test_spec_with_schemas(BTreeMap::from([
    ("Corgi".to_string(), ObjectOrReference::Object(corgi)),
    ("Fluff".to_string(), ObjectOrReference::Object(fluff)),
    ("Composite".to_string(), ObjectOrReference::Object(composite)),
  ]));

  let mut stats = GenerationStats::default();
  let mut graph = SchemaRegistry::new(&spec, &mut stats);
  let union_fingerprints = BTreeMap::new();
  graph.build_dependencies(&union_fingerprints);
  graph.detect_cycles();

  let merged = graph
    .merged("Composite")
    .expect("merged schema should exist for Composite");

  assert!(
    merged.schema.properties.contains_key("corgi_prop"),
    "should have corgi_prop"
  );
  assert!(
    merged.schema.properties.contains_key("fluff_prop"),
    "should have fluff_prop"
  );
  assert!(
    merged.schema.properties.contains_key("own_prop"),
    "should have own_prop"
  );
  assert!(
    merged.schema.required.contains(&"corgi_prop".to_string()),
    "corgi_prop should be required"
  );
}

fn make_variant_schema_with_const(property_name: &str, const_value: &str) -> ObjectSchema {
  let mut properties = BTreeMap::new();
  properties.insert(
    property_name.to_string(),
    ObjectOrReference::Object(ObjectSchema {
      schema_type: Some(SchemaTypeSet::Single(SchemaType::String)),
      const_value: Some(json!(const_value)),
      ..Default::default()
    }),
  );
  ObjectSchema {
    schema_type: Some(SchemaTypeSet::Single(SchemaType::Object)),
    properties,
    required: vec![property_name.to_string()],
    ..Default::default()
  }
}

#[test]
fn implicit_discriminator_mapping_from_const_values() {
  let allergies = make_variant_schema_with_const("type", "allergies");
  let diet = make_variant_schema_with_const("type", "diet");

  let health = ObjectSchema {
    schema_type: Some(SchemaTypeSet::Single(SchemaType::Object)),
    one_of: vec![make_ref("Allergies"), make_ref("Diet")],
    discriminator: Some(Discriminator {
      property_name: "type".to_string(),
      mapping: None,
    }),
    ..Default::default()
  };

  let spec = create_test_spec_with_schemas(BTreeMap::from([
    ("Allergies".to_string(), ObjectOrReference::Object(allergies)),
    ("Diet".to_string(), ObjectOrReference::Object(diet)),
    ("Health".to_string(), ObjectOrReference::Object(health)),
  ]));

  let mut stats = GenerationStats::default();
  let registry = SchemaRegistry::new(&spec, &mut stats);

  let allergies_mapping = registry.mapping("Allergies");
  assert!(
    allergies_mapping.is_some(),
    "Allergies should have a synthesized mapping"
  );
  let am = allergies_mapping.unwrap();
  assert_eq!(am.field_name, "type", "field_name should be 'type'");
  assert_eq!(am.field_value, "allergies", "field_value should be 'allergies'");

  let diet_mapping = registry.mapping("Diet");
  assert!(diet_mapping.is_some(), "Diet should have a synthesized mapping");
  let dm = diet_mapping.unwrap();
  assert_eq!(dm.field_name, "type", "field_name should be 'type'");
  assert_eq!(dm.field_value, "diet", "field_value should be 'diet'");

  assert!(stats.warnings.is_empty(), "should have no warnings");
}

#[test]
fn implicit_discriminator_mapping_warns_on_missing_const() {
  let allergies = make_variant_schema_with_const("type", "allergies");
  let diet = ObjectSchema {
    schema_type: Some(SchemaTypeSet::Single(SchemaType::Object)),
    properties: BTreeMap::from([(
      "type".to_string(),
      ObjectOrReference::Object(ObjectSchema {
        schema_type: Some(SchemaTypeSet::Single(SchemaType::String)),
        ..Default::default()
      }),
    )]),
    ..Default::default()
  };

  let health = ObjectSchema {
    schema_type: Some(SchemaTypeSet::Single(SchemaType::Object)),
    one_of: vec![make_ref("Allergies"), make_ref("Diet")],
    discriminator: Some(Discriminator {
      property_name: "type".to_string(),
      mapping: None,
    }),
    ..Default::default()
  };

  let spec = create_test_spec_with_schemas(BTreeMap::from([
    ("Allergies".to_string(), ObjectOrReference::Object(allergies)),
    ("Diet".to_string(), ObjectOrReference::Object(diet)),
    ("Health".to_string(), ObjectOrReference::Object(health)),
  ]));

  let mut stats = GenerationStats::default();
  let registry = SchemaRegistry::new(&spec, &mut stats);

  assert!(
    registry.mapping("Allergies").is_none(),
    "no mapping should be synthesized"
  );
  assert!(registry.mapping("Diet").is_none(), "no mapping should be synthesized");
  assert_eq!(stats.warnings.len(), 1, "should have one warning");
  assert!(
    matches!(&stats.warnings[0], GenerationWarning::DiscriminatorMappingFailed { schema_name, message }
      if schema_name == "Health" && message.contains("Diet") && message.contains("no string const")),
    "warning should mention Diet missing const: {:?}",
    stats.warnings[0]
  );
}

#[test]
fn implicit_discriminator_mapping_warns_on_duplicate_const() {
  let allergies = make_variant_schema_with_const("type", "same_value");
  let diet = make_variant_schema_with_const("type", "same_value");

  let health = ObjectSchema {
    schema_type: Some(SchemaTypeSet::Single(SchemaType::Object)),
    one_of: vec![make_ref("Allergies"), make_ref("Diet")],
    discriminator: Some(Discriminator {
      property_name: "type".to_string(),
      mapping: None,
    }),
    ..Default::default()
  };

  let spec = create_test_spec_with_schemas(BTreeMap::from([
    ("Allergies".to_string(), ObjectOrReference::Object(allergies)),
    ("Diet".to_string(), ObjectOrReference::Object(diet)),
    ("Health".to_string(), ObjectOrReference::Object(health)),
  ]));

  let mut stats = GenerationStats::default();
  let registry = SchemaRegistry::new(&spec, &mut stats);

  assert!(
    registry.mapping("Allergies").is_none(),
    "no mapping should be synthesized"
  );
  assert!(registry.mapping("Diet").is_none(), "no mapping should be synthesized");
  assert_eq!(stats.warnings.len(), 1, "should have one warning");
  assert!(
    matches!(&stats.warnings[0], GenerationWarning::DiscriminatorMappingFailed { schema_name, message }
      if schema_name == "Health" && message.contains("duplicate")),
    "warning should mention duplicate: {:?}",
    stats.warnings[0]
  );
}

#[test]
fn explicit_mapping_takes_precedence_over_const() {
  let allergies = make_variant_schema_with_const("type", "allergies");
  let diet = make_variant_schema_with_const("type", "diet");

  let health = ObjectSchema {
    schema_type: Some(SchemaTypeSet::Single(SchemaType::Object)),
    one_of: vec![make_ref("Allergies"), make_ref("Diet")],
    discriminator: Some(Discriminator {
      property_name: "type".to_string(),
      mapping: Some(BTreeMap::from([
        ("allergy_override".to_string(), format!("{SCHEMA_REF_PREFIX}Allergies")),
        ("diet_override".to_string(), format!("{SCHEMA_REF_PREFIX}Diet")),
      ])),
    }),
    ..Default::default()
  };

  let spec = create_test_spec_with_schemas(BTreeMap::from([
    ("Allergies".to_string(), ObjectOrReference::Object(allergies)),
    ("Diet".to_string(), ObjectOrReference::Object(diet)),
    ("Health".to_string(), ObjectOrReference::Object(health)),
  ]));

  let mut stats = GenerationStats::default();
  let registry = SchemaRegistry::new(&spec, &mut stats);

  let am = registry.mapping("Allergies").expect("Allergies should have a mapping");
  assert_eq!(
    am.field_value, "allergy_override",
    "explicit mapping should take precedence"
  );

  let dm = registry.mapping("Diet").expect("Diet should have a mapping");
  assert_eq!(
    dm.field_value, "diet_override",
    "explicit mapping should take precedence"
  );

  assert!(stats.warnings.is_empty(), "should have no warnings");
}

#[test]
fn effective_mapping_synthesizes_from_cache() {
  let allergies = make_variant_schema_with_const("type", "allergies");
  let diet = make_variant_schema_with_const("type", "diet");

  let health = ObjectSchema {
    schema_type: Some(SchemaTypeSet::Single(SchemaType::Object)),
    one_of: vec![make_ref("Allergies"), make_ref("Diet")],
    discriminator: Some(Discriminator {
      property_name: "type".to_string(),
      mapping: None,
    }),
    ..Default::default()
  };

  let spec = create_test_spec_with_schemas(BTreeMap::from([
    ("Allergies".to_string(), ObjectOrReference::Object(allergies)),
    ("Diet".to_string(), ObjectOrReference::Object(diet)),
    ("Health".to_string(), ObjectOrReference::Object(health.clone())),
  ]));

  let mut stats = GenerationStats::default();
  let registry = SchemaRegistry::new(&spec, &mut stats);

  let effective = registry.effective_mapping(&health);
  assert!(effective.is_some(), "effective mapping should be available");

  let mapping = effective.unwrap();
  assert_eq!(mapping.len(), 2, "should have 2 entries");
  assert_eq!(
    mapping.get("allergies").map(std::string::String::as_str),
    Some(format!("{SCHEMA_REF_PREFIX}Allergies").as_str()),
    "allergies value should map to Allergies ref"
  );
  assert_eq!(
    mapping.get("diet").map(std::string::String::as_str),
    Some(format!("{SCHEMA_REF_PREFIX}Diet").as_str()),
    "diet value should map to Diet ref"
  );
}

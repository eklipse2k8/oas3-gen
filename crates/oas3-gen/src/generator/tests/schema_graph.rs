use std::collections::BTreeMap;

use oas3::spec::{
  BooleanSchema, Components, Discriminator, Info, ObjectOrReference, ObjectSchema, Schema, SchemaType, SchemaTypeSet,
  Spec,
};

use crate::generator::schema_registry::{RefCollector, SchemaRegistry};

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
    ("#/components/schemas/User", Some("User")),
    ("#/components/schemas/NestedSchema", Some("NestedSchema")),
    ("#/other/path", None),
    ("InvalidRef", None),
  ];
  for (input, expected) in cases {
    let result = SchemaRegistry::parse_ref(input);
    assert_eq!(result.as_deref(), expected, "failed for input {input:?}");
  }
}

#[test]
fn test_ref_collector() {
  let collector = RefCollector::new(None);

  let schema = make_schema_with_ref("User");
  let refs = collector.collect(&schema);
  assert_eq!(refs.len(), 1, "simple ref: expected 1 ref");
  assert!(refs.contains("User"), "simple ref: should contain User");

  let mut properties = BTreeMap::new();
  properties.insert("author".to_string(), make_ref("User"));
  properties.insert("category".to_string(), make_ref("Category"));
  let schema = ObjectSchema {
    schema_type: None,
    properties,
    ..Default::default()
  };
  let refs = collector.collect(&schema);
  assert_eq!(refs.len(), 2, "multiple refs: expected 2 refs");
  assert!(refs.contains("User"), "multiple refs: should contain User");
  assert!(refs.contains("Category"), "multiple refs: should contain Category");

  let schema = ObjectSchema {
    schema_type: None,
    one_of: vec![make_ref("User")],
    any_of: vec![make_ref("Post")],
    all_of: vec![make_ref("Comment")],
    ..Default::default()
  };
  let refs = collector.collect(&schema);
  assert_eq!(refs.len(), 3, "combinators: expected 3 refs");
  assert!(refs.contains("User"), "combinators: should contain User");
  assert!(refs.contains("Post"), "combinators: should contain Post");
  assert!(refs.contains("Comment"), "combinators: should contain Comment");
}

#[test]
fn test_schema_registry() {
  let mut schemas = BTreeMap::new();
  schemas.insert("User".to_string(), ObjectOrReference::Object(make_simple_schema()));
  schemas.insert("Post".to_string(), ObjectOrReference::Object(make_simple_schema()));

  let spec = create_test_spec_with_schemas(schemas);
  let registry = SchemaRegistry::from_spec(spec).registry;

  assert!(registry.get("User").is_some(), "should have User schema");
  assert!(registry.get("Post").is_some(), "should have Post schema");
  assert!(registry.get("NonExistent").is_none(), "should not have NonExistent");
  assert_eq!(registry.keys().len(), 2, "should have 2 schemas");

  let mut schemas = BTreeMap::new();
  schemas.insert("User".to_string(), ObjectOrReference::Object(make_simple_schema()));
  schemas.insert(
    "Post".to_string(),
    ObjectOrReference::Object(make_schema_with_ref("User")),
  );

  let spec = create_test_spec_with_schemas(schemas);
  let mut graph = SchemaRegistry::from_spec(spec).registry;
  graph.build_dependencies();

  assert_eq!(graph.keys().len(), 2, "build deps: should have 2 schemas");
  assert!(graph.get("User").is_some(), "build deps: should have User");
  assert!(graph.get("Post").is_some(), "build deps: should have Post");
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
    let mut graph = SchemaRegistry::from_spec(spec).registry;
    graph.build_dependencies();
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
    let mut graph = SchemaRegistry::from_spec(spec).registry;
    graph.build_dependencies();
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
    let mut graph = SchemaRegistry::from_spec(spec).registry;
    graph.build_dependencies();
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
    let mut graph = SchemaRegistry::from_spec(spec).registry;
    graph.build_dependencies();
    let cycles = graph.detect_cycles();

    assert!(!cycles.is_empty(), "user-post cycle: should detect cycles");
    assert!(graph.is_cyclic("User"), "user-post cycle: User should be cyclic");
    assert!(graph.is_cyclic("Post"), "user-post cycle: Post should be cyclic");
  }
}

#[test]
fn test_schema_graph_integration() {
  let mut schemas = BTreeMap::new();
  schemas.insert("User".to_string(), ObjectOrReference::Object(make_simple_schema()));
  schemas.insert(
    "Post".to_string(),
    ObjectOrReference::Object(make_schema_with_ref("User")),
  );

  let spec = create_test_spec_with_schemas(schemas);
  let mut graph = SchemaRegistry::from_spec(spec).registry;

  assert!(graph.get("User").is_some(), "integration: should have User");
  assert!(graph.get("Post").is_some(), "integration: should have Post");
  assert_eq!(graph.keys().len(), 2, "integration: should have 2 schemas");

  graph.build_dependencies();
  let cycles = graph.detect_cycles();

  assert!(cycles.is_empty(), "integration: should have no cycles");
  assert!(!graph.is_cyclic("User"), "integration: User should not be cyclic");
}

#[test]
fn test_schema_registry_merges_all_of_properties_and_required() {
  let mut parent = make_simple_schema();
  parent.schema_type = Some(SchemaTypeSet::Single(SchemaType::Object));
  parent.required.push("id".to_string());
  parent.properties.insert(
    "id".to_string(),
    ObjectOrReference::Object(ObjectSchema {
      schema_type: Some(SchemaTypeSet::Single(SchemaType::Integer)),
      ..Default::default()
    }),
  );
  parent.additional_properties = Some(Schema::Boolean(BooleanSchema(true)));

  let mut child = make_simple_schema();
  child.schema_type = Some(SchemaTypeSet::Single(SchemaType::Object));
  child.required.push("name".to_string());
  child.properties.insert(
    "name".to_string(),
    ObjectOrReference::Object(ObjectSchema {
      schema_type: Some(SchemaTypeSet::Single(SchemaType::String)),
      ..Default::default()
    }),
  );
  child.all_of.push(make_ref("Parent"));

  let spec = create_test_spec_with_schemas(BTreeMap::from([
    ("Parent".to_string(), ObjectOrReference::Object(parent.clone())),
    ("Child".to_string(), ObjectOrReference::Object(child.clone())),
  ]));

  let mut graph = SchemaRegistry::from_spec(spec).registry;
  graph.build_dependencies();
  graph.detect_cycles();

  let merged = graph.merged("Child").expect("merged schema should exist for Child");

  assert!(merged.schema.properties.contains_key("id"));
  assert!(merged.schema.properties.contains_key("name"));
  assert!(merged.schema.required.contains(&"id".to_string()));
  assert!(merged.schema.required.contains(&"name".to_string()));
  assert!(merged.schema.additional_properties.is_some());
}

#[test]
fn test_schema_registry_merges_and_tracks_discriminator_parents() {
  let mut parent_schema = make_simple_schema();
  parent_schema.properties.insert(
    "kind".to_string(),
    ObjectOrReference::Object(ObjectSchema {
      schema_type: Some(SchemaTypeSet::Single(SchemaType::String)),
      ..Default::default()
    }),
  );
  parent_schema.discriminator = Some(Discriminator {
    property_name: "kind".to_string(),
    mapping: Some(BTreeMap::from([(
      "child".to_string(),
      format!("{SCHEMA_REF_PREFIX}Child"),
    )])),
  });

  let mut child_schema = make_simple_schema();
  child_schema.properties.insert(
    "child_prop".to_string(),
    ObjectOrReference::Object(ObjectSchema {
      schema_type: Some(SchemaTypeSet::Single(SchemaType::Integer)),
      ..Default::default()
    }),
  );
  child_schema.all_of.push(make_ref("Parent"));

  let spec = create_test_spec_with_schemas(BTreeMap::from([
    ("Parent".to_string(), ObjectOrReference::Object(parent_schema.clone())),
    ("Child".to_string(), ObjectOrReference::Object(child_schema.clone())),
  ]));

  let mut graph = SchemaRegistry::from_spec(spec).registry;
  graph.build_dependencies();
  graph.detect_cycles();

  let merged_child = graph.merged("Child").expect("merged schema should exist for Child");

  assert_eq!(merged_child.discriminator_parent.as_deref(), Some("Parent"));
  assert!(merged_child.schema.properties.contains_key("kind"));
  assert!(merged_child.schema.properties.contains_key("child_prop"));

  let discriminator = graph.parent("Child").expect("discriminator parent should be tracked");

  assert_eq!(discriminator.parent_name, "Parent");

  let effective = graph.resolved("Child").unwrap();
  assert_eq!(effective.properties.len(), merged_child.schema.properties.len());
}

#[test]
fn schema_merger_merge_child_with_parent() {
  let mut parent = make_simple_schema();
  parent.schema_type = Some(SchemaTypeSet::Single(SchemaType::Object));
  parent.properties.insert(
    "parent_prop".to_string(),
    ObjectOrReference::Object(ObjectSchema {
      schema_type: Some(SchemaTypeSet::Single(SchemaType::String)),
      ..Default::default()
    }),
  );
  parent.required.push("parent_prop".to_string());

  let mut child = make_simple_schema();
  child.schema_type = Some(SchemaTypeSet::Single(SchemaType::Object));
  child.properties.insert(
    "child_prop".to_string(),
    ObjectOrReference::Object(ObjectSchema {
      schema_type: Some(SchemaTypeSet::Single(SchemaType::Integer)),
      ..Default::default()
    }),
  );
  child.all_of.push(make_ref("Parent"));

  let spec = create_test_spec_with_schemas(BTreeMap::from([
    ("Parent".to_string(), ObjectOrReference::Object(parent)),
    ("Child".to_string(), ObjectOrReference::Object(child)),
  ]));

  let mut graph = SchemaRegistry::from_spec(spec).registry;
  graph.build_dependencies();
  graph.detect_cycles();

  let merged = graph.merged("Child").expect("merged schema should exist for Child");

  assert!(
    merged.schema.properties.contains_key("parent_prop"),
    "should have parent_prop"
  );
  assert!(
    merged.schema.properties.contains_key("child_prop"),
    "should have child_prop"
  );
  assert!(
    merged.schema.required.contains(&"parent_prop".to_string()),
    "parent_prop should be required"
  );

  let effective = graph.resolved("Child").unwrap();
  assert_eq!(
    effective.properties.len(),
    merged.schema.properties.len(),
    "resolved should match merged"
  );
}

#[test]
fn schema_merger_conflict_resolution() {
  let mut parent = make_simple_schema();
  parent.schema_type = Some(SchemaTypeSet::Single(SchemaType::Object));
  parent.properties.insert(
    "prop".to_string(),
    ObjectOrReference::Object(ObjectSchema {
      schema_type: Some(SchemaTypeSet::Single(SchemaType::String)),
      ..Default::default()
    }),
  );

  let mut child = make_simple_schema();
  child.schema_type = Some(SchemaTypeSet::Single(SchemaType::Object));
  child.properties.insert(
    "prop".to_string(),
    ObjectOrReference::Object(ObjectSchema {
      schema_type: Some(SchemaTypeSet::Single(SchemaType::Integer)),
      ..Default::default()
    }),
  );
  child.all_of.push(make_ref("Parent"));

  let spec = create_test_spec_with_schemas(BTreeMap::from([
    ("Parent".to_string(), ObjectOrReference::Object(parent)),
    ("Child".to_string(), ObjectOrReference::Object(child)),
  ]));

  let mut graph = SchemaRegistry::from_spec(spec).registry;
  graph.build_dependencies();
  graph.detect_cycles();

  let merged = graph.merged("Child").expect("merged schema should exist for Child");

  let prop = merged.schema.properties.get("prop").unwrap();
  if let ObjectOrReference::Object(schema) = prop {
    assert_eq!(
      schema.schema_type,
      Some(SchemaTypeSet::Single(SchemaType::Integer)),
      "child property should override parent"
    );
  } else {
    panic!("Expected Object schema");
  }
}

#[test]
fn schema_merger_merge_multiple_all_of() {
  let mut base = make_simple_schema();
  base.schema_type = Some(SchemaTypeSet::Single(SchemaType::Object));
  base.properties.insert(
    "base_prop".to_string(),
    ObjectOrReference::Object(ObjectSchema {
      schema_type: Some(SchemaTypeSet::Single(SchemaType::String)),
      ..Default::default()
    }),
  );
  base.required.push("base_prop".to_string());

  let mut mixin = make_simple_schema();
  mixin.schema_type = Some(SchemaTypeSet::Single(SchemaType::Object));
  mixin.properties.insert(
    "mixin_prop".to_string(),
    ObjectOrReference::Object(ObjectSchema {
      schema_type: Some(SchemaTypeSet::Single(SchemaType::Integer)),
      ..Default::default()
    }),
  );

  let mut composite = make_simple_schema();
  composite.schema_type = Some(SchemaTypeSet::Single(SchemaType::Object));
  composite.all_of.push(make_ref("Base"));
  composite.all_of.push(make_ref("Mixin"));
  composite.properties.insert(
    "own_prop".to_string(),
    ObjectOrReference::Object(ObjectSchema {
      schema_type: Some(SchemaTypeSet::Single(SchemaType::Boolean)),
      ..Default::default()
    }),
  );

  let spec = create_test_spec_with_schemas(BTreeMap::from([
    ("Base".to_string(), ObjectOrReference::Object(base)),
    ("Mixin".to_string(), ObjectOrReference::Object(mixin)),
    ("Composite".to_string(), ObjectOrReference::Object(composite)),
  ]));

  let mut graph = SchemaRegistry::from_spec(spec).registry;
  graph.build_dependencies();
  graph.detect_cycles();

  let merged = graph
    .merged("Composite")
    .expect("merged schema should exist for Composite");

  assert!(
    merged.schema.properties.contains_key("base_prop"),
    "should have base_prop"
  );
  assert!(
    merged.schema.properties.contains_key("mixin_prop"),
    "should have mixin_prop"
  );
  assert!(
    merged.schema.properties.contains_key("own_prop"),
    "should have own_prop"
  );
  assert!(
    merged.schema.required.contains(&"base_prop".to_string()),
    "base_prop should be required"
  );
}

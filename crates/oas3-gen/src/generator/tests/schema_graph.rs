use std::collections::{BTreeMap, BTreeSet, HashMap};

use oas3::spec::{Components, ObjectOrReference, ObjectSchema, Spec};

use crate::generator::schema_graph::{
  CycleDetector, DependencyGraph, ReferenceExtractor, SchemaGraph, SchemaRepository,
};

const SCHEMA_REF_PREFIX: &str = "#/components/schemas/";

fn create_test_spec_with_schemas(schemas: BTreeMap<String, ObjectOrReference<ObjectSchema>>) -> Spec {
  Spec {
    openapi: "3.0.0".to_string(),
    info: oas3::spec::Info {
      title: "Test".to_string(),
      summary: None,
      version: "1.0.0".to_string(),
      description: None,
      terms_of_service: None,
      contact: None,
      license: None,
      extensions: BTreeMap::default(),
    },
    servers: Vec::new(),
    paths: Option::default(),
    webhooks: BTreeMap::default(),
    components: Some(Components {
      schemas,
      ..Default::default()
    }),
    security: Vec::new(),
    tags: Vec::new(),
    external_docs: None,
    extensions: BTreeMap::default(),
  }
}

fn create_simple_schema() -> ObjectSchema {
  ObjectSchema {
    schema_type: None,
    properties: BTreeMap::new(),
    ..Default::default()
  }
}

fn create_schema_with_ref(ref_name: &str) -> ObjectSchema {
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

#[test]
fn test_schema_repository_from_spec() {
  let mut schemas = BTreeMap::new();
  schemas.insert("User".to_string(), ObjectOrReference::Object(create_simple_schema()));
  schemas.insert("Post".to_string(), ObjectOrReference::Object(create_simple_schema()));

  let spec = create_test_spec_with_schemas(schemas);
  let (repo, _warnings) = SchemaRepository::from_spec(&spec);

  assert!(repo.get("User").is_some());
  assert!(repo.get("Post").is_some());
  assert!(repo.get("NonExistent").is_none());
  assert_eq!(repo.names().count(), 2);
}

#[test]
fn test_reference_extractor() {
  // Simple ref
  let schema = create_schema_with_ref("User");
  let refs = ReferenceExtractor::extract_from_schema(&schema, None);
  assert_eq!(refs.len(), 1);
  assert!(refs.contains("User"));

  // Multiple refs
  let mut properties = BTreeMap::new();
  properties.insert(
    "author".to_string(),
    ObjectOrReference::Ref {
      ref_path: format!("{SCHEMA_REF_PREFIX}User"),
      summary: None,
      description: None,
    },
  );
  properties.insert(
    "category".to_string(),
    ObjectOrReference::Ref {
      ref_path: format!("{SCHEMA_REF_PREFIX}Category"),
      summary: None,
      description: None,
    },
  );
  let schema = ObjectSchema {
    schema_type: None,
    properties,
    ..Default::default()
  };
  let refs = ReferenceExtractor::extract_from_schema(&schema, None);
  assert_eq!(refs.len(), 2);
  assert!(refs.contains("User"));
  assert!(refs.contains("Category"));

  // Combinators
  let user_ref = ObjectOrReference::Ref {
    ref_path: format!("{SCHEMA_REF_PREFIX}User"),
    summary: None,
    description: None,
  };
  let post_ref = ObjectOrReference::Ref {
    ref_path: format!("{SCHEMA_REF_PREFIX}Post"),
    summary: None,
    description: None,
  };
  let comment_ref = ObjectOrReference::Ref {
    ref_path: format!("{SCHEMA_REF_PREFIX}Comment"),
    summary: None,
    description: None,
  };
  let schema = ObjectSchema {
    schema_type: None,
    one_of: vec![user_ref],
    any_of: vec![post_ref],
    all_of: vec![comment_ref],
    ..Default::default()
  };
  let refs = ReferenceExtractor::extract_from_schema(&schema, None);
  assert_eq!(refs.len(), 3);
  assert!(refs.contains("User"));
  assert!(refs.contains("Post"));
  assert!(refs.contains("Comment"));
}

#[test]
fn test_cycle_detector() {
  // No cycles
  let mut deps = BTreeMap::new();
  deps.insert("A".to_string(), BTreeSet::new());
  deps.insert("B".to_string(), BTreeSet::from(["A".to_string()]));
  deps.insert("C".to_string(), BTreeSet::from(["B".to_string()]));
  let mut detector = CycleDetector::new(&deps);
  let cycles = detector.find_all_cycles();
  assert!(cycles.is_empty());

  // Simple cycle
  let mut deps = BTreeMap::new();
  deps.insert("A".to_string(), BTreeSet::from(["B".to_string()]));
  deps.insert("B".to_string(), BTreeSet::from(["A".to_string()]));
  let mut detector = CycleDetector::new(&deps);
  let cycles = detector.find_all_cycles();
  assert_eq!(cycles.len(), 1);
  assert!(!cycles[0].is_empty());

  // Self reference
  let mut deps = BTreeMap::new();
  deps.insert("A".to_string(), BTreeSet::from(["A".to_string()]));
  let mut detector = CycleDetector::new(&deps);
  let cycles = detector.find_all_cycles();
  assert_eq!(cycles.len(), 1);
}

#[test]
fn test_dependency_graph() {
  // Build
  let mut schemas = BTreeMap::new();
  schemas.insert("User".to_string(), ObjectOrReference::Object(create_simple_schema()));
  schemas.insert(
    "Post".to_string(),
    ObjectOrReference::Object(create_schema_with_ref("User")),
  );
  let spec = create_test_spec_with_schemas(schemas);
  let (repo, _warnings) = SchemaRepository::from_spec(&spec);
  let mut graph = DependencyGraph::new();
  graph.build(&repo, &HashMap::new());
  assert_eq!(graph.dependencies.len(), 2);
  assert!(graph.dependencies.get("User").unwrap().is_empty());
  assert_eq!(graph.dependencies.get("Post").unwrap().len(), 1);

  // Detect cycles
  let mut schemas = BTreeMap::new();
  let mut user_schema = create_simple_schema();
  user_schema.properties.insert(
    "posts".to_string(),
    ObjectOrReference::Ref {
      ref_path: format!("{SCHEMA_REF_PREFIX}Post"),
      summary: None,
      description: None,
    },
  );
  let mut post_schema = create_simple_schema();
  post_schema.properties.insert(
    "author".to_string(),
    ObjectOrReference::Ref {
      ref_path: format!("{SCHEMA_REF_PREFIX}User"),
      summary: None,
      description: None,
    },
  );
  schemas.insert("User".to_string(), ObjectOrReference::Object(user_schema));
  schemas.insert("Post".to_string(), ObjectOrReference::Object(post_schema));
  let spec = create_test_spec_with_schemas(schemas);
  let (repo, _warnings) = SchemaRepository::from_spec(&spec);
  let mut graph = DependencyGraph::new();
  graph.build(&repo, &HashMap::new());
  let cycles = graph.detect_cycles();
  assert!(!cycles.is_empty());
  assert!(graph.is_cyclic("User"));
  assert!(graph.is_cyclic("Post"));
}

#[test]
fn test_schema_graph_integration() {
  let mut schemas = BTreeMap::new();
  schemas.insert("User".to_string(), ObjectOrReference::Object(create_simple_schema()));
  schemas.insert(
    "Post".to_string(),
    ObjectOrReference::Object(create_schema_with_ref("User")),
  );

  let spec = create_test_spec_with_schemas(schemas);
  let (mut graph, _warnings) = SchemaGraph::new(spec);

  assert!(graph.get_schema("User").is_some());
  assert!(graph.get_schema("Post").is_some());
  assert_eq!(graph.schema_names().len(), 2);

  graph.build_dependencies();
  let cycles = graph.detect_cycles();
  assert!(cycles.is_empty());
  assert!(!graph.is_cyclic("User"));
}

#[test]
fn test_extract_ref_name() {
  assert_eq!(
    SchemaGraph::extract_ref_name("#/components/schemas/User"),
    Some("User".to_string())
  );
  assert_eq!(
    SchemaGraph::extract_ref_name("#/components/schemas/NestedSchema"),
    Some("NestedSchema".to_string())
  );
  assert_eq!(SchemaGraph::extract_ref_name("#/other/path"), None);
  assert_eq!(SchemaGraph::extract_ref_name("InvalidRef"), None);
}

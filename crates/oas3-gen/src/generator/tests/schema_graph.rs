use std::collections::BTreeMap;

use oas3::spec::{Components, ObjectOrReference, ObjectSchema, Spec};

use crate::generator::schema_registry::{ReferenceExtractor, SchemaRegistry};

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
fn test_schema_registry_from_spec() {
  let mut schemas = BTreeMap::new();
  schemas.insert("User".to_string(), ObjectOrReference::Object(create_simple_schema()));
  schemas.insert("Post".to_string(), ObjectOrReference::Object(create_simple_schema()));

  let spec = create_test_spec_with_schemas(schemas);
  let (registry, _) = SchemaRegistry::new(spec);

  assert!(registry.get_schema("User").is_some());
  assert!(registry.get_schema("Post").is_some());
  assert!(registry.get_schema("NonExistent").is_none());
  assert_eq!(registry.schema_names().len(), 2);
}

#[test]
fn test_reference_extractor_simple_ref() {
  let schema = create_schema_with_ref("User");
  let refs = ReferenceExtractor::extract_from_schema(&schema, None);

  assert_eq!(refs.len(), 1);
  assert!(refs.contains("User"));
}

#[test]
fn test_reference_extractor_multiple_refs() {
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
}

#[test]
fn test_reference_extractor_combinators() {
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
fn test_schema_graph_no_cycles() {
  let mut schemas = BTreeMap::new();
  schemas.insert("A".to_string(), ObjectOrReference::Object(create_simple_schema()));
  schemas.insert("B".to_string(), ObjectOrReference::Object(create_schema_with_ref("A")));
  let mut c_schema = create_simple_schema();
  c_schema.properties.insert(
    "b".to_string(),
    ObjectOrReference::Ref {
      ref_path: format!("{SCHEMA_REF_PREFIX}B"),
      summary: None,
      description: None,
    },
  );
  schemas.insert("C".to_string(), ObjectOrReference::Object(c_schema));

  let spec = create_test_spec_with_schemas(schemas);
  let (mut graph, _) = SchemaRegistry::new(spec);

  graph.build_dependencies();
  let cycles = graph.detect_cycles();

  assert!(cycles.is_empty());
  assert!(!graph.is_cyclic("A"));
  assert!(!graph.is_cyclic("B"));
  assert!(!graph.is_cyclic("C"));
}

#[test]
fn test_schema_graph_simple_cycle() {
  let mut schemas = BTreeMap::new();
  let mut a_schema = create_simple_schema();
  a_schema.properties.insert(
    "b".to_string(),
    ObjectOrReference::Ref {
      ref_path: format!("{SCHEMA_REF_PREFIX}B"),
      summary: None,
      description: None,
    },
  );
  let mut b_schema = create_simple_schema();
  b_schema.properties.insert(
    "a".to_string(),
    ObjectOrReference::Ref {
      ref_path: format!("{SCHEMA_REF_PREFIX}A"),
      summary: None,
      description: None,
    },
  );
  schemas.insert("A".to_string(), ObjectOrReference::Object(a_schema));
  schemas.insert("B".to_string(), ObjectOrReference::Object(b_schema));

  let spec = create_test_spec_with_schemas(schemas);
  let (mut graph, _) = SchemaRegistry::new(spec);

  graph.build_dependencies();
  let cycles = graph.detect_cycles();

  assert_eq!(cycles.len(), 1);
  assert!(!cycles[0].is_empty());
  assert!(graph.is_cyclic("A"));
  assert!(graph.is_cyclic("B"));
}

#[test]
fn test_schema_graph_self_reference() {
  let mut schemas = BTreeMap::new();
  let mut a_schema = create_simple_schema();
  a_schema.properties.insert(
    "self_ref".to_string(),
    ObjectOrReference::Ref {
      ref_path: format!("{SCHEMA_REF_PREFIX}A"),
      summary: None,
      description: None,
    },
  );
  schemas.insert("A".to_string(), ObjectOrReference::Object(a_schema));

  let spec = create_test_spec_with_schemas(schemas);
  let (mut graph, _) = SchemaRegistry::new(spec);

  graph.build_dependencies();
  let cycles = graph.detect_cycles();

  assert_eq!(cycles.len(), 1);
  assert!(graph.is_cyclic("A"));
}

#[test]
fn test_schema_graph_build_dependencies() {
  let mut schemas = BTreeMap::new();
  schemas.insert("User".to_string(), ObjectOrReference::Object(create_simple_schema()));
  schemas.insert(
    "Post".to_string(),
    ObjectOrReference::Object(create_schema_with_ref("User")),
  );

  let spec = create_test_spec_with_schemas(schemas);
  let (mut graph, _) = SchemaRegistry::new(spec);

  graph.build_dependencies();

  assert_eq!(graph.schema_names().len(), 2);
  assert!(graph.get_schema("User").is_some());
  assert!(graph.get_schema("Post").is_some());
}

#[test]
fn test_schema_graph_detect_cycles() {
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
  let (mut graph, _) = SchemaRegistry::new(spec);

  graph.build_dependencies();
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
  let (mut graph, _) = SchemaRegistry::new(spec);

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
    SchemaRegistry::extract_ref_name("#/components/schemas/User"),
    Some("User".to_string())
  );
  assert_eq!(
    SchemaRegistry::extract_ref_name("#/components/schemas/NestedSchema"),
    Some("NestedSchema".to_string())
  );
  assert_eq!(SchemaRegistry::extract_ref_name("#/other/path"), None);
  assert_eq!(SchemaRegistry::extract_ref_name("InvalidRef"), None);
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
fn test_schema_graph_dependencies() {
  let mut schemas = BTreeMap::new();
  schemas.insert("User".to_string(), ObjectOrReference::Object(create_simple_schema()));
  schemas.insert(
    "Post".to_string(),
    ObjectOrReference::Object(create_schema_with_ref("User")),
  );
  let spec = create_test_spec_with_schemas(schemas);
  let (mut graph, _) = SchemaRegistry::new(spec);
  graph.build_dependencies();
  assert_eq!(graph.schema_names().len(), 2);
  assert!(graph.get_schema("User").is_some());
  assert!(graph.get_schema("Post").is_some());

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
  let (mut graph, _) = SchemaRegistry::new(spec);
  graph.build_dependencies();
  let cycles = graph.detect_cycles();
  assert!(!cycles.is_empty());
  assert!(graph.is_cyclic("User"));
  assert!(graph.is_cyclic("Post"));
}

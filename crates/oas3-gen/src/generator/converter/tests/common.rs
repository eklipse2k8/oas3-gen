use std::collections::BTreeMap;

use oas3::spec::{ObjectSchema, Spec};
use serde_json::json;

use crate::generator::schema_graph::SchemaGraph;

pub(crate) fn create_test_spec(schemas: BTreeMap<String, ObjectSchema>) -> Spec {
  let mut spec_json = json!({
    "openapi": "3.0.0",
    "info": { "title": "Test API", "version": "1.0.0" },
    "paths": {},
    "components": { "schemas": {} }
  });

  let schemas_obj = spec_json["components"]["schemas"].as_object_mut().unwrap();
  for (name, schema) in schemas {
    schemas_obj.insert(name, serde_json::to_value(schema).unwrap());
  }

  serde_json::from_value(spec_json).unwrap()
}

pub(crate) fn create_test_graph(schemas: BTreeMap<String, ObjectSchema>) -> SchemaGraph {
  let spec = create_test_spec(schemas);
  let mut graph = SchemaGraph::new(spec).unwrap();
  graph.build_dependencies();
  graph.detect_cycles();
  graph
}

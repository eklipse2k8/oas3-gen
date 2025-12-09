use std::{collections::BTreeMap, sync::Arc};

use oas3::spec::{ObjectSchema, Spec};
use serde_json::json;

use crate::generator::{converter::CodegenConfig, schema_registry::SchemaRegistry};

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

pub(crate) fn create_test_graph(schemas: BTreeMap<String, ObjectSchema>) -> Arc<SchemaRegistry> {
  let spec = create_test_spec(schemas);
  let (mut graph, _) = SchemaRegistry::new(spec);
  graph.build_dependencies();
  graph.detect_cycles();
  Arc::new(graph)
}

pub(crate) fn default_config() -> CodegenConfig {
  CodegenConfig {
    preserve_case_variants: false,
    case_insensitive_enums: false,
    no_helpers: false,
    odata_support: false,
  }
}

pub(crate) fn config_with_preserve_case() -> CodegenConfig {
  CodegenConfig {
    preserve_case_variants: true,
    case_insensitive_enums: false,
    no_helpers: false,
    odata_support: false,
  }
}

pub(crate) fn config_with_no_helpers() -> CodegenConfig {
  CodegenConfig {
    preserve_case_variants: false,
    case_insensitive_enums: false,
    no_helpers: true,
    odata_support: false,
  }
}

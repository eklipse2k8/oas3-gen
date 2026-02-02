use std::{collections::BTreeMap, rc::Rc, sync::Arc};

use oas3::spec::{ObjectOrReference, ObjectSchema, SchemaType, SchemaTypeSet, Spec};
use serde_json::json;

use crate::generator::{
  ast::{Documentation, FieldDef, FieldNameToken, RustPrimitive, TypeRef},
  converter::{
    CodegenConfig, ConverterContext, EnumCasePolicy, EnumHelperPolicy, SchemaConverter, cache::SharedSchemaCache,
  },
  metrics::GenerationStats,
  schema_registry::SchemaRegistry,
};

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
  let mut stats = GenerationStats::default();
  let mut graph = SchemaRegistry::new(&spec, &mut stats);
  let union_fingerprints = BTreeMap::new();
  graph.build_dependencies(&union_fingerprints);
  graph.detect_cycles();
  Arc::new(graph)
}

pub(crate) fn default_config() -> CodegenConfig {
  CodegenConfig::default()
}

pub(crate) fn config_with_preserve_case() -> CodegenConfig {
  CodegenConfig {
    enum_case: EnumCasePolicy::Preserve,
    ..Default::default()
  }
}

pub(crate) fn config_with_no_helpers() -> CodegenConfig {
  CodegenConfig {
    enum_helpers: EnumHelperPolicy::Disable,
    ..Default::default()
  }
}

pub(crate) fn create_test_context(graph: Arc<SchemaRegistry>, config: CodegenConfig) -> Rc<ConverterContext> {
  let cache = SharedSchemaCache::new();
  Rc::new(ConverterContext::new(graph, config, cache, None))
}

pub(crate) fn create_schema_converter(context: &Rc<ConverterContext>) -> SchemaConverter {
  SchemaConverter::new(context)
}

pub(crate) fn make_string_schema() -> ObjectSchema {
  ObjectSchema {
    schema_type: Some(SchemaTypeSet::Single(SchemaType::String)),
    ..Default::default()
  }
}

pub(crate) fn make_object_schema_with_property(prop_name: &str, prop_schema: ObjectSchema) -> ObjectSchema {
  ObjectSchema {
    schema_type: Some(SchemaTypeSet::Single(SchemaType::Object)),
    properties: BTreeMap::from([(prop_name.to_string(), ObjectOrReference::Object(prop_schema))]),
    ..Default::default()
  }
}

pub(crate) fn create_empty_test_graph() -> Arc<SchemaRegistry> {
  create_test_graph(BTreeMap::new())
}

pub(crate) fn make_docs() -> Documentation {
  Documentation::from_lines(["Some docs"])
}

pub(crate) fn make_field(name: &str, deprecated: bool) -> FieldDef {
  FieldDef::builder()
    .name(FieldNameToken::from_raw(name))
    .rust_type(TypeRef::new(RustPrimitive::String))
    .docs(make_docs())
    .deprecated(deprecated)
    .build()
}

use std::collections::BTreeMap;

use oas3::spec::{ObjectOrReference, ObjectSchema, SchemaType, SchemaTypeSet};
use serde_json::json;

use crate::{
  generator::{
    converter::type_resolver::{SchemaExt, TypeResolver},
    schema_graph::SchemaGraph,
  },
  tests::common::{create_test_graph, default_config},
};

#[test]
fn test_title_ignored_when_schema_type_present() {
  let message_schema = ObjectSchema {
    schema_type: Some(SchemaTypeSet::Single(SchemaType::Object)),
    properties: BTreeMap::from([(
      "content".to_string(),
      ObjectOrReference::Object(ObjectSchema {
        schema_type: Some(SchemaTypeSet::Single(SchemaType::String)),
        ..Default::default()
      }),
    )]),
    ..Default::default()
  };

  let graph = create_test_graph(BTreeMap::from([("Message".to_string(), message_schema)]));
  let resolver = TypeResolver::new(&graph, default_config());

  let schema = ObjectSchema {
    title: Some("Message".to_string()),
    schema_type: Some(SchemaTypeSet::Single(SchemaType::String)),
    ..Default::default()
  };

  let result = resolver.schema_to_type_ref(&schema).unwrap();
  assert_eq!(result.to_rust_type(), "String");
}

#[test]
fn test_title_used_when_no_schema_type() {
  let custom_schema = ObjectSchema {
    schema_type: Some(SchemaTypeSet::Single(SchemaType::Object)),
    properties: BTreeMap::from([(
      "field".to_string(),
      ObjectOrReference::Object(ObjectSchema {
        schema_type: Some(SchemaTypeSet::Single(SchemaType::String)),
        ..Default::default()
      }),
    )]),
    ..Default::default()
  };

  let graph = create_test_graph(BTreeMap::from([("CustomType".to_string(), custom_schema)]));
  let resolver = TypeResolver::new(&graph, default_config());

  let schema = ObjectSchema {
    title: Some("CustomType".to_string()),
    ..Default::default()
  };

  let result = resolver.schema_to_type_ref(&schema).unwrap();
  assert_eq!(result.to_rust_type(), "CustomType");
}

#[test]
fn test_anyof_with_enum_values_and_freeform_string() {
  let graph = create_test_graph(BTreeMap::new());

  let enum_schema = ObjectSchema {
    schema_type: Some(SchemaTypeSet::Single(SchemaType::String)),
    enum_values: vec![json!("value1"), json!("value2")],
    ..Default::default()
  };

  let freeform_schema = ObjectSchema {
    schema_type: Some(SchemaTypeSet::Single(SchemaType::String)),
    ..Default::default()
  };

  let parent_schema = ObjectSchema {
    any_of: vec![
      ObjectOrReference::Object(enum_schema),
      ObjectOrReference::Object(freeform_schema),
    ],
    ..Default::default()
  };

  let variants = &parent_schema.any_of;
  let has_freeform = variants.iter().any(|v| {
    v.resolve(graph.spec()).ok().is_some_and(|resolved| {
      resolved.const_value.is_none()
        && resolved.enum_values.is_empty()
        && resolved.schema_type == Some(SchemaTypeSet::Single(SchemaType::String))
    })
  });

  assert!(has_freeform);
}

#[test]
fn test_anyof_without_freeform_string() {
  let graph = create_test_graph(BTreeMap::new());

  let enum_schema1 = ObjectSchema {
    schema_type: Some(SchemaTypeSet::Single(SchemaType::String)),
    enum_values: vec![json!("value1")],
    ..Default::default()
  };

  let enum_schema2 = ObjectSchema {
    schema_type: Some(SchemaTypeSet::Single(SchemaType::String)),
    enum_values: vec![json!("value2")],
    ..Default::default()
  };

  let parent_schema = ObjectSchema {
    any_of: vec![
      ObjectOrReference::Object(enum_schema1),
      ObjectOrReference::Object(enum_schema2),
    ],
    ..Default::default()
  };

  let variants = &parent_schema.any_of;
  let has_freeform = variants.iter().any(|v| {
    v.resolve(graph.spec()).ok().is_some_and(|resolved| {
      resolved.const_value.is_none()
        && resolved.enum_values.is_empty()
        && resolved.schema_type == Some(SchemaTypeSet::Single(SchemaType::String))
    })
  });

  assert!(!has_freeform);
}

#[test]
fn test_anyof_with_nested_oneof_resolves_to_ref() {
  let cache_schema = ObjectSchema {
    schema_type: Some(SchemaTypeSet::Single(SchemaType::Object)),
    properties: BTreeMap::from([(
      "type".to_string(),
      ObjectOrReference::Object(ObjectSchema {
        schema_type: Some(SchemaTypeSet::Single(SchemaType::String)),
        const_value: Some(json!("ephemeral")),
        ..Default::default()
      }),
    )]),
    ..Default::default()
  };

  let graph = create_test_graph(BTreeMap::from([("CacheControlEphemeral".to_string(), cache_schema)]));
  let resolver = TypeResolver::new(&graph, default_config());

  let inner_schema = ObjectSchema {
    one_of: vec![ObjectOrReference::Ref {
      ref_path: "#/components/schemas/CacheControlEphemeral".to_string(),
      summary: None,
      description: None,
    }],
    ..Default::default()
  };

  let null_schema = ObjectSchema {
    schema_type: Some(SchemaTypeSet::Single(SchemaType::Null)),
    ..Default::default()
  };

  let variants = vec![
    ObjectOrReference::Object(inner_schema),
    ObjectOrReference::Object(null_schema),
  ];

  let result = resolver.try_convert_union_to_type_ref(&variants).unwrap();
  assert!(result.is_some());
  let type_ref = result.unwrap();
  assert_eq!(type_ref.to_rust_type(), "Option<CacheControlEphemeral>");
}

#[test]
fn test_anyof_with_no_resolvable_variants() {
  let graph = create_test_graph(BTreeMap::new());
  let resolver = TypeResolver::new(&graph, default_config());

  let null_schema1 = ObjectSchema {
    schema_type: Some(SchemaTypeSet::Single(SchemaType::Null)),
    ..Default::default()
  };

  let null_schema2 = ObjectSchema {
    schema_type: Some(SchemaTypeSet::Single(SchemaType::Null)),
    ..Default::default()
  };

  let variants = vec![
    ObjectOrReference::Object(null_schema1),
    ObjectOrReference::Object(null_schema2),
  ];

  let result = resolver.try_convert_union_to_type_ref(&variants).unwrap();
  assert!(result.is_none());
}

#[test]
fn test_array_with_items() {
  let graph = create_test_graph(BTreeMap::new());
  let resolver = TypeResolver::new(&graph, default_config());

  let schema = ObjectSchema {
    schema_type: Some(SchemaTypeSet::Single(SchemaType::Array)),
    items: Some(Box::new(oas3::spec::Schema::Object(Box::new(
      ObjectOrReference::Object(ObjectSchema {
        schema_type: Some(SchemaTypeSet::Single(SchemaType::String)),
        ..Default::default()
      }),
    )))),
    ..Default::default()
  };

  let result = resolver.schema_to_type_ref(&schema).unwrap();
  assert_eq!(result.to_rust_type(), "Vec<String>");
}

#[test]
fn test_array_without_items_fallback() {
  let graph = create_test_graph(BTreeMap::new());
  let resolver = TypeResolver::new(&graph, default_config());

  let schema = ObjectSchema {
    schema_type: Some(SchemaTypeSet::Single(SchemaType::Array)),
    items: None,
    ..Default::default()
  };

  let result = resolver.schema_to_type_ref(&schema).unwrap();
  assert_eq!(result.to_rust_type(), "Vec<serde_json::Value>");
}

#[test]
fn test_array_with_boolean_schema_items() {
  let graph = create_test_graph(BTreeMap::new());
  let resolver = TypeResolver::new(&graph, default_config());

  let schema = ObjectSchema {
    schema_type: Some(SchemaTypeSet::Single(SchemaType::Array)),
    items: Some(Box::new(oas3::spec::Schema::Boolean(oas3::spec::BooleanSchema(true)))),
    ..Default::default()
  };

  let result = resolver.schema_to_type_ref(&schema).unwrap();
  assert_eq!(result.to_rust_type(), "Vec<serde_json::Value>");
}

#[test]
fn test_array_with_ref_items() {
  let custom_schema = ObjectSchema {
    schema_type: Some(SchemaTypeSet::Single(SchemaType::Object)),
    properties: BTreeMap::from([(
      "field".to_string(),
      ObjectOrReference::Object(ObjectSchema {
        schema_type: Some(SchemaTypeSet::Single(SchemaType::String)),
        ..Default::default()
      }),
    )]),
    ..Default::default()
  };

  let graph = create_test_graph(BTreeMap::from([("CustomType".to_string(), custom_schema)]));
  let resolver = TypeResolver::new(&graph, default_config());

  let schema = ObjectSchema {
    schema_type: Some(SchemaTypeSet::Single(SchemaType::Array)),
    items: Some(Box::new(oas3::spec::Schema::Object(Box::new(ObjectOrReference::Ref {
      ref_path: "#/components/schemas/CustomType".to_string(),
      summary: None,
      description: None,
    })))),
    ..Default::default()
  };

  let result = resolver.schema_to_type_ref(&schema).unwrap();
  assert_eq!(result.to_rust_type(), "Vec<CustomType>");
}

fn create_empty_test_graph() -> SchemaGraph {
  let spec = oas3::Spec {
    openapi: "3.0.0".to_string(),
    info: oas3::spec::Info {
      title: "Test".to_string(),
      summary: None,
      version: "1.0.0".to_string(),
      description: None,
      terms_of_service: None,
      contact: None,
      license: None,
      extensions: BTreeMap::new(),
    },
    servers: Vec::new(),
    paths: None,
    webhooks: BTreeMap::new(),
    components: None,
    security: Vec::new(),
    tags: Vec::new(),
    external_docs: None,
    extensions: BTreeMap::new(),
  };
  let (graph, _) = SchemaGraph::new(spec);
  graph
}

#[test]
fn test_schema_ext_is_primitive() {
  let mut s = ObjectSchema {
    schema_type: Some(SchemaTypeSet::Single(SchemaType::String)),
    ..ObjectSchema::default()
  };
  assert!(s.is_primitive());

  s.properties
    .insert("foo".to_string(), ObjectOrReference::Object(ObjectSchema::default()));
  assert!(!s.is_primitive());
}

#[test]
fn test_resolve_simple_primitive() {
  let graph = create_empty_test_graph();
  let resolver = TypeResolver::new(&graph, default_config());

  let schema = ObjectSchema {
    schema_type: Some(SchemaTypeSet::Single(SchemaType::Integer)),
    format: Some("int32".to_string()),
    ..ObjectSchema::default()
  };

  let result = resolver.schema_to_type_ref(&schema).unwrap();
  assert_eq!(result.to_rust_type().clone(), "i32");
}

#[test]
fn test_resolve_nullable_primitive() {
  let graph = create_empty_test_graph();
  let resolver = TypeResolver::new(&graph, default_config());

  let schema = ObjectSchema {
    schema_type: Some(SchemaTypeSet::Multiple(vec![SchemaType::String, SchemaType::Null])),
    ..ObjectSchema::default()
  };

  let result = resolver.schema_to_type_ref(&schema).unwrap();
  assert_eq!(result.to_rust_type().clone(), "Option<String>");
}

#[test]
fn test_resolve_array() {
  let graph = create_empty_test_graph();
  let resolver = TypeResolver::new(&graph, default_config());

  let item_schema = ObjectSchema {
    schema_type: Some(SchemaTypeSet::Single(SchemaType::String)),
    ..ObjectSchema::default()
  };

  let schema = ObjectSchema {
    schema_type: Some(SchemaTypeSet::Single(SchemaType::Array)),
    items: Some(Box::new(oas3::spec::Schema::Object(Box::new(
      oas3::spec::ObjectOrReference::Object(item_schema),
    )))),
    ..ObjectSchema::default()
  };

  let result = resolver.schema_to_type_ref(&schema).unwrap();
  assert_eq!(result.to_rust_type().clone(), "Vec<String>");
}

#[test]
fn test_is_nullable_object() {
  let mut s = ObjectSchema {
    schema_type: Some(SchemaTypeSet::Single(SchemaType::Null)),
    ..ObjectSchema::default()
  };
  assert!(s.is_nullable_object());

  s.schema_type = Some(SchemaTypeSet::Multiple(vec![SchemaType::Object, SchemaType::Null]));
  assert!(s.is_nullable_object());

  s.properties
    .insert("x".into(), ObjectOrReference::Object(ObjectSchema::default()));
  assert!(!s.is_nullable_object());
}

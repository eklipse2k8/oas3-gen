use std::collections::BTreeMap;

use oas3::spec::{ObjectOrReference, ObjectSchema, SchemaType, SchemaTypeSet};
use serde_json::json;

use super::common::create_test_graph;
use crate::generator::converter::type_resolver::TypeResolver;

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
  let resolver = TypeResolver::new(&graph);

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
  let resolver = TypeResolver::new(&graph);

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
  let resolver = TypeResolver::new(&graph);

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
  let resolver = TypeResolver::new(&graph);

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

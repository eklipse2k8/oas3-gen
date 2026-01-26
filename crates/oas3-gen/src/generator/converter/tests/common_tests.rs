use std::collections::BTreeMap;

use oas3::spec::{BooleanSchema, Discriminator, ObjectOrReference, ObjectSchema, Schema, SchemaType, SchemaTypeSet};

use crate::{
  generator::{converter::inline_resolver::InlineTypeResolver, schema_registry::SchemaRegistry},
  tests::common::{create_test_context, create_test_graph, create_test_spec, default_config},
};

#[test]
fn test_inline_resolver_uses_cached_enum() -> anyhow::Result<()> {
  let graph = create_test_graph(BTreeMap::default());
  let context = create_test_context(graph, default_config());

  let enum_values = vec!["A".to_string(), "B".to_string()];
  context
    .cache
    .borrow_mut()
    .register_enum(enum_values.clone(), "CachedEnum".to_string());
  context.cache.borrow_mut().mark_name_used("CachedEnum".to_string());

  let inline_resolver = InlineTypeResolver::new(context);

  let schema = ObjectSchema {
    enum_values: vec![serde_json::json!("A"), serde_json::json!("B")],
    ..Default::default()
  };

  let result = inline_resolver.resolve_inline_enum("Parent", "Status", &schema, &enum_values)?;

  assert_eq!(result.result.to_rust_type(), "CachedEnum");
  assert!(
    result.inline_types.is_empty(),
    "No new types should be generated for cached enum"
  );
  Ok(())
}

#[test]
fn test_inline_resolver_generates_unique_names() -> anyhow::Result<()> {
  let graph = create_test_graph(BTreeMap::default());
  let context = create_test_context(graph, default_config());

  context.cache.borrow_mut().mark_name_used("ParentItem".to_string());

  let inline_resolver = InlineTypeResolver::new(context);

  let schema = ObjectSchema {
    schema_type: Some(SchemaTypeSet::Single(SchemaType::Object)),
    properties: BTreeMap::from([(
      "id".to_string(),
      ObjectOrReference::Object(ObjectSchema {
        schema_type: Some(SchemaTypeSet::Single(SchemaType::String)),
        ..Default::default()
      }),
    )]),
    ..Default::default()
  };

  let result = inline_resolver.resolve_inline_struct("Parent", "Item", &schema)?;

  assert_eq!(result.result.to_rust_type(), "ParentItem2");
  Ok(())
}

#[test]
fn test_inline_schema_merger_combines_all_sources() -> anyhow::Result<()> {
  let base_schema = ObjectSchema {
    schema_type: Some(SchemaTypeSet::Single(SchemaType::Object)),
    properties: BTreeMap::from([(
      "id".to_string(),
      ObjectOrReference::Object(ObjectSchema {
        schema_type: Some(SchemaTypeSet::Single(SchemaType::String)),
        ..Default::default()
      }),
    )]),
    required: vec!["id".to_string()],
    discriminator: Some(Discriminator {
      property_name: "kind".to_string(),
      mapping: None,
    }),
    additional_properties: Some(Schema::Boolean(BooleanSchema(true))),
    ..Default::default()
  };

  let extra_schema = ObjectSchema {
    properties: BTreeMap::from([(
      "flag".to_string(),
      ObjectOrReference::Object(ObjectSchema {
        schema_type: Some(SchemaTypeSet::Single(SchemaType::Boolean)),
        ..Default::default()
      }),
    )]),
    required: vec!["flag".to_string()],
    ..Default::default()
  };

  let spec = create_test_spec(BTreeMap::from([
    ("Base".to_string(), base_schema.clone()),
    ("Extra".to_string(), extra_schema.clone()),
  ]));

  let registry = SchemaRegistry::from_spec(spec).registry;

  let inline_allof = ObjectSchema {
    properties: BTreeMap::from([(
      "child".to_string(),
      ObjectOrReference::Object(ObjectSchema {
        schema_type: Some(SchemaTypeSet::Single(SchemaType::Integer)),
        ..Default::default()
      }),
    )]),
    required: vec!["child".to_string()],
    ..Default::default()
  };

  let target_schema = ObjectSchema {
    all_of: vec![
      ObjectOrReference::Ref {
        ref_path: "#/components/schemas/Base".to_string(),
        summary: None,
        description: None,
      },
      ObjectOrReference::Ref {
        ref_path: "#/components/schemas/Extra".to_string(),
        summary: None,
        description: None,
      },
      ObjectOrReference::Object(inline_allof),
    ],
    properties: BTreeMap::from([(
      "own".to_string(),
      ObjectOrReference::Object(ObjectSchema {
        schema_type: Some(SchemaTypeSet::Single(SchemaType::Number)),
        ..Default::default()
      }),
    )]),
    ..Default::default()
  };

  let merged = registry.merge_inline(&target_schema)?;

  assert!(merged.all_of.is_empty(), "allOf entries should be flattened");
  assert_eq!(
    merged.schema_type,
    Some(SchemaTypeSet::Single(SchemaType::Object)),
    "schema type should be preserved",
  );
  assert!(merged.properties.contains_key("id"));
  assert!(merged.properties.contains_key("flag"));
  assert!(merged.properties.contains_key("child"));
  assert!(merged.properties.contains_key("own"));
  assert!(merged.required.contains(&"id".to_string()));
  assert!(merged.required.contains(&"flag".to_string()));
  assert!(merged.required.contains(&"child".to_string()));
  assert!(
    merged.discriminator.as_ref().is_some(),
    "discriminator should be retained"
  );
  assert!(
    merged.additional_properties.is_some(),
    "additionalProperties should be merged"
  );
  Ok(())
}

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
    .register_enum(enum_values.clone(), "HowlEnum".to_string());
  context.cache.borrow_mut().mark_name_used("HowlEnum".to_string());

  let inline_resolver = InlineTypeResolver::new(context);

  let schema = ObjectSchema {
    enum_values: vec![serde_json::json!("A"), serde_json::json!("B")],
    ..Default::default()
  };

  let result = inline_resolver.resolve_inline_enum("Loaf", "Sploot", &schema, &enum_values)?;

  assert_eq!(result.result.to_rust_type(), "HowlEnum");
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

  context.cache.borrow_mut().mark_name_used("LoafNugget".to_string());

  let inline_resolver = InlineTypeResolver::new(context);

  let schema = ObjectSchema {
    schema_type: Some(SchemaTypeSet::Single(SchemaType::Object)),
    properties: BTreeMap::from([(
      "tag_id".to_string(),
      ObjectOrReference::Object(ObjectSchema {
        schema_type: Some(SchemaTypeSet::Single(SchemaType::String)),
        ..Default::default()
      }),
    )]),
    ..Default::default()
  };

  let result = inline_resolver.resolve_inline_struct("Loaf", "Nugget", &schema)?;

  assert_eq!(result.result.to_rust_type(), "LoafNugget2");
  Ok(())
}

#[test]
fn test_inline_schema_merger_combines_all_sources() -> anyhow::Result<()> {
  let corgi_schema = ObjectSchema {
    schema_type: Some(SchemaTypeSet::Single(SchemaType::Object)),
    properties: BTreeMap::from([(
      "tag_id".to_string(),
      ObjectOrReference::Object(ObjectSchema {
        schema_type: Some(SchemaTypeSet::Single(SchemaType::String)),
        ..Default::default()
      }),
    )]),
    required: vec!["tag_id".to_string()],
    discriminator: Some(Discriminator {
      property_name: "kind".to_string(),
      mapping: None,
    }),
    additional_properties: Some(Schema::Boolean(BooleanSchema(true))),
    ..Default::default()
  };

  let fluff_schema = ObjectSchema {
    properties: BTreeMap::from([(
      "waddle".to_string(),
      ObjectOrReference::Object(ObjectSchema {
        schema_type: Some(SchemaTypeSet::Single(SchemaType::Boolean)),
        ..Default::default()
      }),
    )]),
    required: vec!["waddle".to_string()],
    ..Default::default()
  };

  let spec = create_test_spec(BTreeMap::from([
    ("Corgi".to_string(), corgi_schema.clone()),
    ("Fluff".to_string(), fluff_schema.clone()),
  ]));

  let registry = SchemaRegistry::from_spec(spec).registry;

  let inline_allof = ObjectSchema {
    properties: BTreeMap::from([(
      "sploot".to_string(),
      ObjectOrReference::Object(ObjectSchema {
        schema_type: Some(SchemaTypeSet::Single(SchemaType::Integer)),
        ..Default::default()
      }),
    )]),
    required: vec!["sploot".to_string()],
    ..Default::default()
  };

  let target_schema = ObjectSchema {
    all_of: vec![
      ObjectOrReference::Ref {
        ref_path: "#/components/schemas/Corgi".to_string(),
        summary: None,
        description: None,
      },
      ObjectOrReference::Ref {
        ref_path: "#/components/schemas/Fluff".to_string(),
        summary: None,
        description: None,
      },
      ObjectOrReference::Object(inline_allof),
    ],
    properties: BTreeMap::from([(
      "bark".to_string(),
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
  assert!(merged.properties.contains_key("tag_id"));
  assert!(merged.properties.contains_key("waddle"));
  assert!(merged.properties.contains_key("sploot"));
  assert!(merged.properties.contains_key("bark"));
  assert!(merged.required.contains(&"tag_id".to_string()));
  assert!(merged.required.contains(&"waddle".to_string()));
  assert!(merged.required.contains(&"sploot".to_string()));
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

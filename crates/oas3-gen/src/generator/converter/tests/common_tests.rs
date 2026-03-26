use std::collections::BTreeMap;

use oas3::spec::{SchemaType, SchemaTypeSet};
use serde_json::json;

use crate::{
  generator::{
    converter::inline_resolver::InlineTypeResolver, metrics::GenerationStats, schema_registry::SchemaRegistry,
  },
  tests::common::{create_test_context, create_test_graph, create_test_spec, default_config, parse_schema},
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

  let schema = parse_schema(json!({
    "enum": ["A", "B"]
  }));

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

  let schema = parse_schema(json!({
    "type": "object",
    "properties": {
      "tag_id": { "type": "string" }
    }
  }));

  let result = inline_resolver.resolve_inline_struct("Loaf", "Nugget", &schema)?;

  assert_eq!(result.result.to_rust_type(), "LoafNugget2");
  Ok(())
}

#[test]
fn test_inline_schema_merger_combines_all_sources() -> anyhow::Result<()> {
  let corgi_schema = parse_schema(json!({
    "type": "object",
    "properties": {
      "tag_id": { "type": "string" }
    },
    "required": ["tag_id"],
    "discriminator": {
      "propertyName": "kind"
    },
    "additionalProperties": true
  }));

  let fluff_schema = parse_schema(json!({
    "properties": {
      "waddle": { "type": "boolean" }
    },
    "required": ["waddle"]
  }));

  let spec = create_test_spec(BTreeMap::from([
    ("Corgi".to_string(), corgi_schema.clone()),
    ("Fluff".to_string(), fluff_schema.clone()),
  ]));

  let mut stats = GenerationStats::default();
  let registry = SchemaRegistry::new(&spec, &mut stats);

  let target_schema = parse_schema(json!({
    "allOf": [
      { "$ref": "#/components/schemas/Corgi" },
      { "$ref": "#/components/schemas/Fluff" },
      {
        "type": "object",
        "properties": {
          "sploot": { "type": "integer" }
        },
        "required": ["sploot"]
      }
    ],
    "properties": {
      "bark": { "type": "number" }
    }
  }));

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

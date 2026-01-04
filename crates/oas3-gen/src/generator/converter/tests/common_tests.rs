use std::{cell::Cell, collections::BTreeMap};

use oas3::spec::{BooleanSchema, Discriminator, ObjectOrReference, ObjectSchema, Schema, SchemaType, SchemaTypeSet};

use crate::{
  generator::{
    ast::{RustType, TypeAliasDef, TypeAliasToken, TypeRef},
    converter::common::{ConversionOutput, handle_inline_creation},
    schema_registry::SchemaRegistry,
  },
  tests::common::{create_test_context, create_test_graph, create_test_spec, default_config},
};

#[test]
fn test_handle_inline_creation_uses_cached_name_check() -> anyhow::Result<()> {
  let schema = ObjectSchema::default();
  let graph = create_test_graph(BTreeMap::default());
  let context = create_test_context(graph, default_config());
  let generator_called = Cell::new(false);

  let result = handle_inline_creation(
    &schema,
    "Ignored",
    None,
    &context,
    |_| Some("CachedType".to_string()),
    |_| {
      generator_called.set(true);
      Ok(ConversionOutput::new(RustType::TypeAlias(TypeAliasDef {
        name: TypeAliasToken::from_raw("Ignored"),
        target: TypeRef::new("i32"),
        ..Default::default()
      })))
    },
  )?;

  assert_eq!(result.result.to_rust_type(), "CachedType");
  assert!(
    !generator_called.get(),
    "Generator should not run when cached name exists"
  );
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

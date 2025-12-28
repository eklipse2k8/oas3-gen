use std::{cell::Cell, collections::BTreeMap};

use oas3::spec::{BooleanSchema, Discriminator, ObjectOrReference, ObjectSchema, Schema, SchemaType, SchemaTypeSet};

use crate::{
  generator::{
    ast::{RustType, TypeAliasDef, TypeAliasToken, TypeRef},
    converter::{
      cache::SharedSchemaCache,
      common::{ConversionOutput, handle_inline_creation},
      inline_scanner::InlineSchemaMerger,
    },
    schema_registry::MergedSchema,
  },
  tests::common::create_test_spec,
};

#[test]
fn test_handle_inline_creation_uses_cached_name_check() -> anyhow::Result<()> {
  let schema = ObjectSchema::default();
  let mut cache = SharedSchemaCache::new();
  let generator_called = Cell::new(false);

  let result = handle_inline_creation(
    &schema,
    "Ignored",
    None,
    Some(&mut cache),
    |_| Some("CachedType".to_string()),
    |_, _| {
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

  let merged_schemas = BTreeMap::from([(
    "Base".to_string(),
    MergedSchema {
      schema: base_schema.clone(),
      discriminator_parent: None,
    },
  )]);

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

  let merged = InlineSchemaMerger::new(&spec, &merged_schemas);
  let merged = merged.merge_inline(&target_schema)?;

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

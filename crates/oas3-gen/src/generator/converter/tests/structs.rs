use std::collections::BTreeMap;

use oas3::spec::{BooleanSchema, Discriminator, ObjectOrReference, ObjectSchema, Schema, SchemaType, SchemaTypeSet};

use super::common::create_test_graph;
use crate::generator::{
  ast::RustType,
  converter::{ConversionResult, FieldOptionalityPolicy, SchemaConverter},
};

#[test]
fn test_discriminated_base_struct_renamed() -> ConversionResult<()> {
  let mut entity_schema = ObjectSchema {
    schema_type: Some(SchemaTypeSet::Single(SchemaType::Object)),
    additional_properties: Some(Schema::Boolean(BooleanSchema(false))),
    ..Default::default()
  };
  entity_schema.properties.insert(
    "id".to_string(),
    ObjectOrReference::Object(ObjectSchema {
      schema_type: Some(SchemaTypeSet::Single(SchemaType::String)),
      ..Default::default()
    }),
  );
  entity_schema.properties.insert(
    "@odata.type".to_string(),
    ObjectOrReference::Object(ObjectSchema {
      schema_type: Some(SchemaTypeSet::Single(SchemaType::String)),
      ..Default::default()
    }),
  );
  entity_schema.discriminator = Some(Discriminator {
    property_name: "@odata.type".to_string(),
    mapping: Some(BTreeMap::from([(
      "#microsoft.graph.user".to_string(),
      "#/components/schemas/User".to_string(),
    )])),
  });

  let graph = create_test_graph(BTreeMap::from([("Entity".to_string(), entity_schema)]));
  let converter = SchemaConverter::new(&graph, FieldOptionalityPolicy::standard(), false, false);
  let result = converter.convert_schema("Entity", graph.get_schema("Entity").unwrap(), None)?;

  let struct_def = result
    .iter()
    .find_map(|ty| match ty {
      RustType::Struct(def) => Some(def),
      _ => None,
    })
    .expect("Backing struct should be present");

  assert_eq!(struct_def.name, "EntityBase");
  assert!(struct_def.serde_attrs.iter().any(|a| a == "deny_unknown_fields"));
  Ok(())
}

#[test]
fn test_discriminator_with_enum_remains_visible() -> ConversionResult<()> {
  let mut message_schema = ObjectSchema {
    schema_type: Some(SchemaTypeSet::Single(SchemaType::Object)),
    additional_properties: Some(Schema::Boolean(BooleanSchema(false))),
    ..Default::default()
  };
  message_schema.properties.insert(
    "role".to_string(),
    ObjectOrReference::Object(ObjectSchema {
      schema_type: Some(SchemaTypeSet::Single(SchemaType::String)),
      enum_values: vec![
        serde_json::Value::String("user".to_string()),
        serde_json::Value::String("assistant".to_string()),
      ],
      ..Default::default()
    }),
  );
  message_schema.properties.insert(
    "content".to_string(),
    ObjectOrReference::Object(ObjectSchema {
      schema_type: Some(SchemaTypeSet::Single(SchemaType::String)),
      ..Default::default()
    }),
  );
  message_schema.required = vec!["role".to_string(), "content".to_string()];
  message_schema.discriminator = Some(Discriminator {
    property_name: "role".to_string(),
    mapping: None,
  });

  let graph = create_test_graph(BTreeMap::from([("Message".to_string(), message_schema)]));
  let converter = SchemaConverter::new(&graph, FieldOptionalityPolicy::standard(), false, false);
  let result = converter.convert_schema("Message", graph.get_schema("Message").unwrap(), None)?;

  let struct_def = result
    .iter()
    .find_map(|ty| match ty {
      RustType::Struct(def) => Some(def),
      _ => None,
    })
    .expect("Struct should be present");

  let role_field = struct_def
    .fields
    .iter()
    .find(|f| f.name == "role")
    .expect("role field should exist");

  assert!(
    !role_field.extra_attrs.iter().any(|a| a.contains("doc(hidden)")),
    "role field should not be hidden"
  );
  assert!(
    !role_field.serde_attrs.iter().any(|a| a.contains("skip")),
    "role field should not be skipped"
  );
  assert!(
    !role_field.rust_type.to_rust_type().starts_with("Option<"),
    "role field should be required"
  );

  Ok(())
}

#[test]
fn test_discriminator_without_enum_is_hidden() -> ConversionResult<()> {
  let mut entity_schema = ObjectSchema {
    schema_type: Some(SchemaTypeSet::Single(SchemaType::Object)),
    ..Default::default()
  };
  entity_schema.properties.insert(
    "@odata.type".to_string(),
    ObjectOrReference::Object(ObjectSchema {
      schema_type: Some(SchemaTypeSet::Single(SchemaType::String)),
      ..Default::default()
    }),
  );
  entity_schema.properties.insert(
    "id".to_string(),
    ObjectOrReference::Object(ObjectSchema {
      schema_type: Some(SchemaTypeSet::Single(SchemaType::String)),
      ..Default::default()
    }),
  );
  entity_schema.required = vec!["@odata.type".to_string()];
  entity_schema.discriminator = Some(Discriminator {
    property_name: "@odata.type".to_string(),
    mapping: Some(BTreeMap::from([(
      "#microsoft.graph.user".to_string(),
      "#/components/schemas/User".to_string(),
    )])),
  });

  let graph = create_test_graph(BTreeMap::from([("Entity".to_string(), entity_schema)]));
  let converter = SchemaConverter::new(&graph, FieldOptionalityPolicy::standard(), false, false);
  let result = converter.convert_schema("Entity", graph.get_schema("Entity").unwrap(), None)?;

  let struct_def = result
    .iter()
    .find_map(|ty| match ty {
      RustType::Struct(def) if def.name == "EntityBase" => Some(def),
      _ => None,
    })
    .expect("EntityBase struct should be present");

  let odata_field = struct_def
    .fields
    .iter()
    .find(|f| f.name == "odata_type")
    .expect("odata_type field should exist");

  assert!(
    odata_field.extra_attrs.iter().any(|a| a.contains("doc(hidden)")),
    "odata_type field should be hidden"
  );
  assert!(
    odata_field.serde_attrs.iter().any(|a| a == "skip"),
    "odata_type field should be skipped"
  );

  Ok(())
}

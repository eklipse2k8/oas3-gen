use std::collections::BTreeMap;

use oas3::spec::{ObjectOrReference, ObjectSchema, SchemaType, SchemaTypeSet};

use crate::{
  generator::{ast::RustType, converter::SchemaConverter},
  tests::common::{create_test_graph, default_config},
};

#[test]
fn test_inline_object_generation() -> anyhow::Result<()> {
  let mut parent_schema = ObjectSchema {
    schema_type: Some(SchemaTypeSet::Single(SchemaType::Object)),
    ..Default::default()
  };

  // Define an inline object for the "config" field
  let mut config_schema = ObjectSchema {
    schema_type: Some(SchemaTypeSet::Single(SchemaType::Object)),
    ..Default::default()
  };
  config_schema.properties.insert(
    "timeout".to_string(),
    ObjectOrReference::Object(ObjectSchema {
      schema_type: Some(SchemaTypeSet::Single(SchemaType::Integer)),
      ..Default::default()
    }),
  );
  config_schema.properties.insert(
    "enabled".to_string(),
    ObjectOrReference::Object(ObjectSchema {
      schema_type: Some(SchemaTypeSet::Single(SchemaType::Boolean)),
      ..Default::default()
    }),
  );

  parent_schema
    .properties
    .insert("config".to_string(), ObjectOrReference::Object(config_schema));

  let graph = create_test_graph(BTreeMap::from([("Parent".to_string(), parent_schema)]));
  let converter = SchemaConverter::new(&graph, &default_config());
  let result = converter.convert_schema("Parent", graph.get_schema("Parent").unwrap(), None)?;

  // Check for Parent struct
  let parent_struct = result
    .iter()
    .find_map(|ty| match ty {
      RustType::Struct(def) if def.name == "Parent" => Some(def),
      _ => None,
    })
    .expect("Parent struct should be present");

  // Check "config" field type
  let config_field = parent_struct
    .fields
    .iter()
    .find(|f| f.name == "config")
    .expect("config field should exist");

  assert_eq!(
    config_field.rust_type.to_rust_type(),
    "Option<ParentConfig>",
    "Config field should reference generated inline struct"
  );

  // Check for ParentConfig struct
  let config_struct = result
    .iter()
    .find_map(|ty| match ty {
      RustType::Struct(def) if def.name == "ParentConfig" => Some(def),
      _ => None,
    })
    .expect("ParentConfig struct should be present");

  assert!(config_struct.fields.iter().any(|f| f.name == "timeout"));
  assert!(config_struct.fields.iter().any(|f| f.name == "enabled"));

  Ok(())
}

#[test]
fn test_inline_object_without_type_field() -> anyhow::Result<()> {
  // Some specs omit "type": "object" but implied by properties
  let mut parent_schema = ObjectSchema {
    schema_type: Some(SchemaTypeSet::Single(SchemaType::Object)),
    ..Default::default()
  };

  // Define an inline object for the "metadata" field WITHOUT type: object
  let mut meta_schema = ObjectSchema {
    schema_type: None, // Intentionally missing
    ..Default::default()
  };
  meta_schema.properties.insert(
    "key".to_string(),
    ObjectOrReference::Object(ObjectSchema {
      schema_type: Some(SchemaTypeSet::Single(SchemaType::String)),
      ..Default::default()
    }),
  );

  parent_schema
    .properties
    .insert("metadata".to_string(), ObjectOrReference::Object(meta_schema));

  let graph = create_test_graph(BTreeMap::from([("Resource".to_string(), parent_schema)]));
  let converter = SchemaConverter::new(&graph, &default_config());
  let result = converter.convert_schema("Resource", graph.get_schema("Resource").unwrap(), None)?;

  // Check for Resource struct
  let resource_struct = result
    .iter()
    .find_map(|ty| match ty {
      RustType::Struct(def) if def.name == "Resource" => Some(def),
      _ => None,
    })
    .expect("Resource struct should be present");

  // Check "metadata" field type
  let meta_field = resource_struct
    .fields
    .iter()
    .find(|f| f.name == "metadata")
    .expect("metadata field should exist");

  assert_eq!(
    meta_field.rust_type.to_rust_type(),
    "Option<ResourceMetadata>",
    "Metadata field should reference generated inline struct even if type is missing"
  );

  // Check for ResourceMetadata struct
  let meta_struct = result
    .iter()
    .find_map(|ty| match ty {
      RustType::Struct(def) if def.name == "ResourceMetadata" => Some(def),
      _ => None,
    })
    .expect("ResourceMetadata struct should be present");

  assert!(meta_struct.fields.iter().any(|f| f.name == "key"));

  Ok(())
}

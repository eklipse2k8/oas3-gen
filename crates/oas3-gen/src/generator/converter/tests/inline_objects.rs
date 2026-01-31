use std::collections::BTreeMap;

use oas3::spec::{ObjectOrReference, ObjectSchema, SchemaType, SchemaTypeSet};

use crate::{
  generator::{ast::RustType, converter::SchemaConverter},
  tests::common::{create_test_context, create_test_graph, default_config},
};

#[test]
fn test_inline_object_generation() -> anyhow::Result<()> {
  let mut loaf_schema = ObjectSchema {
    schema_type: Some(SchemaTypeSet::Single(SchemaType::Object)),
    ..Default::default()
  };

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

  loaf_schema
    .properties
    .insert("loaf_config".to_string(), ObjectOrReference::Object(config_schema));

  let graph = create_test_graph(BTreeMap::from([("Loaf".to_string(), loaf_schema)]));
  let context = create_test_context(graph.clone(), default_config());
  let converter = SchemaConverter::new(&context);
  let result = converter.convert_schema("Loaf", graph.get("Loaf").unwrap())?;

  let binding = context.cache.borrow();
  let generated = &binding.types.types;
  let all_types: Vec<&RustType> = result.iter().chain(generated.iter()).collect();

  let loaf_struct = all_types
    .iter()
    .find_map(|ty| match ty {
      RustType::Struct(def) if def.name == "Loaf" => Some(def),
      _ => None,
    })
    .expect("Loaf struct should be present");

  let config_field = loaf_struct
    .fields
    .iter()
    .find(|f| f.name == "loaf_config")
    .expect("loaf_config field should exist");

  assert_eq!(
    config_field.rust_type.to_rust_type(),
    "Option<LoafConfig>",
    "loaf_config field should reference generated inline struct"
  );

  let config_struct = all_types
    .iter()
    .find_map(|ty| match ty {
      RustType::Struct(def) if def.name == "LoafConfig" => Some(def),
      _ => None,
    })
    .expect("LoafConfig struct should be present");

  assert!(config_struct.fields.iter().any(|f| f.name == "timeout"));
  assert!(config_struct.fields.iter().any(|f| f.name == "enabled"));

  Ok(())
}

#[test]
fn test_inline_object_without_type_field() -> anyhow::Result<()> {
  let mut cardigan_schema = ObjectSchema {
    schema_type: Some(SchemaTypeSet::Single(SchemaType::Object)),
    ..Default::default()
  };

  let mut meta_schema = ObjectSchema {
    schema_type: None,
    ..Default::default()
  };
  meta_schema.properties.insert(
    "key".to_string(),
    ObjectOrReference::Object(ObjectSchema {
      schema_type: Some(SchemaTypeSet::Single(SchemaType::String)),
      ..Default::default()
    }),
  );

  cardigan_schema
    .properties
    .insert("fluff".to_string(), ObjectOrReference::Object(meta_schema));

  let graph = create_test_graph(BTreeMap::from([("Cardigan".to_string(), cardigan_schema)]));
  let context = create_test_context(graph.clone(), default_config());
  let converter = SchemaConverter::new(&context);
  let result = converter.convert_schema("Cardigan", graph.get("Cardigan").unwrap())?;

  let binding = context.cache.borrow();
  let generated = &binding.types.types;
  let all_types: Vec<&RustType> = result.iter().chain(generated.iter()).collect();

  let resource_struct = all_types
    .iter()
    .find_map(|ty| match ty {
      RustType::Struct(def) if def.name == "Cardigan" => Some(def),
      _ => None,
    })
    .expect("Cardigan struct should be present");

  let meta_field = resource_struct
    .fields
    .iter()
    .find(|f| f.name == "fluff")
    .expect("fluff field should exist");

  assert_eq!(
    meta_field.rust_type.to_rust_type(),
    "Option<CardiganFluff>",
    "fluff field should reference generated inline struct even if type is missing"
  );

  let meta_struct = all_types
    .iter()
    .find_map(|ty| match ty {
      RustType::Struct(def) if def.name == "CardiganFluff" => Some(def),
      _ => None,
    })
    .expect("CardiganFluff struct should be present");

  assert!(meta_struct.fields.iter().any(|f| f.name == "key"));

  Ok(())
}

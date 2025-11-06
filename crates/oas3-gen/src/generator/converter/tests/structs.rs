use std::collections::BTreeMap;

use oas3::spec::{BooleanSchema, Discriminator, ObjectOrReference, ObjectSchema, Schema, SchemaType, SchemaTypeSet};

use super::common::create_test_graph;
use crate::generator::{
  ast::RustType,
  converter::{SchemaConverter, error::ConversionResult},
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
  let converter = SchemaConverter::new(&graph);
  let result = converter.convert_schema("Entity", graph.get_schema("Entity").unwrap())?;

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

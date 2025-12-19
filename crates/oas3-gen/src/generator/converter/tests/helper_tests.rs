use std::collections::BTreeMap;

use oas3::spec::{ObjectOrReference, ObjectSchema, SchemaType, SchemaTypeSet};
use serde_json::json;

use crate::{
  generator::{
    ast::{EnumMethodKind, RustType},
    converter::SchemaConverter,
  },
  tests::common::{create_test_graph, default_config},
};

#[test]
fn test_enum_helper_with_const_discriminator() -> anyhow::Result<()> {
  // Schema definition for Dog
  // It has a 'type' field which is const "dog", and 'bark' field which is required string.
  let dog_schema = ObjectSchema {
    title: Some("Dog".to_string()),
    properties: BTreeMap::from([
      (
        "type".to_string(),
        ObjectOrReference::Object(ObjectSchema {
          schema_type: Some(SchemaTypeSet::Single(SchemaType::String)),
          const_value: Some(json!("dog")),
          ..Default::default()
        }),
      ),
      (
        "bark".to_string(),
        ObjectOrReference::Object(ObjectSchema {
          schema_type: Some(SchemaTypeSet::Single(SchemaType::String)),
          ..Default::default()
        }),
      ),
    ]),
    required: vec!["type".to_string(), "bark".to_string()],
    ..Default::default()
  };

  let cat_schema = ObjectSchema {
    title: Some("Cat".to_string()),
    properties: BTreeMap::from([
      (
        "type".to_string(),
        ObjectOrReference::Object(ObjectSchema {
          schema_type: Some(SchemaTypeSet::Single(SchemaType::String)),
          const_value: Some(json!("cat")),
          ..Default::default()
        }),
      ),
      (
        "meow".to_string(),
        ObjectOrReference::Object(ObjectSchema {
          schema_type: Some(SchemaTypeSet::Single(SchemaType::String)),
          ..Default::default()
        }),
      ),
    ]),
    required: vec!["type".to_string(), "meow".to_string()],
    ..Default::default()
  };

  // Schema definition for Pet (Union with Discriminator)
  let union_schema = ObjectSchema {
    one_of: vec![
      ObjectOrReference::Ref {
        ref_path: "#/components/schemas/Dog".to_string(),
        summary: None,
        description: None,
      },
      ObjectOrReference::Ref {
        ref_path: "#/components/schemas/Cat".to_string(),
        summary: None,
        description: None,
      },
    ],
    discriminator: Some(oas3::spec::Discriminator {
      property_name: "type".to_string(),
      mapping: Some(BTreeMap::from([
        ("dog".to_string(), "#/components/schemas/Dog".to_string()),
        ("cat".to_string(), "#/components/schemas/Cat".to_string()),
      ])),
    }),
    ..Default::default()
  };

  let graph = create_test_graph(BTreeMap::from([
    ("Pet".to_string(), union_schema),
    ("Dog".to_string(), dog_schema),
    ("Cat".to_string(), cat_schema),
  ]));

  let converter = SchemaConverter::new(&graph, default_config());
  let result = converter.convert_schema("Pet", graph.get_schema("Pet").unwrap(), None)?;

  // Expect DiscriminatedEnum
  let RustType::DiscriminatedEnum(enum_def) = result.last().unwrap() else {
    panic!("Expected DiscriminatedEnum")
  };

  assert_eq!(enum_def.name.to_string(), "Pet");
  // Both variants should have helpers
  assert_eq!(enum_def.methods.len(), 2, "Should have 2 helper methods");

  // Find the dog method
  let method = enum_def
    .methods
    .iter()
    .find(|m| m.name == "dog")
    .expect("dog method not found");

  match &method.kind {
    EnumMethodKind::ParameterizedConstructor {
      param_name, param_type, ..
    } => {
      assert_eq!(param_name, "bark");
      assert_eq!(param_type.to_rust_type(), "String");
    }
    EnumMethodKind::SimpleConstructor { .. } => panic!("Expected ParameterizedConstructor, got {:?}", method.kind),
  }

  Ok(())
}

use std::collections::BTreeMap;

use oas3::spec::{ObjectOrReference, ObjectSchema, SchemaType, SchemaTypeSet};
use serde_json::json;

use crate::{
  generator::{
    ast::{EnumMethodKind, RustType},
    converter::SchemaConverter,
  },
  tests::common::{create_test_context, create_test_graph, default_config},
};

#[test]
fn test_enum_helper_with_const_discriminator() -> anyhow::Result<()> {
  let pembroke_schema = ObjectSchema {
    title: Some("Pembroke".to_string()),
    properties: BTreeMap::from([
      (
        "type".to_string(),
        ObjectOrReference::Object(ObjectSchema {
          schema_type: Some(SchemaTypeSet::Single(SchemaType::String)),
          const_value: Some(json!("pembroke")),
          ..Default::default()
        }),
      ),
      (
        "waddle".to_string(),
        ObjectOrReference::Object(ObjectSchema {
          schema_type: Some(SchemaTypeSet::Single(SchemaType::String)),
          ..Default::default()
        }),
      ),
    ]),
    required: vec!["type".to_string(), "waddle".to_string()],
    ..Default::default()
  };

  let cardigan_schema = ObjectSchema {
    title: Some("Cardigan".to_string()),
    properties: BTreeMap::from([
      (
        "type".to_string(),
        ObjectOrReference::Object(ObjectSchema {
          schema_type: Some(SchemaTypeSet::Single(SchemaType::String)),
          const_value: Some(json!("cardigan")),
          ..Default::default()
        }),
      ),
      (
        "sploot".to_string(),
        ObjectOrReference::Object(ObjectSchema {
          schema_type: Some(SchemaTypeSet::Single(SchemaType::String)),
          ..Default::default()
        }),
      ),
    ]),
    required: vec!["type".to_string(), "sploot".to_string()],
    ..Default::default()
  };

  let union_schema = ObjectSchema {
    one_of: vec![
      ObjectOrReference::Ref {
        ref_path: "#/components/schemas/Pembroke".to_string(),
        summary: None,
        description: None,
      },
      ObjectOrReference::Ref {
        ref_path: "#/components/schemas/Cardigan".to_string(),
        summary: None,
        description: None,
      },
    ],
    discriminator: Some(oas3::spec::Discriminator {
      property_name: "type".to_string(),
      mapping: Some(BTreeMap::from([
        ("pembroke".to_string(), "#/components/schemas/Pembroke".to_string()),
        ("cardigan".to_string(), "#/components/schemas/Cardigan".to_string()),
      ])),
    }),
    ..Default::default()
  };

  let graph = create_test_graph(BTreeMap::from([
    ("Corgi".to_string(), union_schema),
    ("Pembroke".to_string(), pembroke_schema),
    ("Cardigan".to_string(), cardigan_schema),
  ]));

  let context = create_test_context(graph.clone(), default_config());
  let converter = SchemaConverter::new(&context);
  let result = converter.convert_schema("Corgi", graph.get("Corgi").unwrap())?;

  let RustType::DiscriminatedEnum(enum_def) = result.last().unwrap() else {
    panic!("Expected DiscriminatedEnum")
  };

  assert_eq!(enum_def.name.to_string(), "Corgi");
  assert_eq!(enum_def.methods.len(), 2, "Should have 2 helper methods");

  let method = enum_def
    .methods
    .iter()
    .find(|m| m.name == "pembroke")
    .expect("pembroke method not found");

  match &method.kind {
    EnumMethodKind::ParameterizedConstructor {
      param_name, param_type, ..
    } => {
      assert_eq!(param_name, "waddle");
      assert_eq!(param_type.to_rust_type(), "String");
    }
    _ => panic!("Expected ParameterizedConstructor, got {:?}", method.kind),
  }

  Ok(())
}

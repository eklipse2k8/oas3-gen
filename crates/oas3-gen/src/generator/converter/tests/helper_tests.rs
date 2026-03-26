use std::collections::BTreeMap;

use oas3::spec::ObjectSchema;
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
  let pembroke_json = json!({
    "title": "Pembroke",
    "type": "object",
    "properties": {
      "type": {
        "type": "string",
        "const": "pembroke"
      },
      "waddle": {
        "type": "string"
      }
    },
    "required": ["type", "waddle"]
  });

  let cardigan_json = json!({
    "title": "Cardigan",
    "type": "object",
    "properties": {
      "type": {
        "type": "string",
        "const": "cardigan"
      },
      "sploot": {
        "type": "string"
      }
    },
    "required": ["type", "sploot"]
  });

  let corgi_json = json!({
    "oneOf": [
      { "$ref": "#/components/schemas/Pembroke" },
      { "$ref": "#/components/schemas/Cardigan" }
    ],
    "discriminator": {
      "propertyName": "type",
      "mapping": {
        "pembroke": "#/components/schemas/Pembroke",
        "cardigan": "#/components/schemas/Cardigan"
      }
    }
  });

  let pembroke_schema = serde_json::from_value::<ObjectSchema>(pembroke_json)?;
  let cardigan_schema = serde_json::from_value::<ObjectSchema>(cardigan_json)?;
  let corgi_schema = serde_json::from_value::<ObjectSchema>(corgi_json)?;

  let graph = create_test_graph(BTreeMap::from([
    ("Corgi".to_string(), corgi_schema),
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

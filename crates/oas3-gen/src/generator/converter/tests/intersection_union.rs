use std::collections::BTreeMap;

use oas3::spec::ObjectSchema;
use serde_json::json;

use crate::{
  generator::{ast::RustType, converter::SchemaConverter},
  tests::common::{create_test_context, create_test_graph, default_config},
};

#[test]
fn test_intersection_of_union_allof_anyof() -> anyhow::Result<()> {
  let corgi_json = json!({
    "type": "object",
    "description": "Must be a corgi with a tag_id, and must be either a Pembroke or a Cardigan.",
    "allOf": [
      {
        "required": ["tag_id"],
        "properties": {
          "tag_id": {
            "type": "string"
          }
        }
      }
    ],
    "anyOf": [
      {
        "required": ["stumpy_legs"],
        "properties": {
          "stumpy_legs": {
            "type": "integer"
          }
        }
      },
      {
        "required": ["floof_ears"],
        "properties": {
          "floof_ears": {
            "type": "integer"
          }
        }
      }
    ]
  });

  let corgi_schema = serde_json::from_value::<ObjectSchema>(corgi_json)?;
  let graph = create_test_graph(BTreeMap::from([("Corgi".to_string(), corgi_schema)]));
  let context = create_test_context(graph.clone(), default_config());
  let converter = SchemaConverter::new(&context);
  let result = converter.convert_schema("Corgi", graph.get("Corgi").unwrap())?;

  let struct_def = result
    .iter()
    .find_map(|ty| match ty {
      RustType::Struct(def) => Some(def),
      _ => None,
    })
    .expect("Corgi struct should be present");

  assert_eq!(struct_def.name, "Corgi");

  let has_id = struct_def.fields.iter().any(|f| f.name == "tag_id");
  assert!(has_id, "Corgi should have 'tag_id' field from allOf");

  let has_legs = struct_def.fields.iter().any(|f| f.name == "stumpy_legs");
  let has_ears = struct_def.fields.iter().any(|f| f.name == "floof_ears");

  assert!(has_legs);
  assert!(has_ears);

  Ok(())
}

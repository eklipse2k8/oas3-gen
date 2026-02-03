use std::collections::BTreeMap;

use oas3::spec::{ObjectOrReference, ObjectSchema, SchemaType, SchemaTypeSet};

use crate::{
  generator::{ast::RustType, converter::SchemaConverter},
  tests::common::{create_test_context, create_test_graph, default_config},
};

#[test]
fn test_intersection_of_union_allof_anyof() -> anyhow::Result<()> {
  let mut corgi_schema = ObjectSchema {
    schema_type: Some(SchemaTypeSet::Single(SchemaType::Object)),
    description: Some("Must be a corgi with a tag_id, and must be either a Pembroke or a Cardigan.".to_string()),
    ..Default::default()
  };

  corgi_schema.all_of.push(ObjectOrReference::Object(ObjectSchema {
    required: vec!["tag_id".to_string()],
    properties: BTreeMap::from([(
      "tag_id".to_string(),
      ObjectOrReference::Object(ObjectSchema {
        schema_type: Some(SchemaTypeSet::Single(SchemaType::String)),
        ..Default::default()
      }),
    )]),
    ..Default::default()
  }));

  corgi_schema.any_of.push(ObjectOrReference::Object(ObjectSchema {
    required: vec!["stumpy_legs".to_string()],
    properties: BTreeMap::from([(
      "stumpy_legs".to_string(),
      ObjectOrReference::Object(ObjectSchema {
        schema_type: Some(SchemaTypeSet::Single(SchemaType::Integer)),
        ..Default::default()
      }),
    )]),
    ..Default::default()
  }));

  corgi_schema.any_of.push(ObjectOrReference::Object(ObjectSchema {
    required: vec!["floof_ears".to_string()],
    properties: BTreeMap::from([(
      "floof_ears".to_string(),
      ObjectOrReference::Object(ObjectSchema {
        schema_type: Some(SchemaTypeSet::Single(SchemaType::Integer)),
        ..Default::default()
      }),
    )]),
    ..Default::default()
  }));

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

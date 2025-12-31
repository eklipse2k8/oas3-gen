use std::collections::BTreeMap;

use oas3::spec::{ObjectOrReference, ObjectSchema, SchemaType, SchemaTypeSet};

use crate::{
  generator::{ast::RustType, converter::SchemaConverter},
  tests::common::{create_test_graph, default_config},
};

#[test]
fn test_intersection_of_union_allof_anyof() -> anyhow::Result<()> {
  // Vehicle schema from user request
  // description: "Must be a vehicle with an ID, and must be either a Car or a Boat."
  // allOf:
  //   - required: [id]
  //     properties:
  //       id: { type: string }
  // anyOf:
  //   - required: [wheels]
  //     properties:
  //       wheels: { type: integer }
  //   - required: [sails]
  //     properties:
  //       sails: { type: integer }

  let mut vehicle_schema = ObjectSchema {
    schema_type: Some(SchemaTypeSet::Single(SchemaType::Object)),
    description: Some("Must be a vehicle with an ID, and must be either a Car or a Boat.".to_string()),
    ..Default::default()
  };

  // Rule 1: allOf (Intersection)
  vehicle_schema.all_of.push(ObjectOrReference::Object(ObjectSchema {
    required: vec!["id".to_string()],
    properties: BTreeMap::from([(
      "id".to_string(),
      ObjectOrReference::Object(ObjectSchema {
        schema_type: Some(SchemaTypeSet::Single(SchemaType::String)),
        ..Default::default()
      }),
    )]),
    ..Default::default()
  }));

  // Rule 2: anyOf (Union)
  // Note: In typical Rust codegen, mixing allOf (struct properties) and anyOf (union) is challenging.
  // This test checks how the converter handles this.
  vehicle_schema.any_of.push(ObjectOrReference::Object(ObjectSchema {
    required: vec!["wheels".to_string()],
    properties: BTreeMap::from([(
      "wheels".to_string(),
      ObjectOrReference::Object(ObjectSchema {
        schema_type: Some(SchemaTypeSet::Single(SchemaType::Integer)),
        ..Default::default()
      }),
    )]),
    ..Default::default()
  }));

  vehicle_schema.any_of.push(ObjectOrReference::Object(ObjectSchema {
    required: vec!["sails".to_string()],
    properties: BTreeMap::from([(
      "sails".to_string(),
      ObjectOrReference::Object(ObjectSchema {
        schema_type: Some(SchemaTypeSet::Single(SchemaType::Integer)),
        ..Default::default()
      }),
    )]),
    ..Default::default()
  }));

  let graph = create_test_graph(BTreeMap::from([("Vehicle".to_string(), vehicle_schema)]));
  let converter = SchemaConverter::new(&graph, &default_config());
  let result = converter.convert_schema("Vehicle", graph.get("Vehicle").unwrap(), None)?;

  // We expect a struct "Vehicle"
  let struct_def = result
    .iter()
    .find_map(|ty| match ty {
      RustType::Struct(def) => Some(def),
      _ => None,
    })
    .expect("Vehicle struct should be present");

  assert_eq!(struct_def.name, "Vehicle");

  // Verify 'id' field from allOf is present (merged)
  let has_id = struct_def.fields.iter().any(|f| f.name == "id");
  assert!(has_id, "Vehicle should have 'id' field from allOf");

  // Verify behavior regarding anyOf fields ('wheels', 'sails')
  // The generator now merges anyOf properties into the struct when allOf is present.
  let has_wheels = struct_def.fields.iter().any(|f| f.name == "wheels");
  let has_sails = struct_def.fields.iter().any(|f| f.name == "sails");

  assert!(has_wheels, "wheels field should be present (merged from anyOf)");
  assert!(has_sails, "sails field should be present (merged from anyOf)");

  Ok(())
}

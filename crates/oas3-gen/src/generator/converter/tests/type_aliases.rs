use std::collections::BTreeMap;

use oas3::spec::{ObjectOrReference, ObjectSchema, Schema, SchemaType, SchemaTypeSet};

use crate::{
  generator::{
    ast::RustType,
    converter::{FieldOptionalityPolicy, SchemaConverter},
  },
  tests::common::{create_test_graph, default_config},
};

fn make_string_schema() -> ObjectSchema {
  ObjectSchema {
    schema_type: Some(SchemaTypeSet::Single(SchemaType::String)),
    ..Default::default()
  }
}

fn make_array_schema(items: Option<Schema>) -> ObjectSchema {
  ObjectSchema {
    schema_type: Some(SchemaTypeSet::Single(SchemaType::Array)),
    items: items.map(Box::new),
    ..Default::default()
  }
}

#[test]
fn test_primitive_type_aliases() -> anyhow::Result<()> {
  let cases = [
    (
      "Identifier",
      ObjectSchema {
        schema_type: Some(SchemaTypeSet::Single(SchemaType::String)),
        ..Default::default()
      },
      "String",
    ),
    (
      "Timestamp",
      ObjectSchema {
        schema_type: Some(SchemaTypeSet::Single(SchemaType::Integer)),
        format: Some("int64".to_string()),
        ..Default::default()
      },
      "i64",
    ),
  ];

  for (name, schema, expected_type) in cases {
    let graph = create_test_graph(BTreeMap::from([(name.to_string(), schema)]));
    let converter = SchemaConverter::new(&graph, FieldOptionalityPolicy::standard(), default_config());
    let result = converter.convert_schema(name, graph.get_schema(name).unwrap(), None)?;

    assert_eq!(result.len(), 1, "expected single type for {name}");
    let RustType::TypeAlias(alias) = &result[0] else {
      panic!("expected type alias for {name}")
    };
    assert_eq!(alias.name, name, "name mismatch for {name}");
    assert_eq!(alias.target.to_rust_type(), expected_type, "type mismatch for {name}");
  }
  Ok(())
}

#[test]
fn test_array_type_aliases() -> anyhow::Result<()> {
  let strings_schema = make_array_schema(Some(Schema::Object(Box::new(ObjectOrReference::Object(
    make_string_schema(),
  )))));

  let untyped_array_schema = make_array_schema(None);

  let nested_array_schema = make_array_schema(Some(Schema::Object(Box::new(ObjectOrReference::Object(
    ObjectSchema {
      schema_type: Some(SchemaTypeSet::Single(SchemaType::Array)),
      items: Some(Box::new(Schema::Object(Box::new(ObjectOrReference::Object(
        ObjectSchema {
          schema_type: Some(SchemaTypeSet::Single(SchemaType::Integer)),
          ..Default::default()
        },
      ))))),
      ..Default::default()
    },
  )))));

  let cases = [
    ("Strings", strings_schema, "Vec<String>"),
    ("UntypedArray", untyped_array_schema, "Vec<serde_json::Value>"),
    ("Matrix", nested_array_schema, "Vec<Vec<i64>>"),
  ];

  for (name, schema, expected_type) in cases {
    let graph = create_test_graph(BTreeMap::from([(name.to_string(), schema)]));
    let converter = SchemaConverter::new(&graph, FieldOptionalityPolicy::standard(), default_config());
    let result = converter.convert_schema(name, graph.get_schema(name).unwrap(), None)?;

    assert_eq!(result.len(), 1, "expected single type for {name}");
    let RustType::TypeAlias(alias) = &result[0] else {
      panic!("expected type alias for {name}")
    };
    assert_eq!(alias.name, name, "name mismatch for {name}");
    assert_eq!(alias.target.to_rust_type(), expected_type, "type mismatch for {name}");
  }
  Ok(())
}

#[test]
fn test_array_type_alias_with_ref_items() -> anyhow::Result<()> {
  let pet_schema = ObjectSchema {
    schema_type: Some(SchemaTypeSet::Single(SchemaType::Object)),
    properties: BTreeMap::from([
      (
        "id".to_string(),
        ObjectOrReference::Object(ObjectSchema {
          schema_type: Some(SchemaTypeSet::Single(SchemaType::Integer)),
          ..Default::default()
        }),
      ),
      ("name".to_string(), ObjectOrReference::Object(make_string_schema())),
    ]),
    ..Default::default()
  };

  let pets_schema_array = make_array_schema(Some(Schema::Object(Box::new(ObjectOrReference::Ref {
    ref_path: "#/components/schemas/Pet".to_string(),
    summary: None,
    description: None,
  }))));

  let graph = create_test_graph(BTreeMap::from([
    ("Pet".to_string(), pet_schema),
    ("Pets".to_string(), pets_schema_array),
  ]));

  let converter = SchemaConverter::new(&graph, FieldOptionalityPolicy::standard(), default_config());
  let result = converter.convert_schema("Pets", graph.get_schema("Pets").unwrap(), None)?;

  assert_eq!(result.len(), 1);
  let RustType::TypeAlias(alias) = &result[0] else {
    panic!("expected type alias for array schema")
  };

  assert_eq!(alias.name, "Pets");
  assert_eq!(alias.target.to_rust_type(), "Vec<Pet>");
  Ok(())
}

use std::collections::BTreeMap;

use oas3::spec::{ObjectOrReference, ObjectSchema, SchemaType, SchemaTypeSet};

use crate::{
  generator::{
    ast::RustType,
    converter::{ConversionResult, FieldOptionalityPolicy, SchemaConverter},
  },
  tests::common::{create_test_graph, default_config},
};

#[test]
fn test_array_type_alias_with_ref_items() -> ConversionResult<()> {
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
      (
        "name".to_string(),
        ObjectOrReference::Object(ObjectSchema {
          schema_type: Some(SchemaTypeSet::Single(SchemaType::String)),
          ..Default::default()
        }),
      ),
    ]),
    ..Default::default()
  };

  let pets_schema_array = ObjectSchema {
    schema_type: Some(SchemaTypeSet::Single(SchemaType::Array)),
    items: Some(Box::new(oas3::spec::Schema::Object(Box::new(ObjectOrReference::Ref {
      ref_path: "#/components/schemas/Pet".to_string(),
      summary: None,
      description: None,
    })))),
    ..Default::default()
  };

  let graph = create_test_graph(BTreeMap::from([
    ("Pet".to_string(), pet_schema),
    ("Pets".to_string(), pets_schema_array),
  ]));

  let converter = SchemaConverter::new(&graph, FieldOptionalityPolicy::standard(), default_config());
  let result = converter.convert_schema("Pets", graph.get_schema("Pets").unwrap(), None)?;

  assert_eq!(result.len(), 1);
  let RustType::TypeAlias(alias) = &result[0] else {
    panic!("Expected type alias for array schema")
  };

  assert_eq!(alias.name, "Pets");
  assert_eq!(alias.target.to_rust_type(), "Vec<Pet>");
  Ok(())
}

#[test]
fn test_array_type_alias_with_primitive_items() -> ConversionResult<()> {
  let strings_schema = ObjectSchema {
    schema_type: Some(SchemaTypeSet::Single(SchemaType::Array)),
    items: Some(Box::new(oas3::spec::Schema::Object(Box::new(
      ObjectOrReference::Object(ObjectSchema {
        schema_type: Some(SchemaTypeSet::Single(SchemaType::String)),
        ..Default::default()
      }),
    )))),
    ..Default::default()
  };

  let graph = create_test_graph(BTreeMap::from([("Strings".to_string(), strings_schema)]));
  let converter = SchemaConverter::new(&graph, FieldOptionalityPolicy::standard(), default_config());
  let result = converter.convert_schema("Strings", graph.get_schema("Strings").unwrap(), None)?;

  assert_eq!(result.len(), 1);
  let RustType::TypeAlias(alias) = &result[0] else {
    panic!("Expected type alias for array schema")
  };

  assert_eq!(alias.name, "Strings");
  assert_eq!(alias.target.to_rust_type(), "Vec<String>");
  Ok(())
}

#[test]
fn test_primitive_type_alias() -> ConversionResult<()> {
  let identifier_schema = ObjectSchema {
    schema_type: Some(SchemaTypeSet::Single(SchemaType::String)),
    ..Default::default()
  };

  let graph = create_test_graph(BTreeMap::from([("Identifier".to_string(), identifier_schema)]));
  let converter = SchemaConverter::new(&graph, FieldOptionalityPolicy::standard(), default_config());
  let result = converter.convert_schema("Identifier", graph.get_schema("Identifier").unwrap(), None)?;

  assert_eq!(result.len(), 1);
  let RustType::TypeAlias(alias) = &result[0] else {
    panic!("Expected type alias for primitive schema")
  };

  assert_eq!(alias.name, "Identifier");
  assert_eq!(alias.target.to_rust_type(), "String");
  Ok(())
}

#[test]
fn test_integer_type_alias_with_format() -> ConversionResult<()> {
  let timestamp_schema = ObjectSchema {
    schema_type: Some(SchemaTypeSet::Single(SchemaType::Integer)),
    format: Some("int64".to_string()),
    ..Default::default()
  };

  let graph = create_test_graph(BTreeMap::from([("Timestamp".to_string(), timestamp_schema)]));
  let converter = SchemaConverter::new(&graph, FieldOptionalityPolicy::standard(), default_config());
  let result = converter.convert_schema("Timestamp", graph.get_schema("Timestamp").unwrap(), None)?;

  assert_eq!(result.len(), 1);
  let RustType::TypeAlias(alias) = &result[0] else {
    panic!("Expected type alias for integer schema")
  };

  assert_eq!(alias.name, "Timestamp");
  assert_eq!(alias.target.to_rust_type(), "i64");
  Ok(())
}

#[test]
fn test_array_with_no_items_falls_back() -> ConversionResult<()> {
  let untyped_array_schema = ObjectSchema {
    schema_type: Some(SchemaTypeSet::Single(SchemaType::Array)),
    items: None,
    ..Default::default()
  };

  let graph = create_test_graph(BTreeMap::from([("UntypedArray".to_string(), untyped_array_schema)]));
  let converter = SchemaConverter::new(&graph, FieldOptionalityPolicy::standard(), default_config());
  let result = converter.convert_schema("UntypedArray", graph.get_schema("UntypedArray").unwrap(), None)?;

  assert_eq!(result.len(), 1);
  let RustType::TypeAlias(alias) = &result[0] else {
    panic!("Expected type alias for untyped array schema")
  };

  assert_eq!(alias.name, "UntypedArray");
  assert_eq!(alias.target.to_rust_type(), "Vec<serde_json::Value>");
  Ok(())
}

#[test]
fn test_nested_array_type_alias() -> ConversionResult<()> {
  let matrix_schema = ObjectSchema {
    schema_type: Some(SchemaTypeSet::Single(SchemaType::Array)),
    items: Some(Box::new(oas3::spec::Schema::Object(Box::new(
      ObjectOrReference::Object(ObjectSchema {
        schema_type: Some(SchemaTypeSet::Single(SchemaType::Array)),
        items: Some(Box::new(oas3::spec::Schema::Object(Box::new(
          ObjectOrReference::Object(ObjectSchema {
            schema_type: Some(SchemaTypeSet::Single(SchemaType::Integer)),
            ..Default::default()
          }),
        )))),
        ..Default::default()
      }),
    )))),
    ..Default::default()
  };

  let graph = create_test_graph(BTreeMap::from([("Matrix".to_string(), matrix_schema)]));
  let converter = SchemaConverter::new(&graph, FieldOptionalityPolicy::standard(), default_config());
  let result = converter.convert_schema("Matrix", graph.get_schema("Matrix").unwrap(), None)?;

  assert_eq!(result.len(), 1);
  let RustType::TypeAlias(alias) = &result[0] else {
    panic!("Expected type alias for nested array schema")
  };

  assert_eq!(alias.name, "Matrix");
  assert_eq!(alias.target.to_rust_type(), "Vec<Vec<i64>>");
  Ok(())
}

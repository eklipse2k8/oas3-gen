use std::collections::BTreeMap;

use oas3::spec::{Discriminator, ObjectOrReference, ObjectSchema, SchemaType, SchemaTypeSet};
use serde_json::json;

use super::common::create_test_graph;
use crate::generator::{
  ast::RustType,
  converter::{SchemaConverter, error::ConversionResult},
};

#[test]
fn test_simple_string_enum() -> ConversionResult<()> {
  let enum_schema = ObjectSchema {
    schema_type: Some(SchemaTypeSet::Single(SchemaType::String)),
    enum_values: vec![json!("value1"), json!("value2")],
    ..Default::default()
  };
  let graph = create_test_graph(BTreeMap::from([("SimpleEnum".to_string(), enum_schema)]));
  let converter = SchemaConverter::new(&graph);
  let result = converter.convert_schema("SimpleEnum", graph.get_schema("SimpleEnum").unwrap())?;

  assert_eq!(result.len(), 1);
  let RustType::Enum(enum_def) = &result[0] else {
    panic!("Expected enum")
  };

  assert_eq!(enum_def.name, "SimpleEnum");
  assert_eq!(enum_def.variants.len(), 2);
  assert!(enum_def.derives.contains(&"Eq".to_string()));
  assert!(enum_def.derives.contains(&"Hash".to_string()));
  Ok(())
}

#[test]
fn test_oneof_with_discriminator_has_rename_attrs() -> ConversionResult<()> {
  let variant1 = ObjectSchema {
    schema_type: Some(SchemaTypeSet::Single(SchemaType::Object)),
    properties: BTreeMap::from([(
      "type".to_string(),
      ObjectOrReference::Object(ObjectSchema {
        schema_type: Some(SchemaTypeSet::Single(SchemaType::String)),
        const_value: Some(json!("type_a")),
        ..Default::default()
      }),
    )]),
    ..Default::default()
  };

  let variant2 = ObjectSchema {
    schema_type: Some(SchemaTypeSet::Single(SchemaType::Object)),
    properties: BTreeMap::from([(
      "type".to_string(),
      ObjectOrReference::Object(ObjectSchema {
        schema_type: Some(SchemaTypeSet::Single(SchemaType::String)),
        const_value: Some(json!("type_b")),
        ..Default::default()
      }),
    )]),
    ..Default::default()
  };

  let union_schema = ObjectSchema {
    one_of: vec![
      ObjectOrReference::Ref {
        ref_path: "#/components/schemas/VariantA".to_string(),
        summary: None,
        description: None,
      },
      ObjectOrReference::Ref {
        ref_path: "#/components/schemas/VariantB".to_string(),
        summary: None,
        description: None,
      },
    ],
    discriminator: Some(Discriminator {
      property_name: "type".to_string(),
      mapping: Some(BTreeMap::from([
        ("type_a".to_string(), "#/components/schemas/VariantA".to_string()),
        ("type_b".to_string(), "#/components/schemas/VariantB".to_string()),
      ])),
    }),
    ..Default::default()
  };

  let graph = create_test_graph(BTreeMap::from([
    ("TestUnion".to_string(), union_schema),
    ("VariantA".to_string(), variant1),
    ("VariantB".to_string(), variant2),
  ]));
  let converter = SchemaConverter::new(&graph);
  let result = converter.convert_schema("TestUnion", graph.get_schema("TestUnion").unwrap())?;

  let RustType::Enum(enum_def) = result.last().unwrap() else {
    panic!("Expected enum as last type")
  };

  assert_eq!(enum_def.name, "TestUnion");
  assert_eq!(enum_def.variants.len(), 2);
  assert!(
    enum_def.variants[0]
      .serde_attrs
      .contains(&r#"rename = "type_a""#.to_string())
  );
  assert!(
    enum_def.variants[1]
      .serde_attrs
      .contains(&r#"rename = "type_b""#.to_string())
  );
  Ok(())
}

#[test]
fn test_anyof_without_discriminator_has_no_rename_attrs() -> ConversionResult<()> {
  let variant1 = ObjectSchema {
    schema_type: Some(SchemaTypeSet::Single(SchemaType::Object)),
    properties: BTreeMap::from([(
      "field1".to_string(),
      ObjectOrReference::Object(ObjectSchema {
        schema_type: Some(SchemaTypeSet::Single(SchemaType::String)),
        ..Default::default()
      }),
    )]),
    ..Default::default()
  };

  let variant2 = ObjectSchema {
    schema_type: Some(SchemaTypeSet::Single(SchemaType::Object)),
    properties: BTreeMap::from([(
      "field2".to_string(),
      ObjectOrReference::Object(ObjectSchema {
        schema_type: Some(SchemaTypeSet::Single(SchemaType::Integer)),
        ..Default::default()
      }),
    )]),
    ..Default::default()
  };

  let union_schema = ObjectSchema {
    any_of: vec![
      ObjectOrReference::Ref {
        ref_path: "#/components/schemas/VariantA".to_string(),
        summary: None,
        description: None,
      },
      ObjectOrReference::Ref {
        ref_path: "#/components/schemas/VariantB".to_string(),
        summary: None,
        description: None,
      },
    ],
    ..Default::default()
  };

  let graph = create_test_graph(BTreeMap::from([
    ("TestUnion".to_string(), union_schema),
    ("VariantA".to_string(), variant1),
    ("VariantB".to_string(), variant2),
  ]));
  let converter = SchemaConverter::new(&graph);
  let result = converter.convert_schema("TestUnion", graph.get_schema("TestUnion").unwrap())?;

  let RustType::Enum(enum_def) = result.last().unwrap() else {
    panic!("Expected enum as last type")
  };

  assert_eq!(enum_def.name, "TestUnion");
  assert_eq!(enum_def.variants.len(), 2);
  assert!(enum_def.variants[0].serde_attrs.is_empty());
  assert!(enum_def.variants[1].serde_attrs.is_empty());
  assert!(enum_def.serde_attrs.contains(&"untagged".to_string()));
  Ok(())
}

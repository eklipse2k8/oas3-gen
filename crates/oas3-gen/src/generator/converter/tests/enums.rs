use std::{collections::BTreeMap, f64::consts::PI};

use oas3::spec::{Discriminator, ObjectOrReference, ObjectSchema, SchemaType, SchemaTypeSet};
use serde_json::json;

use super::common::create_test_graph;
use crate::generator::{
  ast::RustType,
  converter::{FieldOptionalityPolicy, SchemaConverter, error::ConversionResult},
};

#[test]
fn test_simple_string_enum() -> ConversionResult<()> {
  let enum_schema = ObjectSchema {
    schema_type: Some(SchemaTypeSet::Single(SchemaType::String)),
    enum_values: vec![json!("value1"), json!("value2")],
    ..Default::default()
  };
  let graph = create_test_graph(BTreeMap::from([("SimpleEnum".to_string(), enum_schema)]));
  let converter = SchemaConverter::new(&graph, FieldOptionalityPolicy::standard());
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
  let converter = SchemaConverter::new(&graph, FieldOptionalityPolicy::standard());
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
  let converter = SchemaConverter::new(&graph, FieldOptionalityPolicy::standard());
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

#[test]
fn test_anyof_with_discriminator_no_untagged() -> ConversionResult<()> {
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
  let converter = SchemaConverter::new(&graph, FieldOptionalityPolicy::standard());
  let result = converter.convert_schema("TestUnion", graph.get_schema("TestUnion").unwrap())?;

  let RustType::Enum(enum_def) = result.last().unwrap() else {
    panic!("Expected enum as last type")
  };

  assert_eq!(enum_def.name, "TestUnion");
  assert_eq!(enum_def.discriminator, Some("type".to_string()));
  assert!(!enum_def.serde_attrs.contains(&"untagged".to_string()));
  Ok(())
}

#[test]
fn test_integer_enum_values() -> ConversionResult<()> {
  let enum_schema = ObjectSchema {
    schema_type: Some(SchemaTypeSet::Single(SchemaType::Integer)),
    enum_values: vec![json!(0), json!(1), json!(42), json!(-5)],
    ..Default::default()
  };
  let graph = create_test_graph(BTreeMap::from([("IntEnum".to_string(), enum_schema)]));
  let converter = SchemaConverter::new(&graph, FieldOptionalityPolicy::standard());
  let result = converter.convert_schema("IntEnum", graph.get_schema("IntEnum").unwrap())?;

  assert_eq!(result.len(), 1);
  let RustType::Enum(enum_def) = &result[0] else {
    panic!("Expected enum")
  };

  assert_eq!(enum_def.name, "IntEnum");
  assert_eq!(enum_def.variants.len(), 4);
  assert_eq!(enum_def.variants[0].name, "Value0");
  assert!(
    enum_def.variants[0]
      .serde_attrs
      .contains(&r#"rename = "0""#.to_string())
  );
  assert_eq!(enum_def.variants[1].name, "Value1");
  assert!(
    enum_def.variants[1]
      .serde_attrs
      .contains(&r#"rename = "1""#.to_string())
  );
  assert_eq!(enum_def.variants[2].name, "Value42");
  assert!(
    enum_def.variants[2]
      .serde_attrs
      .contains(&r#"rename = "42""#.to_string())
  );
  assert_eq!(enum_def.variants[3].name, "Value-5");
  assert!(
    enum_def.variants[3]
      .serde_attrs
      .contains(&r#"rename = "-5""#.to_string())
  );
  Ok(())
}

#[test]
fn test_float_enum_values() -> ConversionResult<()> {
  let enum_schema = ObjectSchema {
    schema_type: Some(SchemaTypeSet::Single(SchemaType::Number)),
    enum_values: vec![json!(0.0), json!(1.5), json!(PI), json!(-2.5)],
    ..Default::default()
  };
  let graph = create_test_graph(BTreeMap::from([("FloatEnum".to_string(), enum_schema)]));
  let converter = SchemaConverter::new(&graph, FieldOptionalityPolicy::standard());
  let result = converter.convert_schema("FloatEnum", graph.get_schema("FloatEnum").unwrap())?;

  assert_eq!(result.len(), 1);
  let RustType::Enum(enum_def) = &result[0] else {
    panic!("Expected enum")
  };

  assert_eq!(enum_def.name, "FloatEnum");
  assert_eq!(enum_def.variants.len(), 4);
  assert_eq!(enum_def.variants[0].name, "Value0");
  assert!(
    enum_def.variants[0]
      .serde_attrs
      .contains(&r#"rename = "0""#.to_string())
  );
  assert_eq!(enum_def.variants[1].name, "Value1_5");
  assert!(
    enum_def.variants[1]
      .serde_attrs
      .contains(&r#"rename = "1.5""#.to_string())
  );
  Ok(())
}

#[test]
fn test_boolean_enum_values() -> ConversionResult<()> {
  let enum_schema = ObjectSchema {
    schema_type: Some(SchemaTypeSet::Single(SchemaType::Boolean)),
    enum_values: vec![json!(true), json!(false)],
    ..Default::default()
  };
  let graph = create_test_graph(BTreeMap::from([("BoolEnum".to_string(), enum_schema)]));
  let converter = SchemaConverter::new(&graph, FieldOptionalityPolicy::standard());
  let result = converter.convert_schema("BoolEnum", graph.get_schema("BoolEnum").unwrap())?;

  assert_eq!(result.len(), 1);
  let RustType::Enum(enum_def) = &result[0] else {
    panic!("Expected enum")
  };

  assert_eq!(enum_def.name, "BoolEnum");
  assert_eq!(enum_def.variants.len(), 2);
  assert_eq!(enum_def.variants[0].name, "True");
  assert!(
    enum_def.variants[0]
      .serde_attrs
      .contains(&r#"rename = "true""#.to_string())
  );
  assert_eq!(enum_def.variants[1].name, "False");
  assert!(
    enum_def.variants[1]
      .serde_attrs
      .contains(&r#"rename = "false""#.to_string())
  );
  Ok(())
}

#[test]
fn test_mixed_type_enum_values() -> ConversionResult<()> {
  let enum_schema = ObjectSchema {
    enum_values: vec![json!("string"), json!(42), json!(1.5), json!(true)],
    ..Default::default()
  };
  let graph = create_test_graph(BTreeMap::from([("MixedEnum".to_string(), enum_schema)]));
  let converter = SchemaConverter::new(&graph, FieldOptionalityPolicy::standard());
  let result = converter.convert_schema("MixedEnum", graph.get_schema("MixedEnum").unwrap())?;

  assert_eq!(result.len(), 1);
  let RustType::Enum(enum_def) = &result[0] else {
    panic!("Expected enum")
  };

  assert_eq!(enum_def.name, "MixedEnum");
  assert_eq!(enum_def.variants.len(), 4);
  assert_eq!(enum_def.variants[0].name, "String");
  assert_eq!(enum_def.variants[1].name, "Value42");
  assert_eq!(enum_def.variants[2].name, "Value1_5");
  assert_eq!(enum_def.variants[3].name, "True");
  Ok(())
}

#[test]
fn test_empty_enum_converts_to_string() -> ConversionResult<()> {
  let enum_schema = ObjectSchema {
    schema_type: Some(SchemaTypeSet::Single(SchemaType::String)),
    enum_values: vec![],
    ..Default::default()
  };
  let graph = create_test_graph(BTreeMap::from([("EmptyEnum".to_string(), enum_schema)]));
  let converter = SchemaConverter::new(&graph, FieldOptionalityPolicy::standard());
  let result = converter.convert_schema("EmptyEnum", graph.get_schema("EmptyEnum").unwrap())?;

  assert_eq!(result.len(), 1);
  let RustType::TypeAlias(alias) = &result[0] else {
    panic!("Expected type alias for empty enum")
  };

  assert_eq!(alias.name, "EmptyEnum");
  assert_eq!(alias.target.to_rust_type(), "String");
  Ok(())
}

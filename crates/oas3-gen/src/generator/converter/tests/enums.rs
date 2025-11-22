use std::{collections::BTreeMap, f64::consts::PI};

use oas3::spec::{Discriminator, ObjectOrReference, ObjectSchema, SchemaType, SchemaTypeSet};
use serde_json::json;

use super::common::{config_with_no_helpers, config_with_preserve_case, create_test_graph, default_config};
use crate::generator::{
  ast::{DeriveTrait, EnumMethodKind, RustType, SerdeAttribute},
  converter::{
    ConversionResult, FieldOptionalityPolicy, SchemaConverter,
    enums::{CollisionStrategy, EnumConverter, VariantNameNormalizer},
    string_enum_optimizer::StringEnumOptimizer,
    type_resolver::TypeResolver,
  },
  schema_graph::SchemaGraph,
};

#[test]
fn test_simple_string_enum() -> ConversionResult<()> {
  let enum_schema = ObjectSchema {
    schema_type: Some(SchemaTypeSet::Single(SchemaType::String)),
    enum_values: vec![json!("value1"), json!("value2")],
    ..Default::default()
  };
  let graph = create_test_graph(BTreeMap::from([("SimpleEnum".to_string(), enum_schema)]));
  let converter = SchemaConverter::new(&graph, FieldOptionalityPolicy::standard(), default_config());
  let result = converter.convert_schema("SimpleEnum", graph.get_schema("SimpleEnum").unwrap(), None)?;

  assert_eq!(result.len(), 1);
  let RustType::Enum(enum_def) = &result[0] else {
    panic!("Expected enum")
  };

  assert_eq!(enum_def.name, "SimpleEnum");
  assert_eq!(enum_def.variants.len(), 2);
  assert!(enum_def.derives.contains(&DeriveTrait::Eq));
  assert!(enum_def.derives.contains(&DeriveTrait::Hash));
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
  let converter = SchemaConverter::new(&graph, FieldOptionalityPolicy::standard(), default_config());
  let result = converter.convert_schema("TestUnion", graph.get_schema("TestUnion").unwrap(), None)?;

  let RustType::Enum(enum_def) = result.last().unwrap() else {
    panic!("Expected enum as last type")
  };

  assert_eq!(enum_def.name, "TestUnion");
  assert_eq!(enum_def.variants.len(), 2);
  assert!(
    enum_def.variants[0]
      .serde_attrs
      .contains(&SerdeAttribute::Rename("type_a".to_string()))
  );
  assert!(
    enum_def.variants[1]
      .serde_attrs
      .contains(&SerdeAttribute::Rename("type_b".to_string()))
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
  let converter = SchemaConverter::new(&graph, FieldOptionalityPolicy::standard(), default_config());
  let result = converter.convert_schema("TestUnion", graph.get_schema("TestUnion").unwrap(), None)?;

  let RustType::Enum(enum_def) = result.last().unwrap() else {
    panic!("Expected enum as last type")
  };

  assert_eq!(enum_def.name, "TestUnion");
  assert_eq!(enum_def.variants.len(), 2);
  assert!(enum_def.variants[0].serde_attrs.is_empty());
  assert!(enum_def.variants[1].serde_attrs.is_empty());
  assert!(enum_def.serde_attrs.contains(&SerdeAttribute::Untagged));
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
  let converter = SchemaConverter::new(&graph, FieldOptionalityPolicy::standard(), default_config());
  let result = converter.convert_schema("TestUnion", graph.get_schema("TestUnion").unwrap(), None)?;

  let RustType::Enum(enum_def) = result.last().unwrap() else {
    panic!("Expected enum as last type")
  };

  assert_eq!(enum_def.name, "TestUnion");
  assert_eq!(enum_def.discriminator, Some("type".to_string()));
  assert!(!enum_def.serde_attrs.contains(&SerdeAttribute::Untagged));
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
  let converter = SchemaConverter::new(&graph, FieldOptionalityPolicy::standard(), default_config());
  let result = converter.convert_schema("IntEnum", graph.get_schema("IntEnum").unwrap(), None)?;

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
      .contains(&SerdeAttribute::Rename("0".to_string()))
  );
  assert_eq!(enum_def.variants[1].name, "Value1");
  assert!(
    enum_def.variants[1]
      .serde_attrs
      .contains(&SerdeAttribute::Rename("1".to_string()))
  );
  assert_eq!(enum_def.variants[2].name, "Value42");
  assert!(
    enum_def.variants[2]
      .serde_attrs
      .contains(&SerdeAttribute::Rename("42".to_string()))
  );
  assert_eq!(enum_def.variants[3].name, "Value-5");
  assert!(
    enum_def.variants[3]
      .serde_attrs
      .contains(&SerdeAttribute::Rename("-5".to_string()))
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
  let converter = SchemaConverter::new(&graph, FieldOptionalityPolicy::standard(), default_config());
  let result = converter.convert_schema("FloatEnum", graph.get_schema("FloatEnum").unwrap(), None)?;

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
      .contains(&SerdeAttribute::Rename("0".to_string()))
  );
  assert_eq!(enum_def.variants[1].name, "Value1_5");
  assert!(
    enum_def.variants[1]
      .serde_attrs
      .contains(&SerdeAttribute::Rename("1.5".to_string()))
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
  let converter = SchemaConverter::new(&graph, FieldOptionalityPolicy::standard(), default_config());
  let result = converter.convert_schema("BoolEnum", graph.get_schema("BoolEnum").unwrap(), None)?;

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
      .contains(&SerdeAttribute::Rename("true".to_string()))
  );
  assert_eq!(enum_def.variants[1].name, "False");
  assert!(
    enum_def.variants[1]
      .serde_attrs
      .contains(&SerdeAttribute::Rename("false".to_string()))
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
  let converter = SchemaConverter::new(&graph, FieldOptionalityPolicy::standard(), default_config());
  let result = converter.convert_schema("MixedEnum", graph.get_schema("MixedEnum").unwrap(), None)?;

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
  let converter = SchemaConverter::new(&graph, FieldOptionalityPolicy::standard(), default_config());
  let result = converter.convert_schema("EmptyEnum", graph.get_schema("EmptyEnum").unwrap(), None)?;

  assert_eq!(result.len(), 1);
  let RustType::TypeAlias(alias) = &result[0] else {
    panic!("Expected type alias for empty enum")
  };

  assert_eq!(alias.name, "EmptyEnum");
  assert_eq!(alias.target.to_rust_type(), "String");
  Ok(())
}

#[test]
fn test_case_insensitive_duplicates_with_deduplication() -> ConversionResult<()> {
  let enum_schema = ObjectSchema {
    schema_type: Some(SchemaTypeSet::Single(SchemaType::String)),
    enum_values: vec![json!("ITEM"), json!("item"), json!("SELECT"), json!("select")],
    ..Default::default()
  };
  let graph = create_test_graph(BTreeMap::from([("CaseEnum".to_string(), enum_schema)]));
  let converter = SchemaConverter::new(&graph, FieldOptionalityPolicy::standard(), default_config());
  let result = converter.convert_schema("CaseEnum", graph.get_schema("CaseEnum").unwrap(), None)?;

  assert_eq!(result.len(), 1);
  let RustType::Enum(enum_def) = &result[0] else {
    panic!("Expected enum")
  };

  assert_eq!(enum_def.name, "CaseEnum");
  assert_eq!(enum_def.variants.len(), 2);
  assert_eq!(enum_def.variants[0].name, "Item");
  assert!(
    enum_def.variants[0]
      .serde_attrs
      .contains(&SerdeAttribute::Rename("ITEM".to_string()))
  );
  assert!(
    enum_def.variants[0]
      .serde_attrs
      .contains(&SerdeAttribute::Alias("item".to_string()))
  );
  assert_eq!(enum_def.variants[1].name, "Select");
  assert!(
    enum_def.variants[1]
      .serde_attrs
      .contains(&SerdeAttribute::Rename("SELECT".to_string()))
  );
  assert!(
    enum_def.variants[1]
      .serde_attrs
      .contains(&SerdeAttribute::Alias("select".to_string()))
  );
  Ok(())
}

#[test]
fn test_case_insensitive_duplicates_with_preservation() -> ConversionResult<()> {
  let enum_schema = ObjectSchema {
    schema_type: Some(SchemaTypeSet::Single(SchemaType::String)),
    enum_values: vec![json!("ITEM"), json!("item"), json!("SELECT"), json!("select")],
    ..Default::default()
  };
  let graph = create_test_graph(BTreeMap::from([("CaseEnum".to_string(), enum_schema)]));
  let converter = SchemaConverter::new(&graph, FieldOptionalityPolicy::standard(), config_with_preserve_case());
  let result = converter.convert_schema("CaseEnum", graph.get_schema("CaseEnum").unwrap(), None)?;

  assert_eq!(result.len(), 1);
  let RustType::Enum(enum_def) = &result[0] else {
    panic!("Expected enum")
  };

  assert_eq!(enum_def.name, "CaseEnum");
  assert_eq!(enum_def.variants.len(), 4);
  assert_eq!(enum_def.variants[0].name, "Item");
  assert!(
    enum_def.variants[0]
      .serde_attrs
      .contains(&SerdeAttribute::Rename("ITEM".to_string()))
  );
  assert_eq!(enum_def.variants[1].name, "Item1");
  assert!(
    enum_def.variants[1]
      .serde_attrs
      .contains(&SerdeAttribute::Rename("item".to_string()))
  );
  assert_eq!(enum_def.variants[2].name, "Select");
  assert!(
    enum_def.variants[2]
      .serde_attrs
      .contains(&SerdeAttribute::Rename("SELECT".to_string()))
  );
  assert_eq!(enum_def.variants[3].name, "Select3");
  assert!(
    enum_def.variants[3]
      .serde_attrs
      .contains(&SerdeAttribute::Rename("select".to_string()))
  );
  Ok(())
}

#[test]
fn test_normalize_string() {
  let val = json!("active");
  let res = VariantNameNormalizer::normalize(&val).unwrap();
  assert_eq!(res.rename_value, "active");
}

#[test]
fn test_normalize_int() {
  let val = json!(404);
  let res = VariantNameNormalizer::normalize(&val).unwrap();
  assert_eq!(res.name, "Value404");
  assert_eq!(res.rename_value, "404");
}

#[test]
#[allow(clippy::approx_constant)]
fn test_normalize_float() {
  let val = json!(3.14);
  let res = VariantNameNormalizer::normalize(&val).unwrap();
  assert_eq!(res.name, "Value3_14");
  assert_eq!(res.rename_value, "3.14");
}

#[test]
fn test_normalize_bool() {
  let val = json!(true);
  let res = VariantNameNormalizer::normalize(&val).unwrap();
  assert_eq!(res.name, "True");
  assert_eq!(res.rename_value, "true");

  let val = json!(false);
  let res = VariantNameNormalizer::normalize(&val).unwrap();
  assert_eq!(res.name, "False");
  assert_eq!(res.rename_value, "false");
}

#[test]
fn test_normalize_invalid() {
  let val = json!({});
  assert!(VariantNameNormalizer::normalize(&val).is_none());
}

#[test]
fn test_collision_strategy_enum() {
  let s1 = CollisionStrategy::Preserve;
  let s2 = CollisionStrategy::Deduplicate;
  assert_ne!(s1, s2);
}

#[test]
fn test_preserve_strategy_with_multiple_collisions() {
  let spec = oas3::Spec {
    openapi: "3.1.0".to_string(),
    info: oas3::spec::Info {
      title: "Test".to_string(),
      summary: None,
      version: "1.0.0".to_string(),
      description: None,
      terms_of_service: None,
      contact: None,
      license: None,
      extensions: BTreeMap::default(),
    },
    servers: vec![],
    paths: None,
    webhooks: BTreeMap::default(),
    components: None,
    security: vec![],
    tags: vec![],
    external_docs: None,
    extensions: BTreeMap::default(),
  };

  let (graph, _) = SchemaGraph::new(spec);
  let type_resolver = TypeResolver::new(&graph, default_config());
  let converter = EnumConverter::new(&graph, type_resolver, config_with_preserve_case());

  let schema = ObjectSchema {
    enum_values: vec![json!("active"), json!("Active"), json!("ACTIVE")],
    ..Default::default()
  };

  let result = converter.convert_simple_enum("Status", &schema, None);

  if let Some(RustType::Enum(enum_def)) = result {
    assert_eq!(enum_def.variants.len(), 3);
    assert_eq!(enum_def.variants[0].name, "Active");
    assert_eq!(enum_def.variants[1].name, "Active1");
    assert_eq!(enum_def.variants[2].name, "Active2");
  } else {
    panic!("Expected enum result");
  }
}

#[test]
fn test_string_enum_optimizer_detects_freeform_pattern() {
  let spec = oas3::Spec {
    openapi: "3.1.0".to_string(),
    info: oas3::spec::Info {
      title: "Test".to_string(),
      summary: None,
      version: "1.0.0".to_string(),
      description: None,
      terms_of_service: None,
      contact: None,
      license: None,
      extensions: BTreeMap::default(),
    },
    servers: vec![],
    paths: None,
    webhooks: BTreeMap::default(),
    components: None,
    security: vec![],
    tags: vec![],
    external_docs: None,
    extensions: BTreeMap::default(),
  };

  let (graph, _) = SchemaGraph::new(spec);
  let optimizer = StringEnumOptimizer::new(&graph, false);

  let schema = ObjectSchema {
    any_of: vec![
      ObjectOrReference::Object(ObjectSchema {
        schema_type: Some(SchemaTypeSet::Single(SchemaType::String)),
        const_value: Some(json!("known1")),
        ..Default::default()
      }),
      ObjectOrReference::Object(ObjectSchema {
        schema_type: Some(SchemaTypeSet::Single(SchemaType::String)),
        const_value: Some(json!("known2")),
        ..Default::default()
      }),
      ObjectOrReference::Object(ObjectSchema {
        schema_type: Some(SchemaTypeSet::Single(SchemaType::String)),
        ..Default::default()
      }),
    ],
    ..Default::default()
  };

  let result = optimizer.try_convert("TestEnum", &schema, None);
  assert!(result.is_some());

  let types = result.unwrap();
  assert_eq!(types.len(), 2);

  let has_known_enum = types.iter().any(|t| match t {
    RustType::Enum(e) => e.name == "TestEnumKnown",
    _ => false,
  });
  let has_outer_enum = types.iter().any(|t| match t {
    RustType::Enum(e) => e.name == "TestEnum",
    _ => false,
  });

  assert!(has_known_enum);
  assert!(has_outer_enum);
}

#[test]
fn test_string_enum_optimizer_rejects_no_freeform() {
  let spec = oas3::Spec {
    openapi: "3.1.0".to_string(),
    info: oas3::spec::Info {
      title: "Test".to_string(),
      summary: None,
      version: "1.0.0".to_string(),
      description: None,
      terms_of_service: None,
      contact: None,
      license: None,
      extensions: BTreeMap::default(),
    },
    servers: vec![],
    paths: None,
    webhooks: BTreeMap::default(),
    components: None,
    security: vec![],
    tags: vec![],
    external_docs: None,
    extensions: BTreeMap::default(),
  };

  let (graph, _) = SchemaGraph::new(spec);
  let optimizer = StringEnumOptimizer::new(&graph, false);

  let schema = ObjectSchema {
    any_of: vec![
      ObjectOrReference::Object(ObjectSchema {
        schema_type: Some(SchemaTypeSet::Single(SchemaType::String)),
        const_value: Some(json!("known1")),
        ..Default::default()
      }),
      ObjectOrReference::Object(ObjectSchema {
        schema_type: Some(SchemaTypeSet::Single(SchemaType::String)),
        const_value: Some(json!("known2")),
        ..Default::default()
      }),
    ],
    ..Default::default()
  };

  let result = optimizer.try_convert("TestEnum", &schema, None);
  assert!(result.is_none());
}

#[test]
fn test_anyof_with_const_generates_unit_variant() -> ConversionResult<()> {
  let text_schema = ObjectSchema {
    schema_type: Some(SchemaTypeSet::Single(SchemaType::Object)),
    properties: BTreeMap::from([(
      "type".to_string(),
      ObjectOrReference::Object(ObjectSchema {
        schema_type: Some(SchemaTypeSet::Single(SchemaType::String)),
        const_value: Some(json!("text")),
        ..Default::default()
      }),
    )]),
    ..Default::default()
  };

  let parent_schema = ObjectSchema {
    any_of: vec![
      ObjectOrReference::Object(ObjectSchema {
        schema_type: Some(SchemaTypeSet::Single(SchemaType::String)),
        const_value: Some(json!("auto")),
        description: Some("`auto` is the default value".to_string()),
        ..Default::default()
      }),
      ObjectOrReference::Ref {
        ref_path: "#/components/schemas/TextFormat".to_string(),
        description: None,
        summary: None,
      },
    ],
    description: Some("Response format option".to_string()),
    ..Default::default()
  };

  let schemas = BTreeMap::from([
    ("ResponseFormat".to_string(), parent_schema.clone()),
    ("TextFormat".to_string(), text_schema),
  ]);
  let graph = create_test_graph(schemas);
  let converter = SchemaConverter::new(&graph, FieldOptionalityPolicy::standard(), default_config());

  let result = converter.convert_schema("ResponseFormat", &parent_schema, None)?;

  assert!(!result.is_empty());
  let RustType::Enum(enum_def) = &result[result.len() - 1] else {
    panic!("Expected enum as last type, got: {result:?}");
  };

  assert_eq!(enum_def.name, "ResponseFormat");
  assert_eq!(enum_def.variants.len(), 2);

  let auto_variant = &enum_def.variants[0];
  assert_eq!(auto_variant.name, "Auto");
  assert!(matches!(
    auto_variant.content,
    crate::generator::ast::VariantContent::Unit
  ));
  assert_eq!(
    auto_variant.serde_attrs,
    vec![SerdeAttribute::Rename("auto".to_string())]
  );

  let text_variant = &enum_def.variants[1];
  assert_eq!(text_variant.name, "TextFormat");
  assert!(matches!(
    text_variant.content,
    crate::generator::ast::VariantContent::Tuple(_)
  ));

  Ok(())
}

#[test]
fn test_const_unit_variant_in_enum() {
  use serde::{Deserialize, Serialize};

  #[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
  #[serde(default)]
  struct DataVariant {
    #[serde(rename = "type")]
    r#type: String,
    value: i32,
  }

  #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
  #[serde(untagged)]
  enum TestEnum {
    #[serde(rename = "auto")]
    Auto,
    Data(DataVariant),
  }

  let auto = TestEnum::Auto;
  let data = TestEnum::Data(DataVariant {
    r#type: "data".to_string(),
    value: 42,
  });

  let auto_json = serde_json::to_string(&auto).unwrap();
  let data_json = serde_json::to_value(&data).unwrap();

  assert_eq!(auto_json, "null");
  assert_eq!(data_json["type"], "data");
  assert_eq!(data_json["value"], 42);
}

#[test]
fn test_openapi_response_format_serialization() {
  use serde::{Deserialize, Serialize};

  #[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
  #[serde(default)]
  struct ResponseFormatText {
    #[serde(rename = "type")]
    #[serde(default = "default_text_type")]
    r#type: String,
  }

  fn default_text_type() -> String {
    "text".to_string()
  }

  #[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
  #[serde(default)]
  struct ResponseFormatJsonSchema {
    #[serde(rename = "type")]
    #[serde(default = "default_json_schema_type")]
    r#type: String,
    json_schema: serde_json::Value,
  }

  fn default_json_schema_type() -> String {
    "json_schema".to_string()
  }

  #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
  #[serde(untagged)]
  enum ResponseFormat {
    #[serde(rename = "auto")]
    Auto,
    Text(ResponseFormatText),
    JsonSchema(ResponseFormatJsonSchema),
  }

  let text = ResponseFormat::Text(ResponseFormatText {
    r#type: "text".to_string(),
  });
  let json_schema = ResponseFormat::JsonSchema(ResponseFormatJsonSchema {
    r#type: "json_schema".to_string(),
    json_schema: serde_json::json!({"type": "object"}),
  });

  let text_json = serde_json::to_value(&text).unwrap();
  let json_schema_json = serde_json::to_value(&json_schema).unwrap();

  assert_eq!(text_json["type"], "text");
  assert_eq!(json_schema_json["type"], "json_schema");
  assert_eq!(json_schema_json["json_schema"]["type"], "object");
}

#[test]
fn test_enum_helper_methods_generation() -> ConversionResult<()> {
  let simple_struct_schema = ObjectSchema {
    title: Some("Simple".to_string()),
    properties: BTreeMap::from([(
      "opt_field".to_string(),
      ObjectOrReference::Object(ObjectSchema {
        schema_type: Some(SchemaTypeSet::Single(SchemaType::String)),
        ..Default::default()
      }),
    )]),
    ..Default::default()
  };

  let required_struct_schema = ObjectSchema {
    title: Some("SingleParam".to_string()),
    properties: BTreeMap::from([(
      "req_field".to_string(),
      ObjectOrReference::Object(ObjectSchema {
        schema_type: Some(SchemaTypeSet::Single(SchemaType::String)),
        ..Default::default()
      }),
    )]),
    required: vec!["req_field".to_string()],
    ..Default::default()
  };

  let complex_struct_schema = ObjectSchema {
    title: Some("Complex".to_string()),
    properties: BTreeMap::from([
      (
        "req1".to_string(),
        ObjectOrReference::Object(ObjectSchema {
          schema_type: Some(SchemaTypeSet::Single(SchemaType::String)),
          ..Default::default()
        }),
      ),
      (
        "req2".to_string(),
        ObjectOrReference::Object(ObjectSchema {
          schema_type: Some(SchemaTypeSet::Single(SchemaType::String)),
          ..Default::default()
        }),
      ),
    ]),
    required: vec!["req1".to_string(), "req2".to_string()],
    ..Default::default()
  };

  let union_schema = ObjectSchema {
    one_of: vec![
      ObjectOrReference::Object(simple_struct_schema),
      ObjectOrReference::Object(required_struct_schema),
      ObjectOrReference::Object(complex_struct_schema),
    ],
    ..Default::default()
  };

  let graph = create_test_graph(BTreeMap::from([("TestUnion".to_string(), union_schema)]));

  let converter = SchemaConverter::new(&graph, FieldOptionalityPolicy::standard(), default_config());
  let result = converter.convert_schema("TestUnion", graph.get_schema("TestUnion").unwrap(), None)?;

  let RustType::Enum(enum_def) = result.last().unwrap() else {
    panic!("Expected enum")
  };

  assert_eq!(enum_def.methods.len(), 2); // Simple + SingleParam, Complex skipped

  // Check Simple Constructor
  let simple_method = enum_def
    .methods
    .iter()
    .find(|m| m.name == "simple")
    .expect("simple method not found");
  match &simple_method.kind {
    EnumMethodKind::SimpleConstructor {
      variant_name,
      wrapped_type,
    } => {
      assert_eq!(variant_name, "Simple");
      assert_eq!(wrapped_type, "TestUnionSimple");
    }
    EnumMethodKind::ParameterizedConstructor { .. } => panic!("Expected SimpleConstructor"),
  }

  // Check Parameterized Constructor
  let param_method = enum_def
    .methods
    .iter()
    .find(|m| m.name == "single_param")
    .expect("single_param method not found");
  match &param_method.kind {
    EnumMethodKind::ParameterizedConstructor {
      variant_name,
      wrapped_type,
      param_name,
      param_type,
    } => {
      assert_eq!(variant_name, "SingleParam");
      assert_eq!(wrapped_type, "TestUnionSingleParam");
      assert_eq!(param_name, "req_field");
      assert_eq!(param_type, "String");
    }
    EnumMethodKind::SimpleConstructor { .. } => panic!("Expected ParameterizedConstructor"),
  }

  Ok(())
}

#[test]
fn test_enum_helper_methods_disabled_flag() -> ConversionResult<()> {
  let simple_struct_schema = ObjectSchema {
    title: Some("Simple".to_string()),
    properties: BTreeMap::from([(
      "opt_field".to_string(),
      ObjectOrReference::Object(ObjectSchema {
        schema_type: Some(SchemaTypeSet::Single(SchemaType::String)),
        ..Default::default()
      }),
    )]),
    ..Default::default()
  };

  let union_schema = ObjectSchema {
    one_of: vec![ObjectOrReference::Object(simple_struct_schema)],
    ..Default::default()
  };

  let graph = create_test_graph(BTreeMap::from([("TestUnion".to_string(), union_schema)]));

  // no_helpers = true
  let converter = SchemaConverter::new(&graph, FieldOptionalityPolicy::standard(), config_with_no_helpers());
  let result = converter.convert_schema("TestUnion", graph.get_schema("TestUnion").unwrap(), None)?;

  let RustType::Enum(enum_def) = result.last().unwrap() else {
    panic!("Expected enum")
  };

  assert!(enum_def.methods.is_empty());
  Ok(())
}

#[test]
fn test_enum_helper_naming_stripping() -> ConversionResult<()> {
  let simple_schema = ObjectSchema {
    title: Some("ResponseFormatText".to_string()),
    properties: BTreeMap::from([(
      "dummy".to_string(),
      ObjectOrReference::Object(ObjectSchema {
        schema_type: Some(SchemaTypeSet::Single(SchemaType::String)),
        ..Default::default()
      }),
    )]),
    ..Default::default()
  };

  let union_schema = ObjectSchema {
    one_of: vec![ObjectOrReference::Object(simple_schema)],
    ..Default::default()
  };

  let graph = create_test_graph(BTreeMap::from([("ResponseFormat".to_string(), union_schema)]));

  let converter = SchemaConverter::new(&graph, FieldOptionalityPolicy::standard(), default_config());
  let result = converter.convert_schema("ResponseFormat", graph.get_schema("ResponseFormat").unwrap(), None)?;

  let RustType::Enum(enum_def) = result.last().unwrap() else {
    panic!("Expected enum")
  };

  let method = enum_def.methods.first().unwrap();
  assert_eq!(method.name, "text");

  Ok(())
}

#[test]
fn test_enum_helper_method_name_collision() -> ConversionResult<()> {
  let schema1 = ObjectSchema {
    title: Some("StatusActive".to_string()),
    properties: BTreeMap::from([(
      "opt_field".to_string(),
      ObjectOrReference::Object(ObjectSchema {
        schema_type: Some(SchemaTypeSet::Single(SchemaType::String)),
        ..Default::default()
      }),
    )]),
    ..Default::default()
  };

  let schema2 = ObjectSchema {
    title: Some("Active".to_string()),
    properties: BTreeMap::from([(
      "opt_field2".to_string(),
      ObjectOrReference::Object(ObjectSchema {
        schema_type: Some(SchemaTypeSet::Single(SchemaType::String)),
        ..Default::default()
      }),
    )]),
    ..Default::default()
  };

  let union_schema = ObjectSchema {
    one_of: vec![ObjectOrReference::Object(schema1), ObjectOrReference::Object(schema2)],
    ..Default::default()
  };

  let graph = create_test_graph(BTreeMap::from([("Status".to_string(), union_schema)]));

  let converter = SchemaConverter::new(&graph, FieldOptionalityPolicy::standard(), default_config());
  let result = converter.convert_schema("Status", graph.get_schema("Status").unwrap(), None)?;

  let RustType::Enum(enum_def) = result.last().unwrap() else {
    panic!("Expected enum")
  };

  assert_eq!(enum_def.methods.len(), 2);
  let names: Vec<_> = enum_def.methods.iter().map(|m| &m.name).collect();
  assert!(names.contains(&&"active".to_string()));
  assert!(names.contains(&&"active2".to_string()) || names.iter().any(|n| *n != "active"));

  Ok(())
}

#[test]
fn test_enum_helper_skips_without_default_trait() {
  use std::collections::BTreeSet;

  use crate::generator::ast::{RustPrimitive, TypeRef};

  let enum_def = RustType::Enum(crate::generator::ast::EnumDef {
    name: "TestEnum".to_string(),
    docs: vec![],
    variants: vec![crate::generator::ast::VariantDef {
      name: "Variant".to_string(),
      docs: vec![],
      content: crate::generator::ast::VariantContent::Tuple(vec![TypeRef::new(RustPrimitive::Custom(
        "TestVariant".to_string(),
      ))]),
      serde_attrs: vec![],
      deprecated: false,
    }],
    discriminator: None,
    derives: BTreeSet::new(),
    serde_attrs: vec![],
    outer_attrs: vec![],
    case_insensitive: false,
    methods: vec![],
  });

  if let RustType::Enum(e) = enum_def {
    assert!(e.methods.is_empty());
  }
}

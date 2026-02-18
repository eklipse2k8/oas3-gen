use std::{
  collections::{BTreeMap, BTreeSet},
  f64::consts::PI,
  sync::Arc,
};

use oas3::spec::{Discriminator, Info, ObjectOrReference, ObjectSchema, Schema, SchemaType, SchemaTypeSet};
use serde_json::json;

use crate::{
  generator::{
    ast::{
      DeriveTrait, DerivesProvider, EnumDef, EnumMethodKind, EnumToken, EnumVariantToken, MethodNameToken,
      RustPrimitive, RustType, SerdeAttribute, TypeRef, VariantContent, VariantDef,
    },
    converter::{
      SchemaConverter,
      union_types::CollisionStrategy,
      unions::{EnumConverter, UnionConverter},
    },
    metrics::GenerationStats,
    naming::constants::KNOWN_ENUM_VARIANT,
    schema_registry::SchemaRegistry,
  },
  tests::common::{
    config_with_no_helpers, config_with_preserve_case, create_test_context, create_test_graph, default_config,
  },
};

#[test]
fn test_simple_string_enum() -> anyhow::Result<()> {
  let enum_schema = ObjectSchema {
    schema_type: Some(SchemaTypeSet::Single(SchemaType::String)),
    enum_values: vec![json!("value1"), json!("value2")],
    ..Default::default()
  };
  let graph = create_test_graph(BTreeMap::from([("SimpleEnum".to_string(), enum_schema)]));
  let context = create_test_context(graph.clone(), default_config());
  let converter = SchemaConverter::new(&context);
  let result = converter.convert_schema("SimpleEnum", graph.get("SimpleEnum").unwrap())?;

  assert_eq!(result.len(), 1);
  let RustType::Enum(enum_def) = &result[0] else {
    panic!("Expected enum")
  };

  assert_eq!(enum_def.name.to_string(), "SimpleEnum");
  assert_eq!(enum_def.variants.len(), 2);
  assert!(enum_def.derives().contains(&DeriveTrait::Eq));
  assert!(enum_def.derives().contains(&DeriveTrait::Hash));
  Ok(())
}

#[test]
fn test_oneof_with_discriminator_has_rename_attrs() -> anyhow::Result<()> {
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
  let context = create_test_context(graph.clone(), default_config());
  let converter = SchemaConverter::new(&context);
  let result = converter.convert_schema("TestUnion", graph.get("TestUnion").unwrap())?;

  let RustType::DiscriminatedEnum(enum_def) = result.last().unwrap() else {
    panic!("Expected DiscriminatedEnum as last type")
  };

  assert_eq!(enum_def.name.to_string(), "TestUnion");
  assert_eq!(enum_def.discriminator_field, "type");
  assert_eq!(enum_def.variants.len(), 2);
  let variant_values = enum_def
    .variants
    .iter()
    .flat_map(|v| v.discriminator_values.iter().map(String::as_str))
    .collect::<BTreeSet<_>>();
  assert!(variant_values.contains("type_a"));
  assert!(variant_values.contains("type_b"));
  Ok(())
}

#[test]
fn test_anyof_without_discriminator_has_no_rename_attrs() -> anyhow::Result<()> {
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
  let context = create_test_context(graph.clone(), default_config());
  let converter = SchemaConverter::new(&context);
  let result = converter.convert_schema("TestUnion", graph.get("TestUnion").unwrap())?;

  let RustType::Enum(enum_def) = result.last().unwrap() else {
    panic!("Expected enum as last type")
  };

  assert_eq!(enum_def.name.to_string(), "TestUnion");
  assert_eq!(enum_def.variants.len(), 2);
  assert!(enum_def.variants[0].serde_attrs.is_empty());
  assert!(enum_def.variants[1].serde_attrs.is_empty());
  assert!(enum_def.serde_attrs.contains(&SerdeAttribute::Untagged));
  Ok(())
}

#[test]
fn test_anyof_with_discriminator_no_untagged() -> anyhow::Result<()> {
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
  let context = create_test_context(graph.clone(), default_config());
  let converter = SchemaConverter::new(&context);
  let result = converter.convert_schema("TestUnion", graph.get("TestUnion").unwrap())?;

  let RustType::DiscriminatedEnum(enum_def) = result.last().unwrap() else {
    panic!("Expected DiscriminatedEnum as last type")
  };

  assert_eq!(enum_def.name.to_string(), "TestUnion");
  assert_eq!(enum_def.discriminator_field, "type");
  assert_eq!(enum_def.variants.len(), 2);
  Ok(())
}

#[test]
fn test_integer_enum_values() -> anyhow::Result<()> {
  let enum_schema = ObjectSchema {
    schema_type: Some(SchemaTypeSet::Single(SchemaType::Integer)),
    enum_values: vec![json!(0), json!(1), json!(42), json!(-5)],
    ..Default::default()
  };
  let graph = create_test_graph(BTreeMap::from([("IntEnum".to_string(), enum_schema)]));
  let context = create_test_context(graph.clone(), default_config());
  let converter = SchemaConverter::new(&context);
  let result = converter.convert_schema("IntEnum", graph.get("IntEnum").unwrap())?;

  assert_eq!(result.len(), 1);
  let RustType::Enum(enum_def) = &result[0] else {
    panic!("Expected enum")
  };

  assert_eq!(enum_def.name.to_string(), "IntEnum");
  assert_eq!(enum_def.variants.len(), 4);
  assert_eq!(enum_def.variants[0].name, EnumVariantToken::new("Value0"));
  assert!(
    enum_def.variants[0]
      .serde_attrs
      .contains(&SerdeAttribute::Rename("0".to_string()))
  );
  assert_eq!(enum_def.variants[1].name, EnumVariantToken::new("Value1"));
  assert!(
    enum_def.variants[1]
      .serde_attrs
      .contains(&SerdeAttribute::Rename("1".to_string()))
  );
  assert_eq!(enum_def.variants[2].name, EnumVariantToken::new("Value42"));
  assert!(
    enum_def.variants[2]
      .serde_attrs
      .contains(&SerdeAttribute::Rename("42".to_string()))
  );
  assert_eq!(enum_def.variants[3].name, EnumVariantToken::new("Value_5"));
  assert!(
    enum_def.variants[3]
      .serde_attrs
      .contains(&SerdeAttribute::Rename("-5".to_string()))
  );
  Ok(())
}

#[test]
fn test_float_enum_values() -> anyhow::Result<()> {
  let enum_schema = ObjectSchema {
    schema_type: Some(SchemaTypeSet::Single(SchemaType::Number)),
    enum_values: vec![json!(0.0), json!(1.5), json!(PI), json!(-2.5)],
    ..Default::default()
  };
  let graph = create_test_graph(BTreeMap::from([("FloatEnum".to_string(), enum_schema)]));
  let context = create_test_context(graph.clone(), default_config());
  let converter = SchemaConverter::new(&context);
  let result = converter.convert_schema("FloatEnum", graph.get("FloatEnum").unwrap())?;

  assert_eq!(result.len(), 1);
  let RustType::Enum(enum_def) = &result[0] else {
    panic!("Expected enum")
  };

  assert_eq!(enum_def.name.to_string(), "FloatEnum");
  assert_eq!(enum_def.variants.len(), 4);
  assert_eq!(enum_def.variants[0].name, EnumVariantToken::new("Value0"));
  assert!(
    enum_def.variants[0]
      .serde_attrs
      .contains(&SerdeAttribute::Rename("0".to_string()))
  );
  assert_eq!(enum_def.variants[1].name, EnumVariantToken::new("Value1_5"));
  assert!(
    enum_def.variants[1]
      .serde_attrs
      .contains(&SerdeAttribute::Rename("1.5".to_string()))
  );
  Ok(())
}

#[test]
fn test_boolean_enum_values() -> anyhow::Result<()> {
  let enum_schema = ObjectSchema {
    schema_type: Some(SchemaTypeSet::Single(SchemaType::Boolean)),
    enum_values: vec![json!(true), json!(false)],
    ..Default::default()
  };
  let graph = create_test_graph(BTreeMap::from([("BoolEnum".to_string(), enum_schema)]));
  let context = create_test_context(graph.clone(), default_config());
  let converter = SchemaConverter::new(&context);
  let result = converter.convert_schema("BoolEnum", graph.get("BoolEnum").unwrap())?;

  assert_eq!(result.len(), 1);
  let RustType::Enum(enum_def) = &result[0] else {
    panic!("Expected enum")
  };

  assert_eq!(enum_def.name.to_string(), "BoolEnum");
  assert_eq!(enum_def.variants.len(), 2);
  assert_eq!(enum_def.variants[0].name, EnumVariantToken::new("True"));
  assert!(
    enum_def.variants[0]
      .serde_attrs
      .contains(&SerdeAttribute::Rename("true".to_string()))
  );
  assert_eq!(enum_def.variants[1].name, EnumVariantToken::new("False"));
  assert!(
    enum_def.variants[1]
      .serde_attrs
      .contains(&SerdeAttribute::Rename("false".to_string()))
  );
  Ok(())
}

#[test]
fn test_mixed_type_enum_values() -> anyhow::Result<()> {
  let enum_schema = ObjectSchema {
    enum_values: vec![json!("string"), json!(42), json!(1.5), json!(true)],
    ..Default::default()
  };
  let graph = create_test_graph(BTreeMap::from([("MixedEnum".to_string(), enum_schema)]));
  let context = create_test_context(graph.clone(), default_config());
  let converter = SchemaConverter::new(&context);
  let result = converter.convert_schema("MixedEnum", graph.get("MixedEnum").unwrap())?;

  assert_eq!(result.len(), 1);
  let RustType::Enum(enum_def) = &result[0] else {
    panic!("Expected enum")
  };

  assert_eq!(enum_def.name.to_string(), "MixedEnum");
  assert_eq!(enum_def.variants.len(), 4);
  assert_eq!(enum_def.variants[0].name, EnumVariantToken::new("String"));
  assert_eq!(enum_def.variants[1].name, EnumVariantToken::new("Value42"));
  assert_eq!(enum_def.variants[2].name, EnumVariantToken::new("Value1_5"));
  assert_eq!(enum_def.variants[3].name, EnumVariantToken::new("True"));
  Ok(())
}

#[test]
fn test_empty_enum_converts_to_string() -> anyhow::Result<()> {
  let enum_schema = ObjectSchema {
    schema_type: Some(SchemaTypeSet::Single(SchemaType::String)),
    enum_values: vec![],
    ..Default::default()
  };
  let graph = create_test_graph(BTreeMap::from([("EmptyEnum".to_string(), enum_schema)]));
  let context = create_test_context(graph.clone(), default_config());
  let converter = SchemaConverter::new(&context);
  let result = converter.convert_schema("EmptyEnum", graph.get("EmptyEnum").unwrap())?;

  assert_eq!(result.len(), 1);
  let RustType::TypeAlias(alias) = &result[0] else {
    panic!("Expected type alias for empty enum")
  };

  assert_eq!(alias.name, "EmptyEnum");
  assert_eq!(alias.target.to_rust_type(), "String");
  Ok(())
}

#[test]
fn test_case_insensitive_duplicates_with_deduplication() -> anyhow::Result<()> {
  let enum_schema = ObjectSchema {
    schema_type: Some(SchemaTypeSet::Single(SchemaType::String)),
    enum_values: vec![json!("ITEM"), json!("item"), json!("SELECT"), json!("select")],
    ..Default::default()
  };
  let graph = create_test_graph(BTreeMap::from([("CaseEnum".to_string(), enum_schema)]));
  let context = create_test_context(graph.clone(), default_config());
  let converter = SchemaConverter::new(&context);
  let result = converter.convert_schema("CaseEnum", graph.get("CaseEnum").unwrap())?;

  assert_eq!(result.len(), 1);
  let RustType::Enum(enum_def) = &result[0] else {
    panic!("Expected enum")
  };

  assert_eq!(enum_def.name.to_string(), "CaseEnum");
  assert_eq!(enum_def.variants.len(), 2);
  assert_eq!(enum_def.variants[0].name, EnumVariantToken::new("Item"));
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
  assert_eq!(enum_def.variants[1].name, EnumVariantToken::new("Select"));
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
fn test_case_insensitive_duplicates_with_preservation() -> anyhow::Result<()> {
  let enum_schema = ObjectSchema {
    schema_type: Some(SchemaTypeSet::Single(SchemaType::String)),
    enum_values: vec![json!("ITEM"), json!("item"), json!("SELECT"), json!("select")],
    ..Default::default()
  };
  let graph = create_test_graph(BTreeMap::from([("CaseEnum".to_string(), enum_schema)]));
  let context = create_test_context(graph.clone(), config_with_preserve_case());
  let converter = SchemaConverter::new(&context);
  let result = converter.convert_schema("CaseEnum", graph.get("CaseEnum").unwrap())?;

  assert_eq!(result.len(), 1);
  let RustType::Enum(enum_def) = &result[0] else {
    panic!("Expected enum")
  };

  assert_eq!(enum_def.name.to_string(), "CaseEnum");
  assert_eq!(enum_def.variants.len(), 4);
  assert_eq!(enum_def.variants[0].name, EnumVariantToken::new("Item"));
  assert!(
    enum_def.variants[0]
      .serde_attrs
      .contains(&SerdeAttribute::Rename("ITEM".to_string()))
  );
  assert_eq!(enum_def.variants[1].name, EnumVariantToken::new("Item1"));
  assert!(
    enum_def.variants[1]
      .serde_attrs
      .contains(&SerdeAttribute::Rename("item".to_string()))
  );
  assert_eq!(enum_def.variants[2].name, EnumVariantToken::new("Select"));
  assert!(
    enum_def.variants[2]
      .serde_attrs
      .contains(&SerdeAttribute::Rename("SELECT".to_string()))
  );
  assert_eq!(enum_def.variants[3].name, EnumVariantToken::new("Select3"));
  assert!(
    enum_def.variants[3]
      .serde_attrs
      .contains(&SerdeAttribute::Rename("select".to_string()))
  );
  Ok(())
}

#[test]
fn test_collision_strategy_enum() {
  let s1 = CollisionStrategy::Preserve;
  let s2 = CollisionStrategy::Deduplicate;
  assert_ne!(s1, s2);
}

#[test]
fn test_preserve_strategy_with_multiple_collisions() {
  let graph = create_test_graph(BTreeMap::default());
  let context = create_test_context(graph, config_with_preserve_case());
  let converter = EnumConverter::new(context);

  let schema = ObjectSchema {
    enum_values: vec![json!("active"), json!("Active"), json!("ACTIVE")],
    ..Default::default()
  };

  let result = converter.convert_value_enum("Status", &schema);

  if let RustType::Enum(enum_def) = result {
    assert_eq!(enum_def.variants.len(), 3);
    assert_eq!(enum_def.variants[0].name, EnumVariantToken::new("Active"));
    assert_eq!(enum_def.variants[1].name, EnumVariantToken::new("Active1"));
    assert_eq!(enum_def.variants[2].name, EnumVariantToken::new("Active2"));
  } else {
    panic!("Expected enum result");
  }
}

#[test]
fn test_relaxed_enum_detects_freeform_pattern() {
  let spec = oas3::Spec {
    openapi: "3.1.0".to_string(),
    info: Info {
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

  let mut stats = GenerationStats::default();
  let registry = SchemaRegistry::new(&spec, &mut stats);

  let graph = Arc::new(registry);
  let context = create_test_context(graph.clone(), default_config());
  let union_converter = UnionConverter::new(context);

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

  let result = union_converter.convert_union("TestEnum", &schema);
  assert!(result.is_ok());

  let output = result.unwrap();
  let types = output.into_vec();
  assert_eq!(types.len(), 2);

  let has_known_enum = types.iter().any(|t| match t {
    RustType::Enum(e) => e.name == EnumToken::new("TestEnumKnown"),
    _ => false,
  });
  let outer_enum = types.iter().find_map(|t| match t {
    RustType::Enum(e) if e.name == EnumToken::new("TestEnum") => Some(e),
    _ => None,
  });

  assert!(has_known_enum);
  assert!(outer_enum.is_some(), "should have outer wrapper enum");

  let outer_enum = outer_enum.unwrap();
  assert_eq!(outer_enum.methods.len(), 2, "wrapper enum should have 2 helper methods");
  assert!(
    outer_enum.methods.iter().any(|m| m.name.as_str() == "known1"),
    "should have known1 method"
  );
  assert!(
    outer_enum.methods.iter().any(|m| m.name.as_str() == "known2"),
    "should have known2 method"
  );
}

#[test]
fn test_relaxed_enum_rejects_no_freeform() {
  let spec = oas3::Spec {
    openapi: "3.1.0".to_string(),
    info: Info {
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

  let mut stats = GenerationStats::default();
  let registry = SchemaRegistry::new(&spec, &mut stats);

  let graph = Arc::new(registry);
  let context = create_test_context(graph.clone(), default_config());
  let union_converter = UnionConverter::new(context);

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

  let result = union_converter.convert_union("TestEnum", &schema);
  assert!(result.is_ok());
  let output = result.unwrap();
  let types = output.into_vec();
  assert!(
    !types
      .iter()
      .any(|t| matches!(t, RustType::Enum(e) if e.name == EnumToken::new("TestEnumKnown"))),
    "Should not generate relaxed enum without freeform string variant"
  );
}

#[test]
fn test_anyof_with_const_generates_unit_variant() -> anyhow::Result<()> {
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
  let context = create_test_context(graph.clone(), default_config());
  let converter = SchemaConverter::new(&context);

  let result = converter.convert_schema("ResponseFormat", &parent_schema)?;

  assert!(!result.is_empty());
  let RustType::Enum(enum_def) = &result[result.len() - 1] else {
    panic!("Expected enum as last type, got: {result:?}");
  };

  assert_eq!(enum_def.name.to_string(), "ResponseFormat");
  assert_eq!(enum_def.variants.len(), 2);

  let auto_variant = &enum_def.variants[0];
  assert_eq!(auto_variant.name, EnumVariantToken::new("Auto"));
  assert!(matches!(auto_variant.content, VariantContent::Unit));
  assert_eq!(
    auto_variant.serde_attrs,
    vec![SerdeAttribute::Rename("auto".to_string())]
  );

  let text_variant = &enum_def.variants[1];
  assert_eq!(text_variant.name, EnumVariantToken::new("TextFormat"));
  assert!(matches!(text_variant.content, VariantContent::Tuple(_)));

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
fn test_enum_helper_methods_generation() -> anyhow::Result<()> {
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

  let context = create_test_context(graph.clone(), default_config());
  let converter = SchemaConverter::new(&context);
  let result = converter.convert_schema("TestUnion", graph.get("TestUnion").unwrap())?;

  let RustType::Enum(enum_def) = result.last().unwrap() else {
    panic!("Expected enum")
  };

  assert_eq!(enum_def.methods.len(), 2);

  let simple_method = enum_def
    .methods
    .iter()
    .find(|m| m.name == "simple")
    .expect("simple method not found");
  match &simple_method.kind {
    EnumMethodKind::ParameterizedConstructor {
      variant_name,
      wrapped_type,
      param_name,
      param_type,
    } => {
      assert_eq!(variant_name, &EnumVariantToken::from("Simple"));
      assert_eq!(wrapped_type.to_rust_type(), "TestUnionSimple");
      assert_eq!(param_name, "opt_field");
      assert_eq!(param_type.to_rust_type(), "Option<String>");
    }
    _ => panic!("Expected ParameterizedConstructor for single optional field"),
  }

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
      assert_eq!(variant_name, &EnumVariantToken::from("SingleParam"));
      assert_eq!(wrapped_type.to_rust_type(), "TestUnionSingleParam");
      assert_eq!(param_name, "req_field");
      assert_eq!(param_type.to_rust_type(), "String");
    }
    _ => panic!("Expected ParameterizedConstructor"),
  }

  Ok(())
}

#[test]
fn test_enum_helper_methods_disabled_flag() -> anyhow::Result<()> {
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
  let context = create_test_context(graph.clone(), config_with_no_helpers());
  let converter = SchemaConverter::new(&context);
  let result = converter.convert_schema("TestUnion", graph.get("TestUnion").unwrap())?;

  let RustType::Enum(enum_def) = result.last().unwrap() else {
    panic!("Expected enum")
  };

  assert!(enum_def.methods.is_empty());
  Ok(())
}

#[test]
fn test_enum_helper_naming_stripping() -> anyhow::Result<()> {
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

  let context = create_test_context(graph.clone(), default_config());
  let converter = SchemaConverter::new(&context);
  let result = converter.convert_schema("ResponseFormat", graph.get("ResponseFormat").unwrap())?;

  let RustType::Enum(enum_def) = result.last().unwrap() else {
    panic!("Expected enum")
  };

  let method = enum_def.methods.first().unwrap();
  assert_eq!(method.name, "text");

  Ok(())
}

#[test]
fn test_enum_helper_method_name_collision() -> anyhow::Result<()> {
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

  let context = create_test_context(graph.clone(), default_config());
  let converter = SchemaConverter::new(&context);
  let result = converter.convert_schema("Status", graph.get("Status").unwrap())?;

  let RustType::Enum(enum_def) = result.last().unwrap() else {
    panic!("Expected enum")
  };

  assert_eq!(enum_def.methods.len(), 2);
  let names = enum_def.methods.iter().map(|m| m.name.clone()).collect::<Vec<_>>();
  assert!(names.contains(&MethodNameToken::from("active")));
  assert!(
    names.contains(&MethodNameToken::from("active2")) || names.iter().any(|n| n != &MethodNameToken::from("active"))
  );

  Ok(())
}

#[test]
fn test_enum_helper_skips_without_default_trait() {
  let enum_def = RustType::Enum(EnumDef {
    name: EnumToken::new("TestEnum"),
    variants: vec![
      VariantDef::builder()
        .name(EnumVariantToken::new("Variant"))
        .content(VariantContent::Tuple(vec![TypeRef::new(RustPrimitive::Custom(
          "TestVariant".into(),
        ))]))
        .build(),
    ],
    serde_attrs: vec![],
    outer_attrs: vec![],
    case_insensitive: false,
    methods: vec![],
    ..Default::default()
  });

  if let RustType::Enum(e) = enum_def {
    assert!(e.methods.is_empty());
  }
}

#[test]
fn test_discriminator_deduplicates_same_type_mappings() -> anyhow::Result<()> {
  let interaction_event = ObjectSchema {
    schema_type: Some(SchemaTypeSet::Single(SchemaType::Object)),
    properties: BTreeMap::from([
      (
        "type".to_string(),
        ObjectOrReference::Object(ObjectSchema {
          schema_type: Some(SchemaTypeSet::Single(SchemaType::String)),
          ..Default::default()
        }),
      ),
      (
        "data".to_string(),
        ObjectOrReference::Object(ObjectSchema {
          schema_type: Some(SchemaTypeSet::Single(SchemaType::String)),
          ..Default::default()
        }),
      ),
    ]),
    ..Default::default()
  };

  let union_schema = ObjectSchema {
    one_of: vec![ObjectOrReference::Ref {
      ref_path: "#/components/schemas/InteractionEvent".to_string(),
      summary: None,
      description: None,
    }],
    discriminator: Some(Discriminator {
      property_name: "type".to_string(),
      mapping: Some(BTreeMap::from([
        (
          "InteractionEvent".to_string(),
          "#/components/schemas/InteractionEvent".to_string(),
        ),
        (
          "interaction_event".to_string(),
          "#/components/schemas/InteractionEvent".to_string(),
        ),
      ])),
    }),
    ..Default::default()
  };

  let graph = create_test_graph(BTreeMap::from([
    ("InteractionSseEvent".to_string(), union_schema),
    ("InteractionEvent".to_string(), interaction_event),
  ]));

  let context = create_test_context(graph.clone(), default_config());
  let converter = SchemaConverter::new(&context);
  let result = converter.convert_schema("InteractionSseEvent", graph.get("InteractionSseEvent").unwrap())?;

  let RustType::DiscriminatedEnum(enum_def) = result.last().unwrap() else {
    panic!("Expected DiscriminatedEnum as last type")
  };

  assert_eq!(enum_def.name.to_string(), "InteractionSseEvent");
  assert_eq!(enum_def.discriminator_field, "type");

  assert_eq!(
    enum_def.variants.len(),
    1,
    "Expected 1 variant but got {}: {:?}",
    enum_def.variants.len(),
    enum_def.variants.iter().map(|v| &v.variant_name).collect::<Vec<_>>()
  );

  assert_eq!(enum_def.variants[0].type_name.base_type.to_string(), "InteractionEvent");

  Ok(())
}

#[test]
fn test_union_with_hyphenated_raw_name_converts_correctly() {
  let spec = oas3::Spec {
    openapi: "3.1.0".to_string(),
    info: Info {
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

  let mut stats = GenerationStats::default();
  let registry = SchemaRegistry::new(&spec, &mut stats);

  let graph = Arc::new(registry);
  let context = create_test_context(graph.clone(), default_config());
  let union_converter = UnionConverter::new(context);

  let schema = ObjectSchema {
    one_of: vec![
      ObjectOrReference::Object(ObjectSchema {
        schema_type: Some(SchemaTypeSet::Single(SchemaType::String)),
        const_value: Some(json!("option-a")),
        ..Default::default()
      }),
      ObjectOrReference::Object(ObjectSchema {
        schema_type: Some(SchemaTypeSet::Single(SchemaType::String)),
        const_value: Some(json!("option-b")),
        ..Default::default()
      }),
    ],
    ..Default::default()
  };

  let result = union_converter.convert_union("my-test-enum", &schema);
  assert!(result.is_ok());

  let output = result.unwrap();
  let types = output.into_vec();

  let enum_def = types.iter().find_map(|t| match t {
    RustType::Enum(e) => Some(e),
    _ => None,
  });
  assert!(enum_def.is_some(), "should have an enum");

  let enum_def = enum_def.unwrap();
  assert_eq!(
    enum_def.name.to_string(),
    "MyTestEnum",
    "raw hyphenated name should convert to PascalCase"
  );
}

#[test]
fn test_union_with_underscored_raw_name_converts_correctly() {
  let spec = oas3::Spec {
    openapi: "3.1.0".to_string(),
    info: Info {
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

  let mut stats = GenerationStats::default();
  let registry = SchemaRegistry::new(&spec, &mut stats);

  let graph = Arc::new(registry);
  let context = create_test_context(graph.clone(), default_config());
  let union_converter = UnionConverter::new(context);

  let schema = ObjectSchema {
    one_of: vec![
      ObjectOrReference::Object(ObjectSchema {
        schema_type: Some(SchemaTypeSet::Single(SchemaType::String)),
        const_value: Some(json!("option-a")),
        ..Default::default()
      }),
      ObjectOrReference::Object(ObjectSchema {
        schema_type: Some(SchemaTypeSet::Single(SchemaType::String)),
        const_value: Some(json!("option-b")),
        ..Default::default()
      }),
    ],
    ..Default::default()
  };

  let result = union_converter.convert_union("my_test_enum", &schema);
  assert!(result.is_ok());

  let output = result.unwrap();
  let types = output.into_vec();

  let enum_def = types.iter().find_map(|t| match t {
    RustType::Enum(e) => Some(e),
    _ => None,
  });
  assert!(enum_def.is_some(), "should have an enum");

  let enum_def = enum_def.unwrap();
  assert_eq!(
    enum_def.name.to_string(),
    "MyTestEnum",
    "raw underscored name should convert to PascalCase"
  );
}

#[test]
fn test_union_with_inline_struct_and_raw_name() -> anyhow::Result<()> {
  let inline_object_variant = ObjectSchema {
    schema_type: Some(SchemaTypeSet::Single(SchemaType::Object)),
    properties: BTreeMap::from([(
      "data_field".to_string(),
      ObjectOrReference::Object(ObjectSchema {
        schema_type: Some(SchemaTypeSet::Single(SchemaType::String)),
        ..Default::default()
      }),
    )]),
    ..Default::default()
  };

  let union_schema = ObjectSchema {
    one_of: vec![ObjectOrReference::Object(inline_object_variant)],
    ..Default::default()
  };

  let graph = create_test_graph(BTreeMap::from([("my-union-type".to_string(), union_schema)]));
  let context = create_test_context(graph.clone(), default_config());
  let converter = SchemaConverter::new(&context);
  let result = converter.convert_schema("my-union-type", graph.get("my-union-type").unwrap())?;

  let binding = context.cache.borrow();
  let generated = &binding.types.types;
  let all_types = result.iter().chain(generated.iter()).collect::<Vec<&RustType>>();

  let enum_def = all_types.iter().find_map(|t| match t {
    RustType::Enum(e) => Some(e),
    _ => None,
  });
  assert!(enum_def.is_some(), "should produce an enum");
  assert_eq!(enum_def.unwrap().name.to_string(), "MyUnionType");

  let struct_def = all_types.iter().find_map(|t| match t {
    RustType::Struct(s) => Some(s),
    _ => None,
  });
  assert!(struct_def.is_some(), "should produce inline struct");

  let struct_name = struct_def.unwrap().name.to_string();
  assert!(
    struct_name.starts_with("MyUnionType"),
    "inline struct name '{struct_name}' should start with converted enum name 'MyUnionType'"
  );
  assert!(
    !struct_name.contains('-'),
    "struct name '{struct_name}' should not contain hyphens"
  );

  Ok(())
}

#[test]
fn test_already_pascalcase_name_not_double_converted() {
  let spec = oas3::Spec {
    openapi: "3.1.0".to_string(),
    info: Info {
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

  let mut stats = GenerationStats::default();
  let registry = SchemaRegistry::new(&spec, &mut stats);

  let graph = Arc::new(registry);
  let context = create_test_context(graph.clone(), default_config());
  let union_converter = UnionConverter::new(context);

  let schema = ObjectSchema {
    one_of: vec![
      ObjectOrReference::Object(ObjectSchema {
        schema_type: Some(SchemaTypeSet::Single(SchemaType::String)),
        const_value: Some(json!("OptionA")),
        ..Default::default()
      }),
      ObjectOrReference::Object(ObjectSchema {
        schema_type: Some(SchemaTypeSet::Single(SchemaType::String)),
        const_value: Some(json!("OptionB")),
        ..Default::default()
      }),
    ],
    ..Default::default()
  };

  let result = union_converter.convert_union("MyPascalCaseEnum", &schema);
  assert!(result.is_ok());

  let output = result.unwrap();
  let types = output.into_vec();

  let enum_def = types.iter().find_map(|t| match t {
    RustType::Enum(e) => Some(e),
    _ => None,
  });
  assert!(enum_def.is_some());

  let enum_def = enum_def.unwrap();
  assert_eq!(
    enum_def.name.to_string(),
    "MyPascalCaseEnum",
    "already valid PascalCase name should remain unchanged"
  );
}

#[test]
fn test_relaxed_enum_with_raw_name() {
  let spec = oas3::Spec {
    openapi: "3.1.0".to_string(),
    info: Info {
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

  let mut stats = GenerationStats::default();
  let registry = SchemaRegistry::new(&spec, &mut stats);

  let graph = Arc::new(registry);
  let context = create_test_context(graph.clone(), default_config());
  let union_converter = UnionConverter::new(context);

  let schema = ObjectSchema {
    any_of: vec![
      ObjectOrReference::Object(ObjectSchema {
        schema_type: Some(SchemaTypeSet::Single(SchemaType::String)),
        const_value: Some(json!("known-value")),
        ..Default::default()
      }),
      ObjectOrReference::Object(ObjectSchema {
        schema_type: Some(SchemaTypeSet::Single(SchemaType::String)),
        ..Default::default()
      }),
    ],
    ..Default::default()
  };

  let result = union_converter.convert_union("my-relaxed-enum", &schema);
  assert!(result.is_ok());

  let output = result.unwrap();
  let types = output.into_vec();

  let outer_enum = types.iter().find_map(|t| match t {
    RustType::Enum(e) if !e.name.to_string().ends_with(KNOWN_ENUM_VARIANT) => Some(e),
    _ => None,
  });
  assert!(outer_enum.is_some(), "should have outer wrapper enum");
  assert_eq!(
    outer_enum.unwrap().name.to_string(),
    "MyRelaxedEnum",
    "raw hyphenated name should convert to PascalCase"
  );

  let known_enum = types.iter().find_map(|t| match t {
    RustType::Enum(e) if e.name.to_string().ends_with(KNOWN_ENUM_VARIANT) => Some(e),
    _ => None,
  });
  assert!(known_enum.is_some(), "should have known values enum");
  assert_eq!(
    known_enum.unwrap().name.to_string(),
    "MyRelaxedEnumKnown",
    "known enum should have converted name + Known suffix"
  );
}

#[test]
fn test_nested_anyof_with_null_flattens_to_single_enum() -> anyhow::Result<()> {
  let custom_config_schema = ObjectSchema {
    schema_type: Some(SchemaTypeSet::Single(SchemaType::Object)),
    properties: BTreeMap::from([(
      "mode".to_string(),
      ObjectOrReference::Object(ObjectSchema {
        schema_type: Some(SchemaTypeSet::Single(SchemaType::String)),
        ..Default::default()
      }),
    )]),
    ..Default::default()
  };

  let outer_schema = ObjectSchema {
    any_of: vec![
      ObjectOrReference::Object(ObjectSchema {
        description: Some("The cooking method configuration".to_string()),
        any_of: vec![
          ObjectOrReference::Object(ObjectSchema {
            schema_type: Some(SchemaTypeSet::Single(SchemaType::String)),
            const_value: Some(json!("auto")),
            ..Default::default()
          }),
          ObjectOrReference::Ref {
            ref_path: "#/components/schemas/CustomCookingConfig".to_string(),
            description: None,
            summary: None,
          },
        ],
        ..Default::default()
      }),
      ObjectOrReference::Object(ObjectSchema {
        schema_type: Some(SchemaTypeSet::Single(SchemaType::Null)),
        ..Default::default()
      }),
    ],
    ..Default::default()
  };

  let schemas = BTreeMap::from([
    ("CookingStrategy".to_string(), outer_schema.clone()),
    ("CustomCookingConfig".to_string(), custom_config_schema),
  ]);
  let graph = create_test_graph(schemas);
  let context = create_test_context(graph.clone(), default_config());
  let converter = SchemaConverter::new(&context);

  let result = converter.convert_schema("CookingStrategy", &outer_schema)?;

  let enum_def = result.iter().find_map(|t| match t {
    RustType::Enum(e) if e.name == "CookingStrategy" => Some(e),
    _ => None,
  });

  assert!(enum_def.is_some(), "should produce CookingStrategy enum");
  let enum_def = enum_def.unwrap();

  assert_eq!(
    enum_def.variants.len(),
    2,
    "should have 2 variants (Auto and CustomCookingConfig), not a single Variant0 wrapper"
  );

  let variant_names = enum_def
    .variants
    .iter()
    .map(|v| v.name.to_string())
    .collect::<Vec<String>>();
  assert!(
    variant_names.contains(&"Auto".to_string()),
    "should have Auto variant, got {variant_names:?}"
  );
  assert!(
    variant_names.contains(&"CustomCookingConfig".to_string()),
    "should have CustomCookingConfig variant, got {variant_names:?}"
  );

  assert!(
    !variant_names.iter().any(|n| n.starts_with("Variant")),
    "should NOT have Variant0-style wrapper variants, got {variant_names:?}"
  );

  Ok(())
}

#[test]
fn test_nested_nullable_relaxed_anyof_produces_known_other_enum() -> anyhow::Result<()> {
  let outer_schema = ObjectSchema {
    any_of: vec![
      ObjectOrReference::Object(ObjectSchema {
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
      }),
      ObjectOrReference::Object(ObjectSchema {
        schema_type: Some(SchemaTypeSet::Single(SchemaType::Null)),
        ..Default::default()
      }),
    ],
    ..Default::default()
  };

  let schemas = BTreeMap::from([("NullableRelaxed".to_string(), outer_schema.clone())]);
  let graph = create_test_graph(schemas);
  let context = create_test_context(graph.clone(), default_config());
  let converter = SchemaConverter::new(&context);

  let result = converter.convert_schema("NullableRelaxed", &outer_schema)?;

  let outer_enum = result.iter().find_map(|t| match t {
    RustType::Enum(e) if e.name == "NullableRelaxed" => Some(e),
    _ => None,
  });
  assert!(outer_enum.is_some(), "should produce NullableRelaxed enum");
  let outer_enum = outer_enum.unwrap();

  let variant_names = outer_enum
    .variants
    .iter()
    .map(|v| v.name.to_string())
    .collect::<Vec<_>>();
  assert!(
    variant_names.contains(&"Known".to_string()),
    "should have Known variant, got {variant_names:?}"
  );
  assert!(
    variant_names.contains(&"Other".to_string()),
    "should have Other variant, got {variant_names:?}"
  );

  let has_known_enum = result.iter().any(|t| match t {
    RustType::Enum(e) => e.name.to_string().ends_with("Known") && e.name != "NullableRelaxed",
    _ => false,
  });
  assert!(has_known_enum, "should generate inner known values enum");

  Ok(())
}

#[test]
fn test_nested_oneof_with_null_flattens_to_single_enum() -> anyhow::Result<()> {
  let option_a_schema = ObjectSchema {
    schema_type: Some(SchemaTypeSet::Single(SchemaType::Object)),
    properties: BTreeMap::from([(
      "value".to_string(),
      ObjectOrReference::Object(ObjectSchema {
        schema_type: Some(SchemaTypeSet::Single(SchemaType::String)),
        ..Default::default()
      }),
    )]),
    ..Default::default()
  };

  let outer_schema = ObjectSchema {
    any_of: vec![
      ObjectOrReference::Object(ObjectSchema {
        one_of: vec![
          ObjectOrReference::Object(ObjectSchema {
            schema_type: Some(SchemaTypeSet::Single(SchemaType::String)),
            const_value: Some(json!("default")),
            ..Default::default()
          }),
          ObjectOrReference::Ref {
            ref_path: "#/components/schemas/OptionA".to_string(),
            description: None,
            summary: None,
          },
        ],
        ..Default::default()
      }),
      ObjectOrReference::Object(ObjectSchema {
        schema_type: Some(SchemaTypeSet::Single(SchemaType::Null)),
        ..Default::default()
      }),
    ],
    ..Default::default()
  };

  let schemas = BTreeMap::from([
    ("NestedUnion".to_string(), outer_schema.clone()),
    ("OptionA".to_string(), option_a_schema),
  ]);
  let graph = create_test_graph(schemas);
  let context = create_test_context(graph.clone(), default_config());
  let converter = SchemaConverter::new(&context);

  let result = converter.convert_schema("NestedUnion", &outer_schema)?;

  let enum_def = result.iter().find_map(|t| match t {
    RustType::Enum(e) if e.name == "NestedUnion" => Some(e),
    _ => None,
  });

  assert!(enum_def.is_some(), "should produce NestedUnion enum");
  let enum_def = enum_def.unwrap();

  assert_eq!(
    enum_def.variants.len(),
    2,
    "should have 2 variants (Default and OptionA), not a single Variant0 wrapper"
  );

  let variant_names = enum_def
    .variants
    .iter()
    .map(|v| v.name.to_string())
    .collect::<Vec<String>>();
  assert!(
    variant_names.contains(&"Default".to_string()),
    "should have Default variant, got {variant_names:?}"
  );
  assert!(
    variant_names.contains(&"OptionA".to_string()),
    "should have OptionA variant, got {variant_names:?}"
  );

  Ok(())
}

#[test]
fn test_anyof_with_nullable_map_type_generates_enum() -> anyhow::Result<()> {
  let map_schema = ObjectSchema {
    any_of: vec![
      ObjectOrReference::Object(ObjectSchema {
        schema_type: Some(SchemaTypeSet::Single(SchemaType::Object)),
        title: Some("Prompt Variables".to_string()),
        description: Some("Optional map of values to substitute".to_string()),
        additional_properties: Some(Schema::Object(Box::new(ObjectOrReference::Object(ObjectSchema {
          any_of: vec![
            ObjectOrReference::Object(ObjectSchema {
              schema_type: Some(SchemaTypeSet::Single(SchemaType::String)),
              ..Default::default()
            }),
            ObjectOrReference::Ref {
              ref_path: "#/components/schemas/InputContent".to_string(),
              description: None,
              summary: None,
            },
          ],
          ..Default::default()
        })))),
        ..Default::default()
      }),
      ObjectOrReference::Object(ObjectSchema {
        schema_type: Some(SchemaTypeSet::Single(SchemaType::Null)),
        ..Default::default()
      }),
    ],
    ..Default::default()
  };

  let input_content = ObjectSchema {
    schema_type: Some(SchemaTypeSet::Single(SchemaType::Object)),
    properties: BTreeMap::from([(
      "type".to_string(),
      ObjectOrReference::Object(ObjectSchema {
        schema_type: Some(SchemaTypeSet::Single(SchemaType::String)),
        ..Default::default()
      }),
    )]),
    ..Default::default()
  };

  let schemas = BTreeMap::from([
    ("ResponsePromptVariables".to_string(), map_schema.clone()),
    ("InputContent".to_string(), input_content),
  ]);
  let graph = create_test_graph(schemas);
  let context = create_test_context(graph.clone(), default_config());
  let converter = SchemaConverter::new(&context);

  let result = converter.convert_schema("ResponsePromptVariables", &map_schema)?;

  assert!(
    !result.is_empty(),
    "should generate types for anyOf with nullable map, not skip as single-variant wrapper"
  );

  let enum_def = result.iter().find_map(|t| match t {
    RustType::Enum(e) if e.name == "ResponsePromptVariables" => Some(e),
    _ => None,
  });

  assert!(
    enum_def.is_some(),
    "should generate ResponsePromptVariables enum, got types: {:?}",
    result.iter().map(|t| t.type_name().to_string()).collect::<Vec<_>>()
  );

  let enum_def = enum_def.unwrap();
  assert_eq!(
    enum_def.variants.len(),
    1,
    "should have 1 variant (the map type, with null filtered out)"
  );

  let variant = &enum_def.variants[0];
  assert!(
    matches!(&variant.content, VariantContent::Tuple(types) if !types.is_empty()),
    "variant should have tuple content with the map type"
  );

  Ok(())
}

#[test]
fn test_anyof_with_string_enum_and_object_generates_inline_enum() -> anyhow::Result<()> {
  let function_call_option = ObjectSchema {
    schema_type: Some(SchemaTypeSet::Single(SchemaType::Object)),
    properties: BTreeMap::from([(
      "name".to_string(),
      ObjectOrReference::Object(ObjectSchema {
        schema_type: Some(SchemaTypeSet::Single(SchemaType::String)),
        description: Some("The name of the function to call.".to_string()),
        ..Default::default()
      }),
    )]),
    required: vec!["name".to_string()],
    ..Default::default()
  };

  let union_schema = ObjectSchema {
    description: Some("Controls which function is called by the model.".to_string()),
    any_of: vec![
      ObjectOrReference::Object(ObjectSchema {
        schema_type: Some(SchemaTypeSet::Single(SchemaType::String)),
        description: Some("`none` or `auto` mode".to_string()),
        enum_values: vec![json!("none"), json!("auto")],
        title: Some("FunctionCallMode".to_string()),
        ..Default::default()
      }),
      ObjectOrReference::Ref {
        ref_path: "#/components/schemas/FunctionCallOption".to_string(),
        description: None,
        summary: None,
      },
    ],
    ..Default::default()
  };

  let schemas = BTreeMap::from([
    ("FunctionCall".to_string(), union_schema.clone()),
    ("FunctionCallOption".to_string(), function_call_option),
  ]);
  let graph = create_test_graph(schemas);
  let context = create_test_context(graph.clone(), default_config());
  let converter = SchemaConverter::new(&context);

  let result = converter.convert_schema("FunctionCall", &union_schema)?;

  let binding = context.cache.borrow();
  let cached_types = &binding.types.types;
  let all_types = result.iter().chain(cached_types.iter()).collect::<Vec<_>>();

  let enum_def = all_types.iter().find_map(|t| match t {
    RustType::Enum(e) if e.name == "FunctionCall" => Some(e),
    _ => None,
  });

  assert!(enum_def.is_some(), "should produce FunctionCall enum");
  let enum_def = enum_def.unwrap();

  assert_eq!(enum_def.variants.len(), 2, "should have 2 variants");

  let mode_variant = enum_def
    .variants
    .iter()
    .find(|v| v.name != "Option")
    .expect("should have a mode variant");

  match &mode_variant.content {
    VariantContent::Tuple(types) => {
      assert_eq!(types.len(), 1);
      let inner_type = types[0].to_rust_type();
      assert_ne!(
        inner_type, "String",
        "variant should NOT be plain String, should be an inline enum"
      );
    }
    VariantContent::Unit => panic!("expected tuple variant for mode variant"),
  }

  let inline_enum = all_types.iter().find_map(|t| match t {
    RustType::Enum(e)
      if e.name != "FunctionCall" && e.variants.iter().any(|v| v.name == "None" || v.name == "Auto") =>
    {
      Some(e)
    }
    _ => None,
  });

  assert!(
    inline_enum.is_some(),
    "should generate inline enum for string enum variant with None/Auto variants, types: {:?}",
    all_types.iter().map(|t| t.type_name().to_string()).collect::<Vec<_>>()
  );

  let inline_enum = inline_enum.unwrap();
  assert_eq!(inline_enum.variants.len(), 2);

  let variant_names = inline_enum
    .variants
    .iter()
    .map(|v| v.name.to_string())
    .collect::<Vec<_>>();
  assert!(variant_names.contains(&"None".to_string()), "should have None variant");
  assert!(variant_names.contains(&"Auto".to_string()), "should have Auto variant");

  Ok(())
}

#[test]
fn test_oneof_with_string_enum_variant_generates_inline_enum() -> anyhow::Result<()> {
  let text_format = ObjectSchema {
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

  let union_schema = ObjectSchema {
    one_of: vec![
      ObjectOrReference::Object(ObjectSchema {
        schema_type: Some(SchemaTypeSet::Single(SchemaType::String)),
        title: Some("ResponseMode".to_string()),
        enum_values: vec![json!("streaming"), json!("batch")],
        ..Default::default()
      }),
      ObjectOrReference::Ref {
        ref_path: "#/components/schemas/TextFormat".to_string(),
        description: None,
        summary: None,
      },
    ],
    ..Default::default()
  };

  let schemas = BTreeMap::from([
    ("ResponseFormat".to_string(), union_schema.clone()),
    ("TextFormat".to_string(), text_format),
  ]);
  let graph = create_test_graph(schemas);
  let context = create_test_context(graph.clone(), default_config());
  let converter = SchemaConverter::new(&context);

  let result = converter.convert_schema("ResponseFormat", &union_schema)?;

  let binding = context.cache.borrow();
  let cached_types = &binding.types.types;
  let all_types = result.iter().chain(cached_types.iter()).collect::<Vec<_>>();

  let enum_def = all_types.iter().find_map(|t| match t {
    RustType::Enum(e) if e.name == "ResponseFormat" => Some(e),
    _ => None,
  });

  assert!(enum_def.is_some(), "should produce ResponseFormat enum");
  let enum_def = enum_def.unwrap();

  let mode_variant = enum_def
    .variants
    .iter()
    .find(|v| v.name != "TextFormat")
    .expect("should have a mode variant");

  match &mode_variant.content {
    VariantContent::Tuple(types) => {
      assert_eq!(types.len(), 1);
      let inner_type = types[0].to_rust_type();
      assert_ne!(
        inner_type, "String",
        "variant should NOT be plain String, should be an inline enum"
      );
    }
    VariantContent::Unit => panic!("expected tuple variant for mode variant"),
  }

  let inline_enum = all_types.iter().find_map(|t| match t {
    RustType::Enum(e)
      if e.name != "ResponseFormat" && e.variants.iter().any(|v| v.name == "Streaming" || v.name == "Batch") =>
    {
      Some(e)
    }
    _ => None,
  });

  assert!(
    inline_enum.is_some(),
    "should generate inline enum with Streaming/Batch variants, types: {:?}",
    all_types.iter().map(|t| t.type_name().to_string()).collect::<Vec<_>>()
  );

  let inline_enum = inline_enum.unwrap();
  let variant_names = inline_enum
    .variants
    .iter()
    .map(|v| v.name.to_string())
    .collect::<Vec<_>>();
  assert!(variant_names.contains(&"Streaming".to_string()));
  assert!(variant_names.contains(&"Batch".to_string()));

  Ok(())
}

#[test]
fn test_anyof_with_single_value_enum_uses_primitive() -> anyhow::Result<()> {
  let object_variant = ObjectSchema {
    schema_type: Some(SchemaTypeSet::Single(SchemaType::Object)),
    properties: BTreeMap::from([(
      "name".to_string(),
      ObjectOrReference::Object(ObjectSchema {
        schema_type: Some(SchemaTypeSet::Single(SchemaType::String)),
        ..Default::default()
      }),
    )]),
    ..Default::default()
  };

  let union_schema = ObjectSchema {
    any_of: vec![
      ObjectOrReference::Object(ObjectSchema {
        schema_type: Some(SchemaTypeSet::Single(SchemaType::String)),
        enum_values: vec![json!("auto")],
        ..Default::default()
      }),
      ObjectOrReference::Ref {
        ref_path: "#/components/schemas/ObjectVariant".to_string(),
        description: None,
        summary: None,
      },
    ],
    ..Default::default()
  };

  let schemas = BTreeMap::from([
    ("TestUnion".to_string(), union_schema.clone()),
    ("ObjectVariant".to_string(), object_variant),
  ]);
  let graph = create_test_graph(schemas);
  let context = create_test_context(graph.clone(), default_config());
  let converter = SchemaConverter::new(&context);

  let result = converter.convert_schema("TestUnion", &union_schema)?;

  let enum_def = result.iter().find_map(|t| match t {
    RustType::Enum(e) if e.name == "TestUnion" => Some(e),
    _ => None,
  });

  assert!(enum_def.is_some(), "should produce TestUnion enum");
  let enum_def = enum_def.unwrap();

  let string_variant = enum_def
    .variants
    .iter()
    .find(|v| v.name != "ObjectVariant")
    .expect("should have a non-ObjectVariant variant");

  match &string_variant.content {
    VariantContent::Tuple(types) => {
      assert_eq!(types.len(), 1);
      let inner_type = types[0].to_rust_type();
      assert_eq!(
        inner_type, "String",
        "single-value enum should use primitive String type"
      );
    }
    VariantContent::Unit => panic!("expected tuple variant"),
  }

  Ok(())
}

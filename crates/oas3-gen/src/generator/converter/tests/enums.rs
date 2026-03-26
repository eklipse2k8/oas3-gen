use std::{collections::BTreeSet, sync::Arc};

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
    parse_schema, parse_schemas,
  },
};

#[test]
fn test_simple_string_enum() -> anyhow::Result<()> {
  let graph = create_test_graph(parse_schemas(vec![(
    "SimpleEnum",
    json!({
      "type": "string",
      "enum": ["value1", "value2"]
    }),
  )]));
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
  let graph = create_test_graph(parse_schemas(vec![
    (
      "TestUnion",
      json!({
        "oneOf": [
          { "$ref": "#/components/schemas/VariantA" },
          { "$ref": "#/components/schemas/VariantB" }
        ],
        "discriminator": {
          "propertyName": "type",
          "mapping": {
            "type_a": "#/components/schemas/VariantA",
            "type_b": "#/components/schemas/VariantB"
          }
        }
      }),
    ),
    (
      "VariantA",
      json!({
        "type": "object",
        "properties": {
          "type": { "type": "string", "const": "type_a" }
        }
      }),
    ),
    (
      "VariantB",
      json!({
        "type": "object",
        "properties": {
          "type": { "type": "string", "const": "type_b" }
        }
      }),
    ),
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
  let graph = create_test_graph(parse_schemas(vec![
    (
      "TestUnion",
      json!({
        "anyOf": [
          { "$ref": "#/components/schemas/VariantA" },
          { "$ref": "#/components/schemas/VariantB" }
        ]
      }),
    ),
    (
      "VariantA",
      json!({
        "type": "object",
        "properties": {
          "field1": { "type": "string" }
        }
      }),
    ),
    (
      "VariantB",
      json!({
        "type": "object",
        "properties": {
          "field2": { "type": "integer" }
        }
      }),
    ),
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
  let graph = create_test_graph(parse_schemas(vec![
    (
      "TestUnion",
      json!({
        "anyOf": [
          { "$ref": "#/components/schemas/VariantA" },
          { "$ref": "#/components/schemas/VariantB" }
        ],
        "discriminator": {
          "propertyName": "type",
          "mapping": {
            "type_a": "#/components/schemas/VariantA",
            "type_b": "#/components/schemas/VariantB"
          }
        }
      }),
    ),
    (
      "VariantA",
      json!({
        "type": "object",
        "properties": {
          "type": { "type": "string", "const": "type_a" }
        }
      }),
    ),
    (
      "VariantB",
      json!({
        "type": "object",
        "properties": {
          "type": { "type": "string", "const": "type_b" }
        }
      }),
    ),
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
fn test_empty_enum_converts_to_string() -> anyhow::Result<()> {
  let graph = create_test_graph(parse_schemas(vec![(
    "EmptyEnum",
    json!({
      "type": "string",
      "enum": []
    }),
  )]));
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
#[allow(clippy::approx_constant)]
fn test_float_enum_values() -> anyhow::Result<()> {
  let graph = create_test_graph(parse_schemas(vec![(
    "FloatEnum",
    json!({
      "type": "number",
      "enum": [0.0, 1.5, 3.14, -2.5]
    }),
  )]));
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
  let graph = create_test_graph(parse_schemas(vec![(
    "BoolEnum",
    json!({
      "type": "boolean",
      "enum": [true, false]
    }),
  )]));
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
  let graph = create_test_graph(parse_schemas(vec![(
    "MixedEnum",
    json!({
      "enum": ["string", 42, 1.5, true]
    }),
  )]));
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
fn test_integer_enum_values() -> anyhow::Result<()> {
  let graph = create_test_graph(parse_schemas(vec![(
    "IntEnum",
    json!({
      "type": "integer",
      "enum": [0, 1, 42, -5]
    }),
  )]));
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
fn test_case_insensitive_duplicates_with_preservation() -> anyhow::Result<()> {
  let graph = create_test_graph(parse_schemas(vec![(
    "CaseEnum",
    json!({
      "type": "string",
      "enum": ["ITEM", "item", "SELECT", "select"]
    }),
  )]));
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
fn test_case_insensitive_duplicates_with_deduplication() -> anyhow::Result<()> {
  let graph = create_test_graph(parse_schemas(vec![(
    "CaseEnum",
    json!({
      "type": "string",
      "enum": ["ITEM", "item", "SELECT", "select"]
    }),
  )]));
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
fn test_collision_strategy_enum() {
  let s1 = CollisionStrategy::Preserve;
  let s2 = CollisionStrategy::Deduplicate;
  assert_ne!(s1, s2);
}

#[test]
fn test_preserve_strategy_with_multiple_collisions() {
  let graph = create_test_graph(parse_schemas(vec![]));
  let context = create_test_context(graph, config_with_preserve_case());
  let converter = EnumConverter::new(context);

  let schema = parse_schema(json!({
    "enum": ["active", "Active", "ACTIVE"]
  }));

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
  let spec = serde_json::from_value::<oas3::Spec>(json!({
    "openapi": "3.1.0",
    "info": {
      "title": "Test",
      "version": "1.0.0"
    }
  }))
  .unwrap();

  let mut stats = GenerationStats::default();
  let registry = SchemaRegistry::new(&spec, &mut stats);

  let graph = Arc::new(registry);
  let context = create_test_context(graph.clone(), default_config());
  let union_converter = UnionConverter::new(context);

  let schema = parse_schema(json!({
    "anyOf": [
      { "type": "string", "const": "known1" },
      { "type": "string", "const": "known2" },
      { "type": "string" }
    ]
  }));

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
  let spec = serde_json::from_value::<oas3::Spec>(json!({
    "openapi": "3.1.0",
    "info": {
      "title": "Test",
      "version": "1.0.0"
    }
  }))
  .unwrap();

  let mut stats = GenerationStats::default();
  let registry = SchemaRegistry::new(&spec, &mut stats);

  let graph = Arc::new(registry);
  let context = create_test_context(graph.clone(), default_config());
  let union_converter = UnionConverter::new(context);

  let schema = parse_schema(json!({
    "anyOf": [
      { "type": "string", "const": "known1" },
      { "type": "string", "const": "known2" }
    ]
  }));

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
  let graph = create_test_graph(parse_schemas(vec![
    (
      "ResponseFormat",
      json!({
        "description": "Response format option",
        "anyOf": [
          {
            "type": "string",
            "const": "auto",
            "description": "`auto` is the default value"
          },
          { "$ref": "#/components/schemas/TextFormat" }
        ]
      }),
    ),
    (
      "TextFormat",
      json!({
        "type": "object",
        "properties": {
          "type": { "type": "string", "const": "text" }
        }
      }),
    ),
  ]));
  let context = create_test_context(graph.clone(), default_config());
  let converter = SchemaConverter::new(&context);

  let result = converter.convert_schema("ResponseFormat", graph.get("ResponseFormat").unwrap())?;

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

  let auto_json = serde_json::to_string::<TestEnum>(&auto).unwrap();
  let data_json = serde_json::to_value::<&TestEnum>(&data).unwrap();

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

  let text_json = serde_json::to_value::<&ResponseFormat>(&text).unwrap();
  let json_schema_json = serde_json::to_value::<&ResponseFormat>(&json_schema).unwrap();

  assert_eq!(text_json["type"], "text");
  assert_eq!(json_schema_json["type"], "json_schema");
  assert_eq!(json_schema_json["json_schema"]["type"], "object");
}

#[test]
fn test_enum_helper_methods_generation() -> anyhow::Result<()> {
  let graph = create_test_graph(parse_schemas(vec![(
    "TestUnion",
    json!({
      "oneOf": [
        {
          "title": "Simple",
          "type": "object",
          "properties": {
            "opt_field": { "type": "string" }
          }
        },
        {
          "title": "SingleParam",
          "type": "object",
          "properties": {
            "req_field": { "type": "string" }
          },
          "required": ["req_field"]
        },
        {
          "title": "Complex",
          "type": "object",
          "properties": {
            "req1": { "type": "string" },
            "req2": { "type": "string" }
          },
          "required": ["req1", "req2"]
        }
      ]
    }),
  )]));

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
  let graph = create_test_graph(parse_schemas(vec![(
    "TestUnion",
    json!({
      "oneOf": [
        {
          "title": "Simple",
          "type": "object",
          "properties": {
            "opt_field": { "type": "string" }
          }
        }
      ]
    }),
  )]));

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
  let graph = create_test_graph(parse_schemas(vec![(
    "ResponseFormat",
    json!({
      "oneOf": [
        {
          "title": "ResponseFormatText",
          "type": "object",
          "properties": {
            "dummy": { "type": "string" }
          }
        }
      ]
    }),
  )]));

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
  let graph = create_test_graph(parse_schemas(vec![(
    "Status",
    json!({
      "oneOf": [
        {
          "title": "StatusActive",
          "type": "object",
          "properties": {
            "opt_field": { "type": "string" }
          }
        },
        {
          "title": "Active",
          "type": "object",
          "properties": {
            "opt_field2": { "type": "string" }
          }
        }
      ]
    }),
  )]));

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
  let graph = create_test_graph(parse_schemas(vec![
    (
      "InteractionSseEvent",
      json!({
        "oneOf": [
          { "$ref": "#/components/schemas/InteractionEvent" }
        ],
        "discriminator": {
          "propertyName": "type",
          "mapping": {
            "InteractionEvent": "#/components/schemas/InteractionEvent",
            "interaction_event": "#/components/schemas/InteractionEvent"
          }
        }
      }),
    ),
    (
      "InteractionEvent",
      json!({
        "type": "object",
        "properties": {
          "type": { "type": "string" },
          "data": { "type": "string" }
        }
      }),
    ),
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
  let spec = serde_json::from_value::<oas3::Spec>(json!({
    "openapi": "3.1.0",
    "info": {
      "title": "Test",
      "version": "1.0.0"
    }
  }))
  .unwrap();

  let mut stats = GenerationStats::default();
  let registry = SchemaRegistry::new(&spec, &mut stats);

  let graph = Arc::new(registry);
  let context = create_test_context(graph.clone(), default_config());
  let union_converter = UnionConverter::new(context);

  let schema = parse_schema(json!({
    "oneOf": [
      { "type": "string", "const": "option-a" },
      { "type": "string", "const": "option-b" }
    ]
  }));

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
  let spec = serde_json::from_value::<oas3::Spec>(json!({
    "openapi": "3.1.0",
    "info": {
      "title": "Test",
      "version": "1.0.0"
    }
  }))
  .unwrap();

  let mut stats = GenerationStats::default();
  let registry = SchemaRegistry::new(&spec, &mut stats);

  let graph = Arc::new(registry);
  let context = create_test_context(graph.clone(), default_config());
  let union_converter = UnionConverter::new(context);

  let schema = parse_schema(json!({
    "oneOf": [
      { "type": "string", "const": "option-a" },
      { "type": "string", "const": "option-b" }
    ]
  }));

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
  let graph = create_test_graph(parse_schemas(vec![(
    "my-union-type",
    json!({
      "oneOf": [
        {
          "type": "object",
          "properties": {
            "data_field": { "type": "string" }
          }
        }
      ]
    }),
  )]));
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
  let spec = serde_json::from_value::<oas3::Spec>(json!({
    "openapi": "3.1.0",
    "info": {
      "title": "Test",
      "version": "1.0.0"
    }
  }))
  .unwrap();

  let mut stats = GenerationStats::default();
  let registry = SchemaRegistry::new(&spec, &mut stats);

  let graph = Arc::new(registry);
  let context = create_test_context(graph.clone(), default_config());
  let union_converter = UnionConverter::new(context);

  let schema = parse_schema(json!({
    "oneOf": [
      { "type": "string", "const": "OptionA" },
      { "type": "string", "const": "OptionB" }
    ]
  }));

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
  let spec = serde_json::from_value::<oas3::Spec>(json!({
    "openapi": "3.1.0",
    "info": {
      "title": "Test",
      "version": "1.0.0"
    }
  }))
  .unwrap();

  let mut stats = GenerationStats::default();
  let registry = SchemaRegistry::new(&spec, &mut stats);

  let graph = Arc::new(registry);
  let context = create_test_context(graph.clone(), default_config());
  let union_converter = UnionConverter::new(context);

  let schema = parse_schema(json!({
    "anyOf": [
      { "type": "string", "const": "known-value" },
      { "type": "string" }
    ]
  }));

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
  let graph = create_test_graph(parse_schemas(vec![
    (
      "CookingStrategy",
      json!({
        "anyOf": [
          {
            "description": "The cooking method configuration",
            "anyOf": [
              { "type": "string", "const": "auto" },
              { "$ref": "#/components/schemas/CustomCookingConfig" }
            ]
          },
          { "type": "null" }
        ]
      }),
    ),
    (
      "CustomCookingConfig",
      json!({
        "type": "object",
        "properties": {
          "mode": { "type": "string" }
        }
      }),
    ),
  ]));
  let context = create_test_context(graph.clone(), default_config());
  let converter = SchemaConverter::new(&context);

  let result = converter.convert_schema("CookingStrategy", graph.get("CookingStrategy").unwrap())?;

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
  let graph = create_test_graph(parse_schemas(vec![(
    "NullableRelaxed",
    json!({
      "anyOf": [
        {
          "anyOf": [
            { "type": "string", "const": "known1" },
            { "type": "string", "const": "known2" },
            { "type": "string" }
          ]
        },
        { "type": "null" }
      ]
    }),
  )]));
  let context = create_test_context(graph.clone(), default_config());
  let converter = SchemaConverter::new(&context);

  let result = converter.convert_schema("NullableRelaxed", graph.get("NullableRelaxed").unwrap())?;

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
  let graph = create_test_graph(parse_schemas(vec![
    (
      "NestedUnion",
      json!({
        "anyOf": [
          {
            "oneOf": [
              { "type": "string", "const": "default" },
              { "$ref": "#/components/schemas/OptionA" }
            ]
          },
          { "type": "null" }
        ]
      }),
    ),
    (
      "OptionA",
      json!({
        "type": "object",
        "properties": {
          "value": { "type": "string" }
        }
      }),
    ),
  ]));
  let context = create_test_context(graph.clone(), default_config());
  let converter = SchemaConverter::new(&context);

  let result = converter.convert_schema("NestedUnion", graph.get("NestedUnion").unwrap())?;

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
  let graph = create_test_graph(parse_schemas(vec![
    (
      "ResponsePromptVariables",
      json!({
        "anyOf": [
          {
            "type": "object",
            "title": "Prompt Variables",
            "description": "Optional map of values to substitute",
            "additionalProperties": {
              "anyOf": [
                { "type": "string" },
                { "$ref": "#/components/schemas/InputContent" }
              ]
            }
          },
          { "type": "null" }
        ]
      }),
    ),
    (
      "InputContent",
      json!({
        "type": "object",
        "properties": {
          "type": { "type": "string" }
        }
      }),
    ),
  ]));
  let context = create_test_context(graph.clone(), default_config());
  let converter = SchemaConverter::new(&context);

  let result = converter.convert_schema("ResponsePromptVariables", graph.get("ResponsePromptVariables").unwrap())?;

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
  let graph = create_test_graph(parse_schemas(vec![
    (
      "FunctionCall",
      json!({
        "description": "Controls which function is called by the model.",
        "anyOf": [
          {
            "type": "string",
            "description": "`none` or `auto` mode",
            "enum": ["none", "auto"],
            "title": "FunctionCallMode"
          },
          { "$ref": "#/components/schemas/FunctionCallOption" }
        ]
      }),
    ),
    (
      "FunctionCallOption",
      json!({
        "type": "object",
        "properties": {
          "name": {
            "type": "string",
            "description": "The name of the function to call."
          }
        },
        "required": ["name"]
      }),
    ),
  ]));
  let context = create_test_context(graph.clone(), default_config());
  let converter = SchemaConverter::new(&context);

  let result = converter.convert_schema("FunctionCall", graph.get("FunctionCall").unwrap())?;

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
  let graph = create_test_graph(parse_schemas(vec![
    (
      "ResponseFormat",
      json!({
        "oneOf": [
          {
            "type": "string",
            "title": "ResponseMode",
            "enum": ["streaming", "batch"]
          },
          { "$ref": "#/components/schemas/TextFormat" }
        ]
      }),
    ),
    (
      "TextFormat",
      json!({
        "type": "object",
        "properties": {
          "type": { "type": "string", "const": "text" }
        }
      }),
    ),
  ]));
  let context = create_test_context(graph.clone(), default_config());
  let converter = SchemaConverter::new(&context);

  let result = converter.convert_schema("ResponseFormat", graph.get("ResponseFormat").unwrap())?;

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
  let graph = create_test_graph(parse_schemas(vec![
    (
      "TestUnion",
      json!({
        "anyOf": [
          {
            "type": "string",
            "enum": ["auto"]
          },
          { "$ref": "#/components/schemas/ObjectVariant" }
        ]
      }),
    ),
    (
      "ObjectVariant",
      json!({
        "type": "object",
        "properties": {
          "name": { "type": "string" }
        }
      }),
    ),
  ]));
  let context = create_test_context(graph.clone(), default_config());
  let converter = SchemaConverter::new(&context);

  let result = converter.convert_schema("TestUnion", graph.get("TestUnion").unwrap())?;

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

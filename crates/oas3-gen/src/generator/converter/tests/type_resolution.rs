use std::{collections::BTreeMap, sync::Arc};

use oas3::spec::{ObjectOrReference, ObjectSchema, SchemaType, SchemaTypeSet};
use serde_json::json;

use crate::{
  generator::{
    ast::RustType,
    converter::{SchemaExt, type_resolver::TypeResolverBuilder},
    naming::inference::extract_common_variant_prefix,
    schema_registry::SchemaRegistry,
  },
  tests::common::{create_test_graph, default_config},
};

fn make_string_schema() -> ObjectSchema {
  ObjectSchema {
    schema_type: Some(SchemaTypeSet::Single(SchemaType::String)),
    ..Default::default()
  }
}

fn make_object_schema_with_property(prop_name: &str, prop_schema: ObjectSchema) -> ObjectSchema {
  ObjectSchema {
    schema_type: Some(SchemaTypeSet::Single(SchemaType::Object)),
    properties: BTreeMap::from([(prop_name.to_string(), ObjectOrReference::Object(prop_schema))]),
    ..Default::default()
  }
}

fn create_empty_test_graph() -> Arc<SchemaRegistry> {
  let spec = oas3::Spec {
    openapi: "3.0.0".to_string(),
    info: oas3::spec::Info {
      title: "Test".to_string(),
      summary: None,
      version: "1.0.0".to_string(),
      description: None,
      terms_of_service: None,
      contact: None,
      license: None,
      extensions: BTreeMap::new(),
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

  let graph = SchemaRegistry::from_spec(spec).registry;
  Arc::new(graph)
}

#[test]
fn test_title_resolution() {
  let cases = [
    (
      "title_ignored_when_schema_type_present",
      ObjectSchema {
        title: Some("Message".to_string()),
        schema_type: Some(SchemaTypeSet::Single(SchemaType::String)),
        ..Default::default()
      },
      "String",
    ),
    (
      "title_used_when_no_schema_type",
      ObjectSchema {
        title: Some("CustomType".to_string()),
        ..Default::default()
      },
      "CustomType",
    ),
  ];

  for (case_name, schema, expected_type) in cases {
    let named_schema = make_object_schema_with_property("field", make_string_schema());
    let schema_name = schema.title.clone().unwrap_or_else(|| "Message".to_string());
    let graph = create_test_graph(BTreeMap::from([(schema_name, named_schema)]));
    let resolver = TypeResolverBuilder::default()
      .config(default_config())
      .graph(graph.clone())
      .build()
      .unwrap();

    let result = resolver.resolve_type(&schema).unwrap();
    assert_eq!(result.to_rust_type(), expected_type, "failed for case: {case_name}");
  }
}

#[test]
fn test_anyof_freeform_string_detection() {
  let graph = create_test_graph(BTreeMap::new());

  let enum_schema = ObjectSchema {
    schema_type: Some(SchemaTypeSet::Single(SchemaType::String)),
    enum_values: vec![json!("value1"), json!("value2")],
    ..Default::default()
  };

  let freeform_schema = make_string_schema();

  let enum_only_schema = ObjectSchema {
    schema_type: Some(SchemaTypeSet::Single(SchemaType::String)),
    enum_values: vec![json!("value1")],
    ..Default::default()
  };

  let cases = [
    (
      "with_freeform_string",
      vec![
        ObjectOrReference::Object(enum_schema.clone()),
        ObjectOrReference::Object(freeform_schema),
      ],
      true,
    ),
    (
      "without_freeform_string",
      vec![
        ObjectOrReference::Object(enum_only_schema.clone()),
        ObjectOrReference::Object(enum_only_schema),
      ],
      false,
    ),
  ];

  for (case_name, variants, expected_has_freeform) in cases {
    let has_freeform = variants.iter().any(|v| {
      v.resolve(graph.spec()).ok().is_some_and(|resolved| {
        resolved.const_value.is_none()
          && resolved.enum_values.is_empty()
          && resolved.schema_type == Some(SchemaTypeSet::Single(SchemaType::String))
      })
    });

    assert_eq!(has_freeform, expected_has_freeform, "failed for case: {case_name}");
  }
}

#[test]
fn test_union_to_type_ref_conversion() {
  let cache_schema = ObjectSchema {
    schema_type: Some(SchemaTypeSet::Single(SchemaType::Object)),
    properties: BTreeMap::from([(
      "type".to_string(),
      ObjectOrReference::Object(ObjectSchema {
        schema_type: Some(SchemaTypeSet::Single(SchemaType::String)),
        const_value: Some(json!("ephemeral")),
        ..Default::default()
      }),
    )]),
    ..Default::default()
  };

  let graph = create_test_graph(BTreeMap::from([("CacheControlEphemeral".to_string(), cache_schema)]));
  let resolver = TypeResolverBuilder::default()
    .config(default_config())
    .graph(graph.clone())
    .build()
    .unwrap();

  let inner_schema = ObjectSchema {
    one_of: vec![ObjectOrReference::Ref {
      ref_path: "#/components/schemas/CacheControlEphemeral".to_string(),
      summary: None,
      description: None,
    }],
    ..Default::default()
  };

  let null_schema = ObjectSchema {
    schema_type: Some(SchemaTypeSet::Single(SchemaType::Null)),
    ..Default::default()
  };

  let cases = [
    (
      "nested_oneof_resolves_to_ref",
      vec![
        ObjectOrReference::Object(inner_schema),
        ObjectOrReference::Object(null_schema.clone()),
      ],
      Some("Option<CacheControlEphemeral>"),
    ),
    (
      "no_resolvable_variants",
      vec![
        ObjectOrReference::Object(null_schema.clone()),
        ObjectOrReference::Object(null_schema),
      ],
      None,
    ),
  ];

  for (case_name, variants, expected_type) in cases {
    let result = resolver.resolve_union(&variants).unwrap();
    match expected_type {
      Some(expected) => {
        assert!(result.is_some(), "expected Some for case: {case_name}");
        assert_eq!(
          result.unwrap().to_rust_type(),
          expected,
          "type mismatch for case: {case_name}"
        );
      }
      None => {
        assert!(result.is_none(), "expected None for case: {case_name}");
      }
    }
  }
}

#[test]
fn test_array_type_resolution() {
  let custom_schema = make_object_schema_with_property("field", make_string_schema());
  let graph = create_test_graph(BTreeMap::from([("CustomType".to_string(), custom_schema)]));
  let resolver = TypeResolverBuilder::default()
    .config(default_config())
    .graph(graph.clone())
    .build()
    .unwrap();

  let cases: Vec<(&str, ObjectSchema, &str)> = vec![
    (
      "array_with_inline_string_items",
      ObjectSchema {
        schema_type: Some(SchemaTypeSet::Single(SchemaType::Array)),
        items: Some(Box::new(oas3::spec::Schema::Object(Box::new(
          ObjectOrReference::Object(make_string_schema()),
        )))),
        ..Default::default()
      },
      "Vec<String>",
    ),
    (
      "array_without_items_fallback",
      ObjectSchema {
        schema_type: Some(SchemaTypeSet::Single(SchemaType::Array)),
        items: None,
        ..Default::default()
      },
      "Vec<serde_json::Value>",
    ),
    (
      "array_with_boolean_schema_items",
      ObjectSchema {
        schema_type: Some(SchemaTypeSet::Single(SchemaType::Array)),
        items: Some(Box::new(oas3::spec::Schema::Boolean(oas3::spec::BooleanSchema(true)))),
        ..Default::default()
      },
      "Vec<serde_json::Value>",
    ),
    (
      "array_with_ref_items",
      ObjectSchema {
        schema_type: Some(SchemaTypeSet::Single(SchemaType::Array)),
        items: Some(Box::new(oas3::spec::Schema::Object(Box::new(ObjectOrReference::Ref {
          ref_path: "#/components/schemas/CustomType".to_string(),
          summary: None,
          description: None,
        })))),
        ..Default::default()
      },
      "Vec<CustomType>",
    ),
  ];

  for (case_name, schema, expected_type) in cases {
    let result = resolver.resolve_type(&schema).unwrap();
    assert_eq!(result.to_rust_type(), expected_type, "failed for case: {case_name}");
  }
}

#[test]
fn test_schema_ext_methods() {
  let mut schema = ObjectSchema {
    schema_type: Some(SchemaTypeSet::Single(SchemaType::String)),
    ..ObjectSchema::default()
  };
  assert!(schema.is_primitive(), "string without properties should be primitive");

  schema
    .properties
    .insert("foo".to_string(), ObjectOrReference::Object(ObjectSchema::default()));
  assert!(!schema.is_primitive(), "schema with properties should not be primitive");

  let null_only = ObjectSchema {
    schema_type: Some(SchemaTypeSet::Single(SchemaType::Null)),
    ..ObjectSchema::default()
  };
  assert!(null_only.is_nullable_object(), "null-only schema should be nullable");

  let nullable_object = ObjectSchema {
    schema_type: Some(SchemaTypeSet::Multiple(vec![SchemaType::Object, SchemaType::Null])),
    ..ObjectSchema::default()
  };
  assert!(
    nullable_object.is_nullable_object(),
    "object|null without properties should be nullable"
  );

  let mut nullable_with_props = nullable_object.clone();
  nullable_with_props
    .properties
    .insert("x".into(), ObjectOrReference::Object(ObjectSchema::default()));
  assert!(
    !nullable_with_props.is_nullable_object(),
    "object|null with properties should not be nullable"
  );

  let string_enum_schema = ObjectSchema {
    schema_type: Some(SchemaTypeSet::Single(SchemaType::String)),
    enum_values: vec![json!("auto"), json!("none")],
    ..ObjectSchema::default()
  };
  assert!(
    !string_enum_schema.is_primitive(),
    "string enum with 2+ values should NOT be primitive"
  );

  let single_enum_schema = ObjectSchema {
    schema_type: Some(SchemaTypeSet::Single(SchemaType::String)),
    enum_values: vec![json!("only_value")],
    ..ObjectSchema::default()
  };
  assert!(
    single_enum_schema.is_primitive(),
    "string enum with 1 value should be primitive (const-like)"
  );
}

#[test]
fn test_basic_type_resolution() {
  let graph = create_empty_test_graph();
  let resolver = TypeResolverBuilder::default()
    .config(default_config())
    .graph(graph.clone())
    .build()
    .unwrap();

  let item_schema = make_string_schema();

  let cases: Vec<(&str, ObjectSchema, &str)> = vec![
    (
      "simple_int32",
      ObjectSchema {
        schema_type: Some(SchemaTypeSet::Single(SchemaType::Integer)),
        format: Some("int32".to_string()),
        ..ObjectSchema::default()
      },
      "i32",
    ),
    (
      "nullable_string",
      ObjectSchema {
        schema_type: Some(SchemaTypeSet::Multiple(vec![SchemaType::String, SchemaType::Null])),
        ..ObjectSchema::default()
      },
      "Option<String>",
    ),
    (
      "array_of_strings",
      ObjectSchema {
        schema_type: Some(SchemaTypeSet::Single(SchemaType::Array)),
        items: Some(Box::new(oas3::spec::Schema::Object(Box::new(
          oas3::spec::ObjectOrReference::Object(item_schema),
        )))),
        ..ObjectSchema::default()
      },
      "Vec<String>",
    ),
  ];

  for (case_name, schema, expected_type) in cases {
    let result = resolver.resolve_type(&schema).unwrap();
    assert_eq!(result.to_rust_type(), expected_type, "failed for case: {case_name}");
  }
}

#[test]
fn test_array_with_union_items_inline_generation() {
  let tool_schema = make_object_schema_with_property("name", make_string_schema());
  let bash_tool_schema = make_object_schema_with_property("command", make_string_schema());
  let type_a = make_object_schema_with_property("field_a", make_string_schema());
  let type_b = make_object_schema_with_property(
    "field_b",
    ObjectSchema {
      schema_type: Some(SchemaTypeSet::Single(SchemaType::Integer)),
      ..Default::default()
    },
  );
  let item_schema = make_object_schema_with_property("id", make_string_schema());

  let graph = create_test_graph(BTreeMap::from([
    ("Tool".to_string(), tool_schema),
    ("BashTool".to_string(), bash_tool_schema),
    ("TypeA".to_string(), type_a),
    ("TypeB".to_string(), type_b),
    ("Item".to_string(), item_schema),
  ]));
  let resolver = TypeResolverBuilder::default()
    .config(default_config())
    .graph(graph.clone())
    .build()
    .unwrap();

  let oneof_array_schema = ObjectSchema {
    schema_type: Some(SchemaTypeSet::Single(SchemaType::Array)),
    items: Some(Box::new(oas3::spec::Schema::Object(Box::new(
      ObjectOrReference::Object(ObjectSchema {
        one_of: vec![
          ObjectOrReference::Ref {
            ref_path: "#/components/schemas/Tool".to_string(),
            summary: None,
            description: None,
          },
          ObjectOrReference::Ref {
            ref_path: "#/components/schemas/BashTool".to_string(),
            summary: None,
            description: None,
          },
        ],
        ..Default::default()
      }),
    )))),
    ..Default::default()
  };

  let anyof_array_schema = ObjectSchema {
    schema_type: Some(SchemaTypeSet::Single(SchemaType::Array)),
    items: Some(Box::new(oas3::spec::Schema::Object(Box::new(
      ObjectOrReference::Object(ObjectSchema {
        any_of: vec![
          ObjectOrReference::Ref {
            ref_path: "#/components/schemas/TypeA".to_string(),
            summary: None,
            description: None,
          },
          ObjectOrReference::Ref {
            ref_path: "#/components/schemas/TypeB".to_string(),
            summary: None,
            description: None,
          },
        ],
        ..Default::default()
      }),
    )))),
    ..Default::default()
  };

  let ref_array_schema = ObjectSchema {
    schema_type: Some(SchemaTypeSet::Single(SchemaType::Array)),
    items: Some(Box::new(oas3::spec::Schema::Object(Box::new(ObjectOrReference::Ref {
      ref_path: "#/components/schemas/Item".to_string(),
      summary: None,
      description: None,
    })))),
    ..Default::default()
  };

  let oneof_result = resolver
    .resolve_property_type(
      "CreateMessageParams",
      "tools",
      &oneof_array_schema,
      &ObjectOrReference::Object(oneof_array_schema.clone()),
      None,
    )
    .unwrap();
  assert_eq!(
    oneof_result.result.to_rust_type(),
    "Vec<ToolKind>",
    "oneOf array type mismatch"
  );
  assert_eq!(
    oneof_result.inline_types.len(),
    1,
    "oneOf should generate one inline enum"
  );

  let inline_type = &oneof_result.inline_types[0];
  match inline_type {
    RustType::Enum(enum_def) => {
      assert_eq!(enum_def.name.as_str(), "ToolKind");
      assert_eq!(enum_def.variants.len(), 2);
      let variant_names: Vec<_> = enum_def.variants.iter().map(|v| v.name.as_str()).collect();
      assert!(
        variant_names.contains(&"Tool"),
        "Missing Tool variant, found: {variant_names:?}"
      );
      assert!(
        variant_names.contains(&"Bash"),
        "Missing Bash variant, found: {variant_names:?}"
      );
    }
    _ => panic!("Expected an enum type for oneOf"),
  }

  let anyof_result = resolver
    .resolve_property_type(
      "Response",
      "items",
      &anyof_array_schema,
      &ObjectOrReference::Object(anyof_array_schema.clone()),
      None,
    )
    .unwrap();
  assert_eq!(
    anyof_result.result.to_rust_type(),
    "Vec<TypeItemKind>",
    "anyOf array type mismatch"
  );
  assert!(
    !anyof_result.inline_types.is_empty(),
    "anyOf should generate inline types"
  );

  let ref_result = resolver
    .resolve_property_type(
      "Parent",
      "items",
      &ref_array_schema,
      &ObjectOrReference::Object(ref_array_schema.clone()),
      None,
    )
    .unwrap();
  assert_eq!(ref_result.result.to_rust_type(), "Vec<Item>", "ref array type mismatch");
  assert!(
    ref_result.inline_types.is_empty(),
    "ref items should not generate inline types"
  );
}

#[test]
fn test_multi_ref_oneof_returns_none_for_fallback() {
  let type_a = make_object_schema_with_property("field_a", make_string_schema());
  let type_b = make_object_schema_with_property("field_b", make_string_schema());
  let type_c = make_object_schema_with_property("field_c", make_string_schema());

  let graph = create_test_graph(BTreeMap::from([
    ("TypeA".to_string(), type_a),
    ("TypeB".to_string(), type_b),
    ("TypeC".to_string(), type_c),
  ]));
  let resolver = TypeResolverBuilder::default()
    .config(default_config())
    .graph(graph.clone())
    .build()
    .unwrap();

  let multi_ref_variants = vec![
    ObjectOrReference::Ref {
      ref_path: "#/components/schemas/TypeA".to_string(),
      summary: None,
      description: None,
    },
    ObjectOrReference::Ref {
      ref_path: "#/components/schemas/TypeB".to_string(),
      summary: None,
      description: None,
    },
    ObjectOrReference::Ref {
      ref_path: "#/components/schemas/TypeC".to_string(),
      summary: None,
      description: None,
    },
  ];

  let result = resolver.resolve_union(&multi_ref_variants).unwrap();
  assert!(
    result.is_none(),
    "multi-ref oneOf should return None to trigger enum generation, got: {:?}",
    result.map(|r| r.to_rust_type())
  );

  let single_ref_with_null = vec![
    ObjectOrReference::Ref {
      ref_path: "#/components/schemas/TypeA".to_string(),
      summary: None,
      description: None,
    },
    ObjectOrReference::Object(ObjectSchema {
      schema_type: Some(SchemaTypeSet::Single(SchemaType::Null)),
      ..Default::default()
    }),
  ];

  let result = resolver.resolve_union(&single_ref_with_null).unwrap();
  assert!(result.is_some(), "single ref with null should collapse to Option<T>");
  assert_eq!(
    result.unwrap().to_rust_type(),
    "Option<TypeA>",
    "should be Option<TypeA>"
  );
}

#[test]
fn test_extract_common_variant_prefix() {
  struct Case {
    variants: Vec<&'static str>,
    expected: Option<&'static str>,
    description: &'static str,
  }

  let cases = [
    Case {
      variants: vec![
        "BetaResponseCharLocationCitation",
        "BetaResponseUrlCitation",
        "BetaResponseFileCitation",
      ],
      expected: Some("BetaCitation"),
      description: "first prefix (Beta) + suffix (Citation) - terse naming",
    },
    Case {
      variants: vec!["ContentBlockStart", "ContentBlockDelta", "ContentBlockStop"],
      expected: Some("ContentBlock"),
      description: "full common prefix (ContentBlock), no suffix",
    },
    Case {
      variants: vec!["Tool", "BashTool"],
      expected: None,
      description: "no common prefix (Tool vs Bash) - returns None",
    },
    Case {
      variants: vec!["TypeA", "TypeB", "TypeC"],
      expected: Some("Type"),
      description: "common prefix (Type) only, no suffix",
    },
    Case {
      variants: vec!["AlphaFoo", "BetaBar", "GammaQux"],
      expected: None,
      description: "no common prefix - returns None",
    },
    Case {
      variants: vec!["RequestBody"],
      expected: None,
      description: "single variant - returns None",
    },
    Case {
      variants: vec![],
      expected: None,
      description: "empty variants - returns None",
    },
    Case {
      variants: vec!["ApiErrorNotFound", "ApiErrorBadRequest", "ApiErrorUnauthorized"],
      expected: Some("ApiError"),
      description: "full common prefix (ApiError), no suffix",
    },
    Case {
      variants: vec!["BetaMessageStartEvent", "BetaMessageDeltaEvent", "BetaMessageStopEvent"],
      expected: Some("BetaEvent"),
      description: "first prefix (Beta) + suffix (Event) - terse naming",
    },
    Case {
      variants: vec!["FooBarBaz", "FooQuxBaz"],
      expected: Some("FooBaz"),
      description: "first prefix (Foo) + suffix (Baz) - terse naming",
    },
    Case {
      variants: vec!["StreamEventStart", "StreamEventData", "StreamEventEnd"],
      expected: Some("StreamEvent"),
      description: "full common prefix (StreamEvent), no suffix",
    },
  ];

  for case in cases {
    let variants: Vec<ObjectOrReference<ObjectSchema>> = case
      .variants
      .iter()
      .map(|name| ObjectOrReference::Ref {
        ref_path: format!("#/components/schemas/{name}"),
        summary: None,
        description: None,
      })
      .collect();

    let result = extract_common_variant_prefix(&variants);

    assert_eq!(
      result.as_ref().map(|r| r.name.as_str()),
      case.expected,
      "{}: expected {:?}, got {:?}",
      case.description,
      case.expected,
      result.as_ref().map(|r| r.name.as_str())
    );
  }
}

#[test]
fn test_union_naming_with_common_suffix() {
  let citation_a = make_object_schema_with_property("type", make_string_schema());
  let citation_b = make_object_schema_with_property("url", make_string_schema());
  let citation_c = make_object_schema_with_property("file", make_string_schema());

  let graph = create_test_graph(BTreeMap::from([
    ("BetaResponseCharLocationCitation".to_string(), citation_a),
    ("BetaResponseUrlCitation".to_string(), citation_b),
    ("BetaResponseFileCitation".to_string(), citation_c),
  ]));
  let resolver = TypeResolverBuilder::default()
    .config(default_config())
    .graph(graph.clone())
    .build()
    .unwrap();

  let union_schema = ObjectSchema {
    one_of: vec![
      ObjectOrReference::Ref {
        ref_path: "#/components/schemas/BetaResponseCharLocationCitation".to_string(),
        summary: None,
        description: None,
      },
      ObjectOrReference::Ref {
        ref_path: "#/components/schemas/BetaResponseUrlCitation".to_string(),
        summary: None,
        description: None,
      },
      ObjectOrReference::Ref {
        ref_path: "#/components/schemas/BetaResponseFileCitation".to_string(),
        summary: None,
        description: None,
      },
    ],
    ..Default::default()
  };

  let result = resolver
    .resolve_property_type(
      "BetaResponse",
      "citation",
      &union_schema,
      &ObjectOrReference::Object(union_schema.clone()),
      None,
    )
    .unwrap();

  assert_eq!(result.result.to_rust_type(), "BetaCitationKind");
  assert_eq!(result.inline_types.len(), 1);
  if let RustType::Enum(enum_def) = &result.inline_types[0] {
    assert_eq!(enum_def.name.as_str(), "BetaCitationKind");
  } else {
    panic!("Expected enum type");
  }
}

#[test]
fn test_union_naming_without_common_suffix() {
  let tool_a = make_object_schema_with_property("name", make_string_schema());
  let tool_b = make_object_schema_with_property("command", make_string_schema());

  let graph = create_test_graph(BTreeMap::from([
    ("BetaTool".to_string(), tool_a),
    ("BetaBashTool20241022".to_string(), tool_b),
  ]));
  let resolver = TypeResolverBuilder::default()
    .config(default_config())
    .graph(graph.clone())
    .build()
    .unwrap();
  let union_schema = ObjectSchema {
    one_of: vec![
      ObjectOrReference::Ref {
        ref_path: "#/components/schemas/BetaTool".to_string(),
        summary: None,
        description: None,
      },
      ObjectOrReference::Ref {
        ref_path: "#/components/schemas/BetaBashTool20241022".to_string(),
        summary: None,
        description: None,
      },
    ],
    ..Default::default()
  };

  let result = resolver
    .resolve_property_type(
      "Request",
      "tool",
      &union_schema,
      &ObjectOrReference::Object(union_schema.clone()),
      None,
    )
    .unwrap();

  assert_eq!(result.result.to_rust_type(), "BetaToolKind");
  assert_eq!(result.inline_types.len(), 1);
  if let RustType::Enum(enum_def) = &result.inline_types[0] {
    assert_eq!(enum_def.name.as_str(), "BetaToolKind");
  } else {
    panic!("Expected enum type");
  }
}

#[test]
fn test_array_union_naming_with_common_suffix() {
  let event_a = make_object_schema_with_property("started", make_string_schema());
  let event_b = make_object_schema_with_property("data", make_string_schema());
  let event_c = make_object_schema_with_property("stopped", make_string_schema());

  let graph = create_test_graph(BTreeMap::from([
    ("BetaMessageStartEvent".to_string(), event_a),
    ("BetaMessageDeltaEvent".to_string(), event_b),
    ("BetaMessageStopEvent".to_string(), event_c),
  ]));
  let resolver = TypeResolverBuilder::default()
    .config(default_config())
    .graph(graph.clone())
    .build()
    .unwrap();

  let array_schema = ObjectSchema {
    schema_type: Some(SchemaTypeSet::Single(SchemaType::Array)),
    items: Some(Box::new(oas3::spec::Schema::Object(Box::new(
      ObjectOrReference::Object(ObjectSchema {
        one_of: vec![
          ObjectOrReference::Ref {
            ref_path: "#/components/schemas/BetaMessageStartEvent".to_string(),
            summary: None,
            description: None,
          },
          ObjectOrReference::Ref {
            ref_path: "#/components/schemas/BetaMessageDeltaEvent".to_string(),
            summary: None,
            description: None,
          },
          ObjectOrReference::Ref {
            ref_path: "#/components/schemas/BetaMessageStopEvent".to_string(),
            summary: None,
            description: None,
          },
        ],
        ..Default::default()
      }),
    )))),
    ..Default::default()
  };

  let result = resolver
    .resolve_property_type(
      "Stream",
      "events",
      &array_schema,
      &ObjectOrReference::Object(array_schema.clone()),
      None,
    )
    .unwrap();

  assert_eq!(result.result.to_rust_type(), "Vec<BetaEventKind>");
  assert_eq!(result.inline_types.len(), 1);
  if let RustType::Enum(enum_def) = &result.inline_types[0] {
    assert_eq!(enum_def.name.as_str(), "BetaEventKind");
  } else {
    panic!("Expected enum type");
  }
}

#[test]
fn test_additional_properties_map_only_boolean() {
  let graph = create_empty_test_graph();
  let resolver = TypeResolverBuilder::default()
    .config(default_config())
    .graph(graph.clone())
    .build()
    .unwrap();
  let schema = ObjectSchema {
    schema_type: Some(SchemaTypeSet::Single(SchemaType::Object)),
    additional_properties: Some(oas3::spec::Schema::Object(Box::new(ObjectOrReference::Object(
      ObjectSchema {
        schema_type: Some(SchemaTypeSet::Single(SchemaType::Boolean)),
        ..Default::default()
      },
    )))),
    ..Default::default()
  };

  let result = resolver.resolve_type(&schema).unwrap();
  assert_eq!(
    result.to_rust_type(),
    "std::collections::HashMap<String, bool>",
    "map-only object with additionalProperties: {{type: boolean}} should resolve to HashMap<String, bool>"
  );
}

#[test]
fn test_additional_properties_map_only_string() {
  let graph = create_empty_test_graph();
  let resolver = TypeResolverBuilder::default()
    .config(default_config())
    .graph(graph.clone())
    .build()
    .unwrap();

  let schema = ObjectSchema {
    schema_type: Some(SchemaTypeSet::Single(SchemaType::Object)),
    additional_properties: Some(oas3::spec::Schema::Object(Box::new(ObjectOrReference::Object(
      ObjectSchema {
        schema_type: Some(SchemaTypeSet::Single(SchemaType::String)),
        ..Default::default()
      },
    )))),
    ..Default::default()
  };

  let result = resolver.resolve_type(&schema).unwrap();
  assert_eq!(
    result.to_rust_type(),
    "std::collections::HashMap<String, String>",
    "map-only object with additionalProperties: {{type: string}} should resolve to HashMap<String, String>"
  );
}

#[test]
fn test_additional_properties_map_only_integer() {
  let graph = create_empty_test_graph();
  let resolver = TypeResolverBuilder::default()
    .config(default_config())
    .graph(graph.clone())
    .build()
    .unwrap();

  let schema = ObjectSchema {
    schema_type: Some(SchemaTypeSet::Single(SchemaType::Object)),
    additional_properties: Some(oas3::spec::Schema::Object(Box::new(ObjectOrReference::Object(
      ObjectSchema {
        schema_type: Some(SchemaTypeSet::Single(SchemaType::Integer)),
        ..Default::default()
      },
    )))),
    ..Default::default()
  };

  let result = resolver.resolve_type(&schema).unwrap();
  assert_eq!(
    result.to_rust_type(),
    "std::collections::HashMap<String, i64>",
    "map-only object with additionalProperties: {{type: integer}} should resolve to HashMap<String, i64>"
  );
}

#[test]
fn test_additional_properties_map_only_ref() {
  let custom_schema = make_object_schema_with_property("field", make_string_schema());
  let graph = create_test_graph(BTreeMap::from([("CustomType".to_string(), custom_schema)]));
  let resolver = TypeResolverBuilder::default()
    .config(default_config())
    .graph(graph.clone())
    .build()
    .unwrap();

  let schema = ObjectSchema {
    schema_type: Some(SchemaTypeSet::Single(SchemaType::Object)),
    additional_properties: Some(oas3::spec::Schema::Object(Box::new(ObjectOrReference::Ref {
      ref_path: "#/components/schemas/CustomType".to_string(),
      summary: None,
      description: None,
    }))),
    ..Default::default()
  };

  let result = resolver.resolve_type(&schema).unwrap();
  assert_eq!(
    result.to_rust_type(),
    "std::collections::HashMap<String, CustomType>",
    "map-only object with additionalProperties: {{$ref: CustomType}} should resolve to HashMap<String, CustomType>"
  );
}

#[test]
fn test_additional_properties_map_only_empty_schema() {
  let graph = create_empty_test_graph();
  let resolver = TypeResolverBuilder::default()
    .config(default_config())
    .graph(graph.clone())
    .build()
    .unwrap();

  let schema = ObjectSchema {
    schema_type: Some(SchemaTypeSet::Single(SchemaType::Object)),
    additional_properties: Some(oas3::spec::Schema::Object(Box::new(ObjectOrReference::Object(
      ObjectSchema::default(),
    )))),
    ..Default::default()
  };

  let result = resolver.resolve_type(&schema).unwrap();
  assert_eq!(
    result.to_rust_type(),
    "std::collections::HashMap<String, serde_json::Value>",
    "map-only object with additionalProperties: {{}} should resolve to HashMap<String, serde_json::Value>"
  );
}

#[test]
fn test_additional_properties_boolean_true() {
  let graph = create_empty_test_graph();
  let resolver = TypeResolverBuilder::default()
    .config(default_config())
    .graph(graph.clone())
    .build()
    .unwrap();

  let schema = ObjectSchema {
    schema_type: Some(SchemaTypeSet::Single(SchemaType::Object)),
    additional_properties: Some(oas3::spec::Schema::Boolean(oas3::spec::BooleanSchema(true))),
    ..Default::default()
  };

  let result = resolver.resolve_type(&schema).unwrap();
  assert_eq!(
    result.to_rust_type(),
    "std::collections::HashMap<String, serde_json::Value>",
    "map-only object with additionalProperties: true should resolve to HashMap<String, serde_json::Value>"
  );
}

#[test]
fn test_object_with_properties_not_resolved_as_map() {
  let graph = create_empty_test_graph();
  let resolver = TypeResolverBuilder::default()
    .config(default_config())
    .graph(graph.clone())
    .build()
    .unwrap();

  let schema = ObjectSchema {
    schema_type: Some(SchemaTypeSet::Single(SchemaType::Object)),
    properties: BTreeMap::from([("name".to_string(), ObjectOrReference::Object(make_string_schema()))]),
    additional_properties: Some(oas3::spec::Schema::Object(Box::new(ObjectOrReference::Object(
      ObjectSchema {
        schema_type: Some(SchemaTypeSet::Single(SchemaType::Boolean)),
        ..Default::default()
      },
    )))),
    ..Default::default()
  };

  let result = resolver.resolve_type(&schema).unwrap();
  assert_eq!(
    result.to_rust_type(),
    "serde_json::Value",
    "object with properties should NOT be resolved as map type (it's a struct)"
  );
}

#[test]
fn test_resolve_additional_properties_type_ref() {
  let custom_schema = make_object_schema_with_property("field", make_string_schema());
  let graph = create_test_graph(BTreeMap::from([("AgentConfig".to_string(), custom_schema)]));
  let resolver = TypeResolverBuilder::default()
    .config(default_config())
    .graph(graph.clone())
    .build()
    .unwrap();

  let additional = oas3::spec::Schema::Object(Box::new(ObjectOrReference::Ref {
    ref_path: "#/components/schemas/AgentConfig".to_string(),
    summary: None,
    description: None,
  }));

  let result = resolver.resolve_additional_properties_type(&additional).unwrap();
  assert_eq!(
    result.to_rust_type(),
    "AgentConfig",
    "additionalProperties with $ref should resolve to the referenced type name"
  );
}

#[test]
fn test_resolve_additional_properties_type_boolean() {
  let graph = create_empty_test_graph();
  let resolver = TypeResolverBuilder::default()
    .config(default_config())
    .graph(graph.clone())
    .build()
    .unwrap();

  let additional = oas3::spec::Schema::Object(Box::new(ObjectOrReference::Object(ObjectSchema {
    schema_type: Some(SchemaTypeSet::Single(SchemaType::Boolean)),
    ..Default::default()
  })));

  let result = resolver.resolve_additional_properties_type(&additional).unwrap();
  assert_eq!(
    result.to_rust_type(),
    "bool",
    "additionalProperties with type: boolean should resolve to bool"
  );
}

#[test]
fn test_resolve_additional_properties_type_empty() {
  let graph = create_empty_test_graph();
  let resolver = TypeResolverBuilder::default()
    .config(default_config())
    .graph(graph.clone())
    .build()
    .unwrap();

  let additional = oas3::spec::Schema::Object(Box::new(ObjectOrReference::Object(ObjectSchema::default())));

  let result = resolver.resolve_additional_properties_type(&additional).unwrap();
  assert_eq!(
    result.to_rust_type(),
    "serde_json::Value",
    "additionalProperties with empty schema should resolve to serde_json::Value"
  );
}

#[test]
fn test_resolve_additional_properties_type_boolean_schema_true() {
  let graph = create_empty_test_graph();
  let resolver = TypeResolverBuilder::default()
    .config(default_config())
    .graph(graph.clone())
    .build()
    .unwrap();

  let additional = oas3::spec::Schema::Boolean(oas3::spec::BooleanSchema(true));

  let result = resolver.resolve_additional_properties_type(&additional).unwrap();
  assert_eq!(
    result.to_rust_type(),
    "serde_json::Value",
    "additionalProperties: true should resolve to serde_json::Value"
  );
}

#[test]
fn test_resolve_additional_properties_type_boolean_schema_false() {
  let graph = create_empty_test_graph();
  let resolver = TypeResolverBuilder::default()
    .config(default_config())
    .graph(graph.clone())
    .build()
    .unwrap();

  let additional = oas3::spec::Schema::Boolean(oas3::spec::BooleanSchema(false));

  let result = resolver.resolve_additional_properties_type(&additional).unwrap();
  assert_eq!(
    result.to_rust_type(),
    "serde_json::Value",
    "additionalProperties: false should resolve to serde_json::Value (though it typically means deny unknown fields)"
  );
}

#[test]
fn test_additional_properties_false_not_resolved_as_map() {
  let graph = create_empty_test_graph();
  let resolver = TypeResolverBuilder::default()
    .config(default_config())
    .graph(graph.clone())
    .build()
    .unwrap();

  let schema = ObjectSchema {
    schema_type: Some(SchemaTypeSet::Single(SchemaType::Object)),
    additional_properties: Some(oas3::spec::Schema::Boolean(oas3::spec::BooleanSchema(false))),
    ..Default::default()
  };

  let result = resolver.resolve_type(&schema).unwrap();
  assert_eq!(
    result.to_rust_type(),
    "serde_json::Value",
    "object with additionalProperties: false should NOT resolve to HashMap (it means no additional properties allowed)"
  );
}

#[allow(clippy::approx_constant)]
#[test]
fn test_const_value_type_inference() {
  let graph = create_empty_test_graph();
  let resolver = TypeResolverBuilder::default()
    .config(default_config())
    .graph(graph.clone())
    .build()
    .unwrap();

  let string_const = ObjectSchema {
    const_value: Some(json!("thought")),
    ..Default::default()
  };
  let result = resolver.resolve_type(&string_const).unwrap();
  assert_eq!(
    result.to_rust_type(),
    "String",
    "const: \"thought\" should infer String type"
  );

  let integer_const = ObjectSchema {
    const_value: Some(json!(42)),
    ..Default::default()
  };
  let result = resolver.resolve_type(&integer_const).unwrap();
  assert_eq!(result.to_rust_type(), "i64", "const: 42 should infer i64 type");

  let float_const = ObjectSchema {
    const_value: Some(json!(3.14)),
    ..Default::default()
  };
  let result = resolver.resolve_type(&float_const).unwrap();
  assert_eq!(result.to_rust_type(), "f64", "const: 3.14 should infer f64 type");

  let bool_const = ObjectSchema {
    const_value: Some(json!(true)),
    ..Default::default()
  };
  let result = resolver.resolve_type(&bool_const).unwrap();
  assert_eq!(result.to_rust_type(), "bool", "const: true should infer bool type");

  let object_const = ObjectSchema {
    const_value: Some(json!({"key": "value"})),
    ..Default::default()
  };
  let result = resolver.resolve_type(&object_const).unwrap();
  assert_eq!(
    result.to_rust_type(),
    "serde_json::Value",
    "const with object value should fall back to serde_json::Value"
  );
}

#[test]
fn test_array_with_union_items_not_treated_as_primitive() {
  let text_content = make_object_schema_with_property("text", make_string_schema());
  let image_content = make_object_schema_with_property("data", make_string_schema());

  let graph = create_test_graph(BTreeMap::from([
    ("TextContent".to_string(), text_content),
    ("ImageContent".to_string(), image_content),
    (
      "ThoughtSummary".to_string(),
      ObjectSchema {
        description: Some("A summary of the thought.".to_string()),
        schema_type: Some(SchemaTypeSet::Single(SchemaType::Array)),
        items: Some(Box::new(oas3::spec::Schema::Object(Box::new(
          ObjectOrReference::Object(ObjectSchema {
            one_of: vec![
              ObjectOrReference::Ref {
                ref_path: "#/components/schemas/TextContent".to_string(),
                summary: None,
                description: None,
              },
              ObjectOrReference::Ref {
                ref_path: "#/components/schemas/ImageContent".to_string(),
                summary: None,
                description: None,
              },
            ],
            ..Default::default()
          }),
        )))),
        ..Default::default()
      },
    ),
  ]));

  let resolver = TypeResolverBuilder::default()
    .config(default_config())
    .graph(graph.clone())
    .build()
    .unwrap();

  let thought_summary_ref = ObjectOrReference::Ref {
    ref_path: "#/components/schemas/ThoughtSummary".to_string(),
    summary: None,
    description: None,
  };
  let thought_summary_schema = graph.get("ThoughtSummary").unwrap();

  let result = resolver
    .resolve_property_type(
      "ThoughtContent",
      "summary",
      thought_summary_schema,
      &thought_summary_ref,
      None,
    )
    .unwrap();

  assert_eq!(
    result.result.to_rust_type(),
    "ThoughtSummary",
    "reference to array with union items should use the named type, not inline Vec<serde_json::Value>"
  );
  assert!(
    result.inline_types.is_empty(),
    "should not generate inline types for named schema reference"
  );
}

#[test]
fn test_string_enum_reference_preserves_named_type() {
  let pet_status_schema = ObjectSchema {
    schema_type: Some(SchemaTypeSet::Single(SchemaType::String)),
    enum_values: vec![json!("available"), json!("pending"), json!("sold")],
    ..Default::default()
  };

  let graph = create_test_graph(BTreeMap::from([("PetStatus".to_string(), pet_status_schema)]));

  let resolver = TypeResolverBuilder::default()
    .config(default_config())
    .graph(graph.clone())
    .build()
    .unwrap();

  let pet_status_ref = ObjectOrReference::Ref {
    ref_path: "#/components/schemas/PetStatus".to_string(),
    summary: None,
    description: None,
  };
  let pet_status_schema = graph.get("PetStatus").unwrap();

  let result = resolver
    .resolve_property_type("Pet", "status", pet_status_schema, &pet_status_ref, None)
    .unwrap();

  assert_eq!(
    result.result.to_rust_type(),
    "PetStatus",
    "reference to string enum should preserve the named type, not collapse to String"
  );
  assert!(
    result.inline_types.is_empty(),
    "should not generate inline types for named enum reference"
  );
}

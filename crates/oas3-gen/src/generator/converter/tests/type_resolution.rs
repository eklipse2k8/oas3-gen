use std::collections::BTreeMap;

use oas3::spec::{BooleanSchema, ObjectOrReference, ObjectSchema, Schema, SchemaType, SchemaTypeSet};
use serde_json::json;

use crate::{
  generator::{ast::RustType, converter::type_resolver::TypeResolver},
  tests::common::{
    create_empty_test_graph, create_schema_converter, create_test_context, create_test_graph, default_config,
    make_object_schema_with_property, make_string_schema,
  },
};

fn null_schema() -> ObjectSchema {
  ObjectSchema {
    schema_type: Some(SchemaTypeSet::Single(SchemaType::Null)),
    ..Default::default()
  }
}

fn int_schema() -> ObjectSchema {
  ObjectSchema {
    schema_type: Some(SchemaTypeSet::Single(SchemaType::Integer)),
    ..Default::default()
  }
}

fn make_ref(name: &str) -> ObjectOrReference<ObjectSchema> {
  ObjectOrReference::Ref {
    ref_path: format!("#/components/schemas/{name}"),
    summary: None,
    description: None,
  }
}

#[test]
fn title_resolution() {
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
    let context = create_test_context(graph.clone(), default_config());
    let resolver = TypeResolver::new(context);

    let result = resolver.resolve_type(&schema).unwrap();
    assert_eq!(result.to_rust_type(), expected_type, "failed for case: {case_name}");
  }
}

#[test]
fn union_to_type_ref_conversion() {
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
  let context = create_test_context(graph.clone(), default_config());
  let resolver = TypeResolver::new(context);

  let inner_schema = ObjectSchema {
    one_of: vec![make_ref("CacheControlEphemeral")],
    ..Default::default()
  };

  let cases = [
    (
      "nested_oneof_resolves_to_ref",
      vec![
        ObjectOrReference::Object(inner_schema),
        ObjectOrReference::Object(null_schema()),
      ],
      Some("Option<CacheControlEphemeral>"),
    ),
    (
      "no_resolvable_variants",
      vec![
        ObjectOrReference::Object(null_schema()),
        ObjectOrReference::Object(null_schema()),
      ],
      None,
    ),
  ];

  for (case_name, variants, expected_type) in cases {
    let result = resolver.try_union(&variants).unwrap();
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
fn array_type_resolution() {
  let custom_schema = make_object_schema_with_property("field", make_string_schema());
  let graph = create_test_graph(BTreeMap::from([("CustomType".to_string(), custom_schema)]));
  let context = create_test_context(graph.clone(), default_config());
  let resolver = TypeResolver::new(context);

  let cases: Vec<(&str, ObjectSchema, &str)> = vec![
    (
      "array_with_inline_string_items",
      ObjectSchema {
        schema_type: Some(SchemaTypeSet::Single(SchemaType::Array)),
        items: Some(Box::new(Schema::Object(Box::new(ObjectOrReference::Object(
          make_string_schema(),
        ))))),
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
        items: Some(Box::new(Schema::Boolean(BooleanSchema(true)))),
        ..Default::default()
      },
      "Vec<serde_json::Value>",
    ),
    (
      "array_with_ref_items",
      ObjectSchema {
        schema_type: Some(SchemaTypeSet::Single(SchemaType::Array)),
        items: Some(Box::new(Schema::Object(Box::new(make_ref("CustomType"))))),
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
fn primitive_type_resolution() {
  let graph = create_empty_test_graph();
  let context = create_test_context(graph.clone(), default_config());
  let resolver = TypeResolver::new(context);
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
      "int64_format",
      ObjectSchema {
        schema_type: Some(SchemaTypeSet::Single(SchemaType::Integer)),
        format: Some("int64".to_string()),
        ..Default::default()
      },
      "i64",
    ),
    (
      "integer_without_format",
      ObjectSchema {
        schema_type: Some(SchemaTypeSet::Single(SchemaType::Integer)),
        ..Default::default()
      },
      "i64",
    ),
    (
      "float_format",
      ObjectSchema {
        schema_type: Some(SchemaTypeSet::Single(SchemaType::Number)),
        format: Some("float".to_string()),
        ..Default::default()
      },
      "f32",
    ),
    (
      "double_format",
      ObjectSchema {
        schema_type: Some(SchemaTypeSet::Single(SchemaType::Number)),
        format: Some("double".to_string()),
        ..Default::default()
      },
      "f64",
    ),
    (
      "number_without_format",
      ObjectSchema {
        schema_type: Some(SchemaTypeSet::Single(SchemaType::Number)),
        ..Default::default()
      },
      "f64",
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
        items: Some(Box::new(Schema::Object(Box::new(ObjectOrReference::Object(
          item_schema,
        ))))),
        ..ObjectSchema::default()
      },
      "Vec<String>",
    ),
    (
      "date_time_format",
      ObjectSchema {
        schema_type: Some(SchemaTypeSet::Single(SchemaType::String)),
        format: Some("date-time".to_string()),
        ..Default::default()
      },
      "chrono::DateTime<chrono::Utc>",
    ),
    (
      "date_format",
      ObjectSchema {
        schema_type: Some(SchemaTypeSet::Single(SchemaType::String)),
        format: Some("date".to_string()),
        ..Default::default()
      },
      "chrono::NaiveDate",
    ),
    (
      "uuid_format",
      ObjectSchema {
        schema_type: Some(SchemaTypeSet::Single(SchemaType::String)),
        format: Some("uuid".to_string()),
        ..Default::default()
      },
      "uuid::Uuid",
    ),
    (
      "uri_format_unsupported",
      ObjectSchema {
        schema_type: Some(SchemaTypeSet::Single(SchemaType::String)),
        format: Some("uri".to_string()),
        ..Default::default()
      },
      "String",
    ),
    (
      "byte_format",
      ObjectSchema {
        schema_type: Some(SchemaTypeSet::Single(SchemaType::String)),
        format: Some("byte".to_string()),
        ..Default::default()
      },
      "Vec<u8>",
    ),
    (
      "boolean_type",
      ObjectSchema {
        schema_type: Some(SchemaTypeSet::Single(SchemaType::Boolean)),
        ..Default::default()
      },
      "bool",
    ),
    (
      "null_type",
      ObjectSchema {
        schema_type: Some(SchemaTypeSet::Single(SchemaType::Null)),
        ..Default::default()
      },
      "Option<()>",
    ),
  ];

  for (case_name, schema, expected_type) in cases {
    let result = resolver.resolve_type(&schema).unwrap();
    assert_eq!(result.to_rust_type(), expected_type, "failed for case: {case_name}");
  }
}

#[test]
fn array_with_union_items_inline_generation() {
  let tool_schema = make_object_schema_with_property("name", make_string_schema());
  let bash_tool_schema = make_object_schema_with_property("command", make_string_schema());
  let type_a = make_object_schema_with_property("field_a", make_string_schema());
  let type_b = make_object_schema_with_property("field_b", int_schema());
  let item_schema = make_object_schema_with_property("id", make_string_schema());

  let graph = create_test_graph(BTreeMap::from([
    ("Tool".to_string(), tool_schema),
    ("BashTool".to_string(), bash_tool_schema),
    ("TypeA".to_string(), type_a),
    ("TypeB".to_string(), type_b),
    ("Item".to_string(), item_schema),
  ]));
  let context = create_test_context(graph.clone(), default_config());
  let resolver = TypeResolver::new(context.clone());

  let oneof_array_schema = ObjectSchema {
    schema_type: Some(SchemaTypeSet::Single(SchemaType::Array)),
    items: Some(Box::new(Schema::Object(Box::new(ObjectOrReference::Object(
      ObjectSchema {
        one_of: vec![make_ref("Tool"), make_ref("BashTool")],
        ..Default::default()
      },
    ))))),
    ..Default::default()
  };

  let anyof_array_schema = ObjectSchema {
    schema_type: Some(SchemaTypeSet::Single(SchemaType::Array)),
    items: Some(Box::new(Schema::Object(Box::new(ObjectOrReference::Object(
      ObjectSchema {
        any_of: vec![make_ref("TypeA"), make_ref("TypeB")],
        ..Default::default()
      },
    ))))),
    ..Default::default()
  };

  let ref_array_schema = ObjectSchema {
    schema_type: Some(SchemaTypeSet::Single(SchemaType::Array)),
    items: Some(Box::new(Schema::Object(Box::new(make_ref("Item"))))),
    ..Default::default()
  };

  let oneof_result = resolver
    .resolve_property(
      "CreateMessageParams",
      "tools",
      &oneof_array_schema,
      &ObjectOrReference::Object(oneof_array_schema.clone()),
    )
    .unwrap();
  assert_eq!(
    oneof_result.result.to_rust_type(),
    "Vec<ToolKind>",
    "oneOf array type mismatch"
  );

  let generated_types = context.cache.borrow().types.types.clone();
  assert_eq!(generated_types.len(), 1, "oneOf should generate one inline enum");

  let inline_type = &generated_types[0];
  match inline_type {
    RustType::Enum(enum_def) => {
      assert_eq!(enum_def.name.as_str(), "ToolKind");
      assert_eq!(enum_def.variants.len(), 2);
      let variant_names = enum_def.variants.iter().map(|v| v.name.as_str()).collect::<Vec<_>>();
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
    .resolve_property(
      "Response",
      "items",
      &anyof_array_schema,
      &ObjectOrReference::Object(anyof_array_schema.clone()),
    )
    .unwrap();
  assert_eq!(
    anyof_result.result.to_rust_type(),
    "Vec<TypeItemKind>",
    "anyOf array type mismatch"
  );

  {
    let generated_types_after_anyof = &context.cache.borrow().types.types;
    assert_eq!(
      generated_types_after_anyof.len(),
      2,
      "anyOf should generate additional inline types"
    );
  }

  let ref_result = resolver
    .resolve_property(
      "Parent",
      "items",
      &ref_array_schema,
      &ObjectOrReference::Object(ref_array_schema.clone()),
    )
    .unwrap();
  assert_eq!(ref_result.result.to_rust_type(), "Vec<Item>", "ref array type mismatch");

  {
    let generated_types_after_ref = &context.cache.borrow().types.types;
    assert_eq!(
      generated_types_after_ref.len(),
      2,
      "ref items should not generate additional inline types"
    );
  }
}

#[test]
fn multi_ref_oneof_returns_none_for_fallback() {
  let type_a = make_object_schema_with_property("field_a", make_string_schema());
  let type_b = make_object_schema_with_property("field_b", make_string_schema());
  let type_c = make_object_schema_with_property("field_c", make_string_schema());

  let graph = create_test_graph(BTreeMap::from([
    ("TypeA".to_string(), type_a),
    ("TypeB".to_string(), type_b),
    ("TypeC".to_string(), type_c),
  ]));
  let context = create_test_context(graph.clone(), default_config());
  let resolver = TypeResolver::new(context);

  let multi_ref_variants = vec![make_ref("TypeA"), make_ref("TypeB"), make_ref("TypeC")];

  let result = resolver.try_union(&multi_ref_variants).unwrap();
  assert!(
    result.is_none(),
    "multi-ref oneOf should return None to trigger enum generation, got: {:?}",
    result.map(|r| r.to_rust_type())
  );

  let single_ref_with_null = vec![make_ref("TypeA"), ObjectOrReference::Object(null_schema())];

  let result = resolver.try_union(&single_ref_with_null).unwrap();
  assert!(result.is_some(), "single ref with null should collapse to Option<T>");
  assert_eq!(
    result.unwrap().to_rust_type(),
    "Option<TypeA>",
    "should be Option<TypeA>"
  );
}

#[test]
fn union_naming_with_common_suffix() {
  let citation_a = make_object_schema_with_property("type", make_string_schema());
  let citation_b = make_object_schema_with_property("url", make_string_schema());
  let citation_c = make_object_schema_with_property("file", make_string_schema());

  let graph = create_test_graph(BTreeMap::from([
    ("BetaResponseCharLocationCitation".to_string(), citation_a),
    ("BetaResponseUrlCitation".to_string(), citation_b),
    ("BetaResponseFileCitation".to_string(), citation_c),
  ]));
  let context = create_test_context(graph.clone(), default_config());
  let resolver = TypeResolver::new(context.clone());

  let union_schema = ObjectSchema {
    one_of: vec![
      make_ref("BetaResponseCharLocationCitation"),
      make_ref("BetaResponseUrlCitation"),
      make_ref("BetaResponseFileCitation"),
    ],
    ..Default::default()
  };

  let result = resolver
    .resolve_property(
      "BetaResponse",
      "citation",
      &union_schema,
      &ObjectOrReference::Object(union_schema.clone()),
    )
    .unwrap();

  assert_eq!(result.result.to_rust_type(), "BetaCitationKind");

  let generated_types = &context.cache.borrow().types.types;
  assert_eq!(generated_types.len(), 1);
  if let RustType::Enum(enum_def) = &generated_types[0] {
    assert_eq!(enum_def.name.as_str(), "BetaCitationKind");
  } else {
    panic!("Expected enum type");
  }
}

#[test]
fn union_naming_without_common_suffix() {
  let tool_a = make_object_schema_with_property("name", make_string_schema());
  let tool_b = make_object_schema_with_property("command", make_string_schema());

  let graph = create_test_graph(BTreeMap::from([
    ("BetaTool".to_string(), tool_a),
    ("BetaBashTool20241022".to_string(), tool_b),
  ]));
  let context = create_test_context(graph.clone(), default_config());
  let resolver = TypeResolver::new(context.clone());

  let union_schema = ObjectSchema {
    one_of: vec![make_ref("BetaTool"), make_ref("BetaBashTool20241022")],
    ..Default::default()
  };

  let result = resolver
    .resolve_property(
      "Request",
      "tool",
      &union_schema,
      &ObjectOrReference::Object(union_schema.clone()),
    )
    .unwrap();

  assert_eq!(result.result.to_rust_type(), "BetaToolKind");

  let generated_types = &context.cache.borrow().types.types;
  assert_eq!(generated_types.len(), 1);
  if let RustType::Enum(enum_def) = &generated_types[0] {
    assert_eq!(enum_def.name.as_str(), "BetaToolKind");
  } else {
    panic!("Expected enum type");
  }
}

#[test]
fn array_union_naming_with_common_suffix() {
  let event_a = make_object_schema_with_property("started", make_string_schema());
  let event_b = make_object_schema_with_property("data", make_string_schema());
  let event_c = make_object_schema_with_property("stopped", make_string_schema());

  let graph = create_test_graph(BTreeMap::from([
    ("BetaMessageStartEvent".to_string(), event_a),
    ("BetaMessageDeltaEvent".to_string(), event_b),
    ("BetaMessageStopEvent".to_string(), event_c),
  ]));
  let context = create_test_context(graph.clone(), default_config());
  let resolver = TypeResolver::new(context.clone());

  let array_schema = ObjectSchema {
    schema_type: Some(SchemaTypeSet::Single(SchemaType::Array)),
    items: Some(Box::new(Schema::Object(Box::new(ObjectOrReference::Object(
      ObjectSchema {
        one_of: vec![
          make_ref("BetaMessageStartEvent"),
          make_ref("BetaMessageDeltaEvent"),
          make_ref("BetaMessageStopEvent"),
        ],
        ..Default::default()
      },
    ))))),
    ..Default::default()
  };

  let result = resolver
    .resolve_property(
      "Stream",
      "events",
      &array_schema,
      &ObjectOrReference::Object(array_schema.clone()),
    )
    .unwrap();

  assert_eq!(result.result.to_rust_type(), "Vec<BetaEventKind>");

  let generated_types = context.cache.borrow().types.types.clone();
  assert_eq!(generated_types.len(), 1);
  if let RustType::Enum(enum_def) = &generated_types[0] {
    assert_eq!(enum_def.name.as_str(), "BetaEventKind");
  } else {
    panic!("Expected enum type");
  }
}

#[test]
fn additional_properties_type_resolution() {
  let custom_schema = make_object_schema_with_property("field", make_string_schema());
  let graph = create_test_graph(BTreeMap::from([("CustomType".to_string(), custom_schema)]));
  let context = create_test_context(graph.clone(), default_config());
  let resolver = TypeResolver::new(context);

  let cases: Vec<(&str, ObjectSchema, &str)> = vec![
    (
      "boolean_value_type",
      ObjectSchema {
        schema_type: Some(SchemaTypeSet::Single(SchemaType::Object)),
        additional_properties: Some(Schema::Object(Box::new(ObjectOrReference::Object(ObjectSchema {
          schema_type: Some(SchemaTypeSet::Single(SchemaType::Boolean)),
          ..Default::default()
        })))),
        ..Default::default()
      },
      "std::collections::HashMap<String, bool>",
    ),
    (
      "string_value_type",
      ObjectSchema {
        schema_type: Some(SchemaTypeSet::Single(SchemaType::Object)),
        additional_properties: Some(Schema::Object(Box::new(ObjectOrReference::Object(ObjectSchema {
          schema_type: Some(SchemaTypeSet::Single(SchemaType::String)),
          ..Default::default()
        })))),
        ..Default::default()
      },
      "std::collections::HashMap<String, String>",
    ),
    (
      "integer_value_type",
      ObjectSchema {
        schema_type: Some(SchemaTypeSet::Single(SchemaType::Object)),
        additional_properties: Some(Schema::Object(Box::new(ObjectOrReference::Object(ObjectSchema {
          schema_type: Some(SchemaTypeSet::Single(SchemaType::Integer)),
          ..Default::default()
        })))),
        ..Default::default()
      },
      "std::collections::HashMap<String, i64>",
    ),
    (
      "ref_value_type",
      ObjectSchema {
        schema_type: Some(SchemaTypeSet::Single(SchemaType::Object)),
        additional_properties: Some(Schema::Object(Box::new(make_ref("CustomType")))),
        ..Default::default()
      },
      "std::collections::HashMap<String, CustomType>",
    ),
    (
      "empty_schema_value_type",
      ObjectSchema {
        schema_type: Some(SchemaTypeSet::Single(SchemaType::Object)),
        additional_properties: Some(Schema::Object(Box::new(ObjectOrReference::Object(
          ObjectSchema::default(),
        )))),
        ..Default::default()
      },
      "std::collections::HashMap<String, serde_json::Value>",
    ),
    (
      "boolean_true",
      ObjectSchema {
        schema_type: Some(SchemaTypeSet::Single(SchemaType::Object)),
        additional_properties: Some(Schema::Boolean(BooleanSchema(true))),
        ..Default::default()
      },
      "std::collections::HashMap<String, serde_json::Value>",
    ),
    (
      "boolean_false_not_map",
      ObjectSchema {
        schema_type: Some(SchemaTypeSet::Single(SchemaType::Object)),
        additional_properties: Some(Schema::Boolean(BooleanSchema(false))),
        ..Default::default()
      },
      "serde_json::Value",
    ),
  ];

  for (case_name, schema, expected_type) in cases {
    let result = resolver.resolve_type(&schema).unwrap();
    assert_eq!(result.to_rust_type(), expected_type, "failed for case: {case_name}");
  }

  let additional = Schema::Boolean(BooleanSchema(false));
  let result = resolver.additional_properties_type(&additional).unwrap();
  assert_eq!(
    result.to_rust_type(),
    "serde_json::Value",
    "additionalProperties: false should resolve to serde_json::Value"
  );
}

#[allow(clippy::approx_constant)]
#[test]
fn const_value_type_inference() {
  let graph = create_empty_test_graph();
  let context = create_test_context(graph.clone(), default_config());
  let resolver = TypeResolver::new(context);

  let cases: Vec<(&str, ObjectSchema, &str)> = vec![
    (
      "string_const",
      ObjectSchema {
        const_value: Some(json!("thought")),
        ..Default::default()
      },
      "String",
    ),
    (
      "integer_const",
      ObjectSchema {
        const_value: Some(json!(42)),
        ..Default::default()
      },
      "i64",
    ),
    (
      "float_const",
      ObjectSchema {
        const_value: Some(json!(3.14)),
        ..Default::default()
      },
      "f64",
    ),
    (
      "bool_const",
      ObjectSchema {
        const_value: Some(json!(true)),
        ..Default::default()
      },
      "bool",
    ),
    (
      "object_const_fallback",
      ObjectSchema {
        const_value: Some(json!({"key": "value"})),
        ..Default::default()
      },
      "serde_json::Value",
    ),
  ];

  for (case_name, schema, expected_type) in cases {
    let result = resolver.resolve_type(&schema).unwrap();
    assert_eq!(result.to_rust_type(), expected_type, "failed for case: {case_name}");
  }
}

#[test]
fn array_with_union_items_not_treated_as_primitive() {
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
        items: Some(Box::new(Schema::Object(Box::new(ObjectOrReference::Object(
          ObjectSchema {
            one_of: vec![make_ref("TextContent"), make_ref("ImageContent")],
            ..Default::default()
          },
        ))))),
        ..Default::default()
      },
    ),
  ]));

  let context = create_test_context(graph.clone(), default_config());
  let resolver = TypeResolver::new(context.clone());
  let thought_summary_ref = make_ref("ThoughtSummary");
  let thought_summary_schema = graph.get("ThoughtSummary").unwrap();

  let result = resolver
    .resolve_property(
      "ThoughtContent",
      "summary",
      thought_summary_schema,
      &thought_summary_ref,
    )
    .unwrap();

  assert_eq!(
    result.result.to_rust_type(),
    "ThoughtSummary",
    "reference to array with union items should use the named type"
  );
  assert!(
    result.inline_types.is_empty(),
    "should not generate inline types for named schema reference"
  );
}

#[test]
fn string_enum_reference_preserves_named_type() {
  let pet_status_schema = ObjectSchema {
    schema_type: Some(SchemaTypeSet::Single(SchemaType::String)),
    enum_values: vec![json!("available"), json!("pending"), json!("sold")],
    ..Default::default()
  };

  let graph = create_test_graph(BTreeMap::from([("PetStatus".to_string(), pet_status_schema)]));

  let context = create_test_context(graph.clone(), default_config());
  let resolver = TypeResolver::new(context.clone());
  let pet_status_ref = make_ref("PetStatus");
  let pet_status_schema = graph.get("PetStatus").unwrap();

  let result = resolver
    .resolve_property("Pet", "status", pet_status_schema, &pet_status_ref)
    .unwrap();

  assert_eq!(
    result.result.to_rust_type(),
    "PetStatus",
    "reference to string enum should preserve the named type"
  );
  assert!(
    result.inline_types.is_empty(),
    "should not generate inline types for named enum reference"
  );
}

#[test]
fn try_nullable_union_edge_cases() {
  let type_a = make_object_schema_with_property("field_a", make_string_schema());
  let type_b = make_object_schema_with_property("field_b", make_string_schema());
  let type_c = make_object_schema_with_property("field_c", make_string_schema());

  let graph = create_test_graph(BTreeMap::from([
    ("TypeA".to_string(), type_a),
    ("TypeB".to_string(), type_b),
    ("TypeC".to_string(), type_c),
  ]));
  let context = create_test_context(graph.clone(), default_config());
  let resolver = TypeResolver::new(context);

  let three_variants_with_null = vec![
    make_ref("TypeA"),
    make_ref("TypeB"),
    ObjectOrReference::Object(null_schema()),
  ];
  let result = resolver.try_union(&three_variants_with_null).unwrap();
  assert!(
    result.is_none(),
    "3 variants with null should not collapse to Option<T>"
  );

  let two_refs_no_null = vec![make_ref("TypeA"), make_ref("TypeB")];
  let result = resolver.try_union(&two_refs_no_null).unwrap();
  assert!(result.is_none(), "2 refs without null should not collapse to Option<T>");

  let nullable_object = ObjectSchema {
    schema_type: Some(SchemaTypeSet::Multiple(vec![SchemaType::Object, SchemaType::Null])),
    ..Default::default()
  };
  let two_nullable_objects = vec![
    ObjectOrReference::Object(nullable_object.clone()),
    ObjectOrReference::Object(null_schema()),
  ];
  let result = resolver.try_union(&two_nullable_objects).unwrap();
  assert!(
    result.is_none(),
    "two nullable objects should not have a non-null variant"
  );

  let inline_string = make_string_schema();
  let inline_with_null = vec![
    ObjectOrReference::Object(inline_string),
    ObjectOrReference::Object(null_schema()),
  ];
  let result = resolver.try_union(&inline_with_null).unwrap();
  assert!(
    result.is_some(),
    "inline string with null should collapse to Option<String>"
  );
  assert_eq!(
    result.unwrap().to_rust_type(),
    "Option<String>",
    "should be Option<String>"
  );
}

#[test]
fn is_wrapper_union() {
  let type_a = make_object_schema_with_property("field_a", make_string_schema());
  let type_b = make_object_schema_with_property("field_b", make_string_schema());

  let graph = create_test_graph(BTreeMap::from([
    ("TypeA".to_string(), type_a),
    ("TypeB".to_string(), type_b),
  ]));
  let context = create_test_context(graph.clone(), default_config());
  let resolver = TypeResolver::new(context);

  let wrapper_with_ref_and_null = ObjectSchema {
    one_of: vec![make_ref("TypeA"), ObjectOrReference::Object(null_schema())],
    ..Default::default()
  };
  let result = resolver.try_union(&wrapper_with_ref_and_null.one_of).unwrap();
  assert!(result.is_some(), "single ref + null should be a wrapper union");
  assert_eq!(result.unwrap().to_rust_type(), "Option<TypeA>");

  let wrapper_with_string_and_null = ObjectSchema {
    one_of: vec![
      ObjectOrReference::Object(make_string_schema()),
      ObjectOrReference::Object(null_schema()),
    ],
    ..Default::default()
  };
  let result = resolver.try_union(&wrapper_with_string_and_null.one_of).unwrap();
  assert!(result.is_some(), "single string + null should be a wrapper union");
  assert_eq!(result.unwrap().to_rust_type(), "Option<String>");

  let two_refs = ObjectSchema {
    one_of: vec![make_ref("TypeA"), make_ref("TypeB")],
    ..Default::default()
  };
  let result = resolver.try_union(&two_refs.one_of).unwrap();
  assert!(result.is_none(), "two refs should not be a wrapper union");
}

#[test]
fn try_flatten_nested_union() {
  let type_a = make_object_schema_with_property("field_a", make_string_schema());
  let type_b = make_object_schema_with_property("field_b", make_string_schema());

  let graph = create_test_graph(BTreeMap::from([
    ("TypeA".to_string(), type_a),
    ("TypeB".to_string(), type_b),
  ]));
  let context = create_test_context(graph.clone(), default_config());
  let converter = create_schema_converter(&context);

  let inner_union = ObjectSchema {
    one_of: vec![make_ref("TypeA"), make_ref("TypeB")],
    ..Default::default()
  };

  let nested_schema = ObjectSchema {
    one_of: vec![
      ObjectOrReference::Object(inner_union),
      ObjectOrReference::Object(null_schema()),
    ],
    ..Default::default()
  };

  let types = converter.convert_schema("NestedUnion", &nested_schema).unwrap();
  assert!(!types.is_empty(), "nested union should generate types");

  let type_names = types
    .iter()
    .map(|t| match t {
      RustType::Enum(e) => e.name.as_str().to_string(),
      RustType::DiscriminatedEnum(e) => e.name.as_str().to_string(),
      _ => "other".to_string(),
    })
    .collect::<Vec<_>>();
  assert!(
    type_names.iter().any(|n| n == "NestedUnion"),
    "should generate NestedUnion type, got: {type_names:?}"
  );
}

#[test]
fn unique_items_flag_preserved() {
  let graph = create_empty_test_graph();
  let context = create_test_context(graph.clone(), default_config());
  let resolver = TypeResolver::new(context);

  let unique_array = ObjectSchema {
    schema_type: Some(SchemaTypeSet::Single(SchemaType::Array)),
    items: Some(Box::new(Schema::Object(Box::new(ObjectOrReference::Object(
      make_string_schema(),
    ))))),
    unique_items: Some(true),
    ..Default::default()
  };

  let result = resolver.resolve_type(&unique_array).unwrap();
  assert!(result.unique_items, "unique_items flag should be preserved in TypeRef");
  assert_eq!(
    result.to_rust_type(),
    "Vec<String>",
    "unique array still generates Vec<> in to_rust_type (codegen handles BTreeSet conversion)"
  );

  let non_unique_array = ObjectSchema {
    schema_type: Some(SchemaTypeSet::Single(SchemaType::Array)),
    items: Some(Box::new(Schema::Object(Box::new(ObjectOrReference::Object(
      make_string_schema(),
    ))))),
    unique_items: Some(false),
    ..Default::default()
  };

  let result = resolver.resolve_type(&non_unique_array).unwrap();
  assert!(!result.unique_items, "unique_items flag should be false when not set");
  assert_eq!(
    result.to_rust_type(),
    "Vec<String>",
    "non-unique array should resolve to Vec"
  );
}

#[test]
fn convert_schema_type_alias() {
  let graph = create_empty_test_graph();
  let context = create_test_context(graph.clone(), default_config());
  let converter = create_schema_converter(&context);

  let string_schema = ObjectSchema {
    schema_type: Some(SchemaTypeSet::Single(SchemaType::String)),
    description: Some("A custom string type".to_string()),
    ..Default::default()
  };

  let types = converter.convert_schema("MyString", &string_schema).unwrap();
  assert_eq!(types.len(), 1, "should generate one type");

  match &types[0] {
    RustType::TypeAlias(alias) => {
      assert_eq!(alias.name.as_str(), "MyString");
      assert_eq!(alias.target.to_rust_type(), "String");
    }
    _ => panic!("expected TypeAlias, got {:?}", types[0]),
  }
}

#[test]
fn convert_schema_with_allof() {
  let base_schema = make_object_schema_with_property("base_field", make_string_schema());
  let extended_schema = ObjectSchema {
    all_of: vec![
      make_ref("BaseType"),
      ObjectOrReference::Object(make_object_schema_with_property("extra_field", make_string_schema())),
    ],
    ..Default::default()
  };

  let graph = create_test_graph(BTreeMap::from([
    ("BaseType".to_string(), base_schema),
    ("ExtendedType".to_string(), extended_schema),
  ]));
  let context = create_test_context(graph.clone(), default_config());
  let converter = create_schema_converter(&context);

  let allof_schema = graph.get("ExtendedType").unwrap();

  let types = converter.convert_schema("ExtendedType", allof_schema).unwrap();
  assert!(!types.is_empty(), "allOf schema should generate types");

  let has_struct = types.iter().any(|t| matches!(t, RustType::Struct(_)));
  assert!(has_struct, "allOf should generate a struct");
}

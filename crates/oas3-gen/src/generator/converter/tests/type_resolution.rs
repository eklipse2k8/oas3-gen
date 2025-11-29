use std::{collections::BTreeMap, sync::Arc};

use oas3::spec::{ObjectOrReference, ObjectSchema, SchemaType, SchemaTypeSet};
use serde_json::json;

use crate::{
  generator::{
    converter::{SchemaExt, type_resolver::TypeResolver},
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

  let (graph, _) = SchemaRegistry::new(spec);
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
    let resolver = TypeResolver::new(&graph, default_config());

    let result = resolver.schema_to_type_ref(&schema).unwrap();
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
  let resolver = TypeResolver::new(&graph, default_config());

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
    let result = resolver.try_convert_union_to_type_ref(&variants).unwrap();
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
  let resolver = TypeResolver::new(&graph, default_config());

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
    let result = resolver.schema_to_type_ref(&schema).unwrap();
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
}

#[test]
fn test_basic_type_resolution() {
  let graph = create_empty_test_graph();
  let resolver = TypeResolver::new(&graph, default_config());

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
    let result = resolver.schema_to_type_ref(&schema).unwrap();
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
  let resolver = TypeResolver::new(&graph, default_config());

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
    .resolve_property_type_with_inlines(
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
    crate::generator::ast::RustType::Enum(enum_def) => {
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
    .resolve_property_type_with_inlines(
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
    .resolve_property_type_with_inlines(
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
  let resolver = TypeResolver::new(&graph, default_config());

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

  let result = resolver.try_convert_union_to_type_ref(&multi_ref_variants).unwrap();
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

  let result = resolver.try_convert_union_to_type_ref(&single_ref_with_null).unwrap();
  assert!(result.is_some(), "single ref with null should collapse to Option<T>");
  assert_eq!(
    result.unwrap().to_rust_type(),
    "Option<TypeA>",
    "should be Option<TypeA>"
  );
}

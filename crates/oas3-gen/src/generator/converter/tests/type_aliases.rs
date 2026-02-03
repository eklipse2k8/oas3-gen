use std::collections::BTreeMap;

use oas3::spec::{ObjectOrReference, ObjectSchema, Schema, SchemaType, SchemaTypeSet};

use crate::{
  generator::{ast::RustType, converter::SchemaConverter},
  tests::common::{create_test_context, create_test_graph, default_config},
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
      "TagId",
      ObjectSchema {
        schema_type: Some(SchemaTypeSet::Single(SchemaType::String)),
        ..Default::default()
      },
      "String",
    ),
    (
      "SplootAt",
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
    let context = create_test_context(graph.clone(), default_config());
    let converter = SchemaConverter::new(&context);
    let result = converter.convert_schema(name, graph.get(name).unwrap())?;

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
  let barks_schema = make_array_schema(Some(Schema::Object(Box::new(ObjectOrReference::Object(
    make_string_schema(),
  )))));

  let floof_array_schema = make_array_schema(None);

  let ball_matrix_schema = make_array_schema(Some(Schema::Object(Box::new(ObjectOrReference::Object(
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
    ("Barks", barks_schema, "Vec<String>"),
    ("FloofArray", floof_array_schema, "Vec<serde_json::Value>"),
    ("BallMatrix", ball_matrix_schema, "Vec<Vec<i64>>"),
  ];

  for (name, schema, expected_type) in cases {
    let graph = create_test_graph(BTreeMap::from([(name.to_string(), schema)]));
    let context = create_test_context(graph.clone(), default_config());
    let converter = SchemaConverter::new(&context);
    let result = converter.convert_schema(name, graph.get(name).unwrap())?;

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
  let corgi_schema = ObjectSchema {
    schema_type: Some(SchemaTypeSet::Single(SchemaType::Object)),
    properties: BTreeMap::from([
      (
        "tag_id".to_string(),
        ObjectOrReference::Object(ObjectSchema {
          schema_type: Some(SchemaTypeSet::Single(SchemaType::Integer)),
          ..Default::default()
        }),
      ),
      ("name".to_string(), ObjectOrReference::Object(make_string_schema())),
    ]),
    ..Default::default()
  };

  let corgis_schema_array = make_array_schema(Some(Schema::Object(Box::new(ObjectOrReference::Ref {
    ref_path: "#/components/schemas/Corgi".to_string(),
    summary: None,
    description: None,
  }))));

  let graph = create_test_graph(BTreeMap::from([
    ("Corgi".to_string(), corgi_schema),
    ("Corgis".to_string(), corgis_schema_array),
  ]));

  let context = create_test_context(graph.clone(), default_config());
  let converter = SchemaConverter::new(&context);
  let result = converter.convert_schema("Corgis", graph.get("Corgis").unwrap())?;

  assert_eq!(result.len(), 1);
  let RustType::TypeAlias(alias) = &result[0] else {
    panic!("expected type alias for array schema")
  };

  assert_eq!(alias.name, "Corgis");
  assert_eq!(alias.target.to_rust_type(), "Vec<Corgi>");
  Ok(())
}

#[test]
fn test_array_type_alias_with_inline_union_items() -> anyhow::Result<()> {
  let bark_frappe = ObjectSchema {
    schema_type: Some(SchemaTypeSet::Single(SchemaType::Object)),
    properties: BTreeMap::from([
      (
        "type".to_string(),
        ObjectOrReference::Object(ObjectSchema {
          schema_type: Some(SchemaTypeSet::Single(SchemaType::String)),
          const_value: Some(serde_json::json!("bark")),
          ..Default::default()
        }),
      ),
      ("message".to_string(), ObjectOrReference::Object(make_string_schema())),
    ]),
    ..Default::default()
  };

  let sploot_frappe = ObjectSchema {
    schema_type: Some(SchemaTypeSet::Single(SchemaType::Object)),
    properties: BTreeMap::from([
      (
        "type".to_string(),
        ObjectOrReference::Object(ObjectSchema {
          schema_type: Some(SchemaTypeSet::Single(SchemaType::String)),
          const_value: Some(serde_json::json!("sploot")),
          ..Default::default()
        }),
      ),
      ("url".to_string(), ObjectOrReference::Object(make_string_schema())),
    ]),
    ..Default::default()
  };

  let frappe_list_schema = ObjectSchema {
    schema_type: Some(SchemaTypeSet::Single(SchemaType::Array)),
    items: Some(Box::new(Schema::Object(Box::new(ObjectOrReference::Object(
      ObjectSchema {
        one_of: vec![
          ObjectOrReference::Ref {
            ref_path: "#/components/schemas/BarkFrappe".to_string(),
            summary: None,
            description: None,
          },
          ObjectOrReference::Ref {
            ref_path: "#/components/schemas/SplootFrappe".to_string(),
            summary: None,
            description: None,
          },
        ],
        ..Default::default()
      },
    ))))),
    ..Default::default()
  };

  let graph = create_test_graph(BTreeMap::from([
    ("BarkFrappe".to_string(), bark_frappe),
    ("SplootFrappe".to_string(), sploot_frappe),
    ("FrappeList".to_string(), frappe_list_schema),
  ]));

  let context = create_test_context(graph.clone(), default_config());
  let converter = SchemaConverter::new(&context);
  let result = converter.convert_schema("FrappeList", graph.get("FrappeList").unwrap())?;

  let mut all_types = result;
  all_types.extend(context.cache.borrow().types.types.clone());

  assert_eq!(all_types.len(), 2, "expected type alias + inline enum");

  let alias = all_types.iter().find_map(|t| match t {
    RustType::TypeAlias(a) => Some(a),
    _ => None,
  });
  assert!(alias.is_some(), "expected a type alias");
  let alias = alias.unwrap();
  assert_eq!(alias.name, "FrappeList");
  assert_eq!(alias.target.to_rust_type(), "Vec<FrappeListKind>");

  let inline_enum = all_types.iter().find_map(|t| match t {
    RustType::Enum(e) => Some(e),
    _ => None,
  });
  assert!(inline_enum.is_some(), "expected inline enum for union items");
  let inline_enum = inline_enum.unwrap();
  assert_eq!(inline_enum.name.as_str(), "FrappeListKind");
  assert_eq!(inline_enum.variants.len(), 2);

  Ok(())
}

#[test]
fn test_nullable_array_type_alias_with_inline_union_items() -> anyhow::Result<()> {
  let bark_frappe = ObjectSchema {
    schema_type: Some(SchemaTypeSet::Single(SchemaType::Object)),
    properties: BTreeMap::from([
      (
        "type".to_string(),
        ObjectOrReference::Object(ObjectSchema {
          schema_type: Some(SchemaTypeSet::Single(SchemaType::String)),
          const_value: Some(serde_json::json!("bark")),
          ..Default::default()
        }),
      ),
      ("message".to_string(), ObjectOrReference::Object(make_string_schema())),
    ]),
    ..Default::default()
  };

  let sploot_frappe = ObjectSchema {
    schema_type: Some(SchemaTypeSet::Single(SchemaType::Object)),
    properties: BTreeMap::from([
      (
        "type".to_string(),
        ObjectOrReference::Object(ObjectSchema {
          schema_type: Some(SchemaTypeSet::Single(SchemaType::String)),
          const_value: Some(serde_json::json!("sploot")),
          ..Default::default()
        }),
      ),
      ("url".to_string(), ObjectOrReference::Object(make_string_schema())),
    ]),
    ..Default::default()
  };

  let nullable_frappe_list_schema = ObjectSchema {
    schema_type: Some(SchemaTypeSet::Multiple(vec![SchemaType::Array, SchemaType::Null])),
    items: Some(Box::new(Schema::Object(Box::new(ObjectOrReference::Object(
      ObjectSchema {
        one_of: vec![
          ObjectOrReference::Ref {
            ref_path: "#/components/schemas/BarkFrappe".to_string(),
            summary: None,
            description: None,
          },
          ObjectOrReference::Ref {
            ref_path: "#/components/schemas/SplootFrappe".to_string(),
            summary: None,
            description: None,
          },
        ],
        ..Default::default()
      },
    ))))),
    ..Default::default()
  };

  let graph = create_test_graph(BTreeMap::from([
    ("BarkFrappe".to_string(), bark_frappe),
    ("SplootFrappe".to_string(), sploot_frappe),
    ("OptionFrappeList".to_string(), nullable_frappe_list_schema),
  ]));

  let context = create_test_context(graph.clone(), default_config());
  let converter = SchemaConverter::new(&context);
  let result = converter.convert_schema("OptionFrappeList", graph.get("OptionFrappeList").unwrap())?;

  let mut all_types = result;
  all_types.extend(context.cache.borrow().types.types.clone());

  assert_eq!(all_types.len(), 2, "expected type alias + inline enum");

  let alias = all_types.iter().find_map(|t| match t {
    RustType::TypeAlias(a) => Some(a),
    _ => None,
  });
  assert!(alias.is_some(), "expected a type alias");
  let alias = alias.unwrap();
  assert_eq!(alias.name, "OptionFrappeList");
  assert_eq!(
    alias.target.to_rust_type(),
    "Option<Vec<OptionFrappeListKind>>",
    "nullable array should be wrapped in Option"
  );

  let inline_enum = all_types.iter().find_map(|t| match t {
    RustType::Enum(e) => Some(e),
    _ => None,
  });
  assert!(inline_enum.is_some(), "expected inline enum for union items");
  let inline_enum = inline_enum.unwrap();
  assert_eq!(inline_enum.name.as_str(), "OptionFrappeListKind");
  assert_eq!(inline_enum.variants.len(), 2);

  Ok(())
}

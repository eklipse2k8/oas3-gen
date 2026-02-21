use serde_json::json;

use super::support::assert_single_type_alias;
use crate::{
  generator::{ast::RustType, converter::SchemaConverter},
  tests::common::{create_test_context, create_test_graph, default_config, parse_schemas},
};

#[test]
fn test_primitive_type_aliases() -> anyhow::Result<()> {
  let schemas = parse_schemas(vec![
    ("TagId", json!({"type": "string"})),
    ("SplootAt", json!({"type": "integer", "format": "int64"})),
  ]);
  let cases = [("TagId", "String"), ("SplootAt", "i64")];
  for (name, expected_type) in cases {
    let graph = create_test_graph(schemas.clone());
    let context = create_test_context(graph.clone(), default_config());
    let converter = SchemaConverter::new(&context);
    let result = converter.convert_schema(name, graph.get(name).unwrap())?;
    assert_single_type_alias(&result, name, expected_type);
  }
  Ok(())
}

#[test]
fn test_array_type_aliases() -> anyhow::Result<()> {
  let schemas = parse_schemas(vec![
    ("Barks", json!({"type": "array", "items": {"type": "string"}})),
    ("FloofArray", json!({"type": "array"})),
    (
      "BallMatrix",
      json!({"type": "array", "items": {"type": "array", "items": {"type": "integer"}}}),
    ),
  ]);
  let cases = [
    ("Barks", "Vec<String>"),
    ("FloofArray", "Vec<serde_json::Value>"),
    ("BallMatrix", "Vec<Vec<i64>>"),
  ];
  for (name, expected_type) in cases {
    let graph = create_test_graph(schemas.clone());
    let context = create_test_context(graph.clone(), default_config());
    let converter = SchemaConverter::new(&context);
    let result = converter.convert_schema(name, graph.get(name).unwrap())?;
    assert_single_type_alias(&result, name, expected_type);
  }
  Ok(())
}

#[test]
fn test_array_type_alias_with_ref_items() -> anyhow::Result<()> {
  let schemas = parse_schemas(vec![
    (
      "Corgi",
      json!({
        "type": "object",
        "properties": {
          "tag_id": {"type": "integer"},
          "name": {"type": "string"}
        }
      }),
    ),
    (
      "Corgis",
      json!({"type": "array", "items": {"$ref": "#/components/schemas/Corgi"}}),
    ),
  ]);
  let graph = create_test_graph(schemas);
  let context = create_test_context(graph.clone(), default_config());
  let converter = SchemaConverter::new(&context);
  let result = converter.convert_schema("Corgis", graph.get("Corgis").unwrap())?;
  assert_single_type_alias(&result, "Corgis", "Vec<Corgi>");
  Ok(())
}

#[test]
fn test_array_type_alias_with_inline_union_items() -> anyhow::Result<()> {
  let schemas = parse_schemas(vec![
    (
      "BarkFrappe",
      json!({
        "type": "object",
        "properties": {
          "type": {"type": "string", "const": "bark"},
          "message": {"type": "string"}
        }
      }),
    ),
    (
      "SplootFrappe",
      json!({
        "type": "object",
        "properties": {
          "type": {"type": "string", "const": "sploot"},
          "url": {"type": "string"}
        }
      }),
    ),
    (
      "FrappeList",
      json!({
        "type": "array",
        "items": {"oneOf": [{"$ref": "#/components/schemas/BarkFrappe"}, {"$ref": "#/components/schemas/SplootFrappe"}]}
      }),
    ),
  ]);
  let graph = create_test_graph(schemas);
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
  let schemas = parse_schemas(vec![
    (
      "BarkFrappe",
      json!({
        "type": "object",
        "properties": {
          "type": {"type": "string", "const": "bark"},
          "message": {"type": "string"}
        }
      }),
    ),
    (
      "SplootFrappe",
      json!({
        "type": "object",
        "properties": {
          "type": {"type": "string", "const": "sploot"},
          "url": {"type": "string"}
        }
      }),
    ),
    (
      "OptionFrappeList",
      json!({
        "type": ["array", "null"],
        "items": {"oneOf": [{"$ref": "#/components/schemas/BarkFrappe"}, {"$ref": "#/components/schemas/SplootFrappe"}]}
      }),
    ),
  ]);
  let graph = create_test_graph(schemas);
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

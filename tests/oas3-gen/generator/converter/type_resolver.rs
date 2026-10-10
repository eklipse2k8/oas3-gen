use oas3_gen::generator::CollectionTypePolicy;
use rstest::rstest;

use super::{convert, field};

#[rstest]
fn property_schemas_resolve_to_rust_types() {
  let converted = convert(spec!("converter/property_types.json"), config!());
  assert_eq!(
    converted.fields("Holder"),
    [
      "titled: Option<CustomType>",
      "titled_string: Option<String>",
      "float: Option<f32>",
      "null: Option<()>",
      "items_true: Option<Vec<serde_json::Value>>",
      "no_items: Option<Vec<serde_json::Value>>",
      "unique: Option<indexmap::IndexSet<String>>",
      "map: Option<indexmap::IndexMap<String, String>>",
      "map_true: Option<indexmap::IndexMap<String, serde_json::Value>>",
      "map_empty: Option<indexmap::IndexMap<String, serde_json::Value>>",
      "map_false: Option<serde_json::Value>",
      "const_int: Option<i64>",
      "const_float: Option<f64>",
      "const_bool: Option<bool>",
      "const_object: Option<serde_json::Value>",
      "anything: Option<serde_json::Value>",
      "impossible: Option<serde_json::Value>",
      "nested_nullable: Option<Ephemeral>",
      "strings_or_null: Option<Vec<String>>",
      "summary: Option<Summary>",
    ]
  );
}

#[rstest]
fn hashed_collection_policy_emits_std_collections() {
  let converted = convert(
    spec!("converter/property_types.json"),
    config!(collection_types: CollectionTypePolicy::Hashed),
  );
  let collections = converted
    .fields("Holder")
    .into_iter()
    .filter(|field| field.starts_with("unique") || field.starts_with("map"))
    .collect::<Vec<_>>();

  assert_eq!(
    collections,
    [
      "unique: Option<Vec<String>>",
      "map: Option<std::collections::HashMap<String, String>>",
      "map_true: Option<std::collections::HashMap<String, serde_json::Value>>",
      "map_empty: Option<std::collections::HashMap<String, serde_json::Value>>",
      "map_false: Option<serde_json::Value>",
    ]
  );
  assert_eq!(
    field(converted.struct_def("Holder"), "unique").serde_as_attr,
    None,
    "hashed arrays drop the uniqueness check"
  );
}

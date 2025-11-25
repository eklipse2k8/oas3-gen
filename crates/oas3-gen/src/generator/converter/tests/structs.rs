use std::collections::{BTreeMap, HashMap};

use oas3::spec::{BooleanSchema, Discriminator, ObjectOrReference, ObjectSchema, Schema, SchemaType, SchemaTypeSet};
use string_cache::DefaultAtom;

use crate::{
  generator::{
    ast::{FieldDef, RustPrimitive, RustType, SerdeAttribute, TypeRef, ValidationAttribute},
    converter::{
      FieldOptionalityPolicy, SchemaConverter,
      field_optionality::FieldContext,
      metadata::FieldMetadata,
      structs::{DiscriminatorHandler, DiscriminatorInfo, FieldProcessor, SchemaMerger, StructConverter},
    },
  },
  tests::common::{create_test_graph, default_config},
};

#[test]
fn test_discriminated_base_struct_renamed() -> anyhow::Result<()> {
  let mut entity_schema = ObjectSchema {
    schema_type: Some(SchemaTypeSet::Single(SchemaType::Object)),
    additional_properties: Some(Schema::Boolean(BooleanSchema(false))),
    ..Default::default()
  };
  entity_schema.properties.insert(
    "id".to_string(),
    ObjectOrReference::Object(ObjectSchema {
      schema_type: Some(SchemaTypeSet::Single(SchemaType::String)),
      ..Default::default()
    }),
  );
  entity_schema.properties.insert(
    "@odata.type".to_string(),
    ObjectOrReference::Object(ObjectSchema {
      schema_type: Some(SchemaTypeSet::Single(SchemaType::String)),
      ..Default::default()
    }),
  );
  entity_schema.discriminator = Some(Discriminator {
    property_name: "@odata.type".to_string(),
    mapping: Some(BTreeMap::from([(
      "#microsoft.graph.user".to_string(),
      "#/components/schemas/User".to_string(),
    )])),
  });

  let graph = create_test_graph(BTreeMap::from([("Entity".to_string(), entity_schema)]));
  let converter = SchemaConverter::new(&graph, FieldOptionalityPolicy::standard(), default_config());
  let result = converter.convert_schema("Entity", graph.get_schema("Entity").unwrap(), None)?;

  let struct_def = result
    .iter()
    .find_map(|ty| match ty {
      RustType::Struct(def) => Some(def),
      _ => None,
    })
    .expect("Backing struct should be present");

  assert_eq!(struct_def.name, "EntityBase");
  assert!(struct_def.serde_attrs.contains(&SerdeAttribute::DenyUnknownFields));
  Ok(())
}

#[test]
fn test_discriminator_with_enum_remains_visible() -> anyhow::Result<()> {
  let mut message_schema = ObjectSchema {
    schema_type: Some(SchemaTypeSet::Single(SchemaType::Object)),
    additional_properties: Some(Schema::Boolean(BooleanSchema(false))),
    ..Default::default()
  };
  message_schema.properties.insert(
    "role".to_string(),
    ObjectOrReference::Object(ObjectSchema {
      schema_type: Some(SchemaTypeSet::Single(SchemaType::String)),
      enum_values: vec![
        serde_json::Value::String("user".to_string()),
        serde_json::Value::String("assistant".to_string()),
      ],
      ..Default::default()
    }),
  );
  message_schema.properties.insert(
    "content".to_string(),
    ObjectOrReference::Object(ObjectSchema {
      schema_type: Some(SchemaTypeSet::Single(SchemaType::String)),
      ..Default::default()
    }),
  );
  message_schema.required = vec!["role".to_string(), "content".to_string()];
  message_schema.discriminator = Some(Discriminator {
    property_name: "role".to_string(),
    mapping: None,
  });

  let graph = create_test_graph(BTreeMap::from([("Message".to_string(), message_schema)]));
  let converter = SchemaConverter::new(&graph, FieldOptionalityPolicy::standard(), default_config());
  let result = converter.convert_schema("Message", graph.get_schema("Message").unwrap(), None)?;

  let struct_def = result
    .iter()
    .find_map(|ty| match ty {
      RustType::Struct(def) => Some(def),
      _ => None,
    })
    .expect("Struct should be present");

  assert_eq!(struct_def.name, "Message");

  let role_field = struct_def
    .fields
    .iter()
    .find(|f| f.name == "role")
    .expect("role field should exist");

  assert!(
    !role_field.extra_attrs.iter().any(|a| a.contains("doc(hidden)")),
    "role field should not be hidden when discriminator has enum values"
  );
  assert!(
    !role_field
      .serde_attrs
      .iter()
      .any(|a| matches!(a, SerdeAttribute::Skip | SerdeAttribute::SkipDeserializing)),
    "role field should not be skipped when discriminator has enum values"
  );
  assert!(
    !role_field.rust_type.to_rust_type().starts_with("Option<"),
    "role field should be required, not optional"
  );

  Ok(())
}

#[test]
fn test_discriminator_without_enum_is_hidden() -> anyhow::Result<()> {
  let mut entity_schema = ObjectSchema {
    schema_type: Some(SchemaTypeSet::Single(SchemaType::Object)),
    ..Default::default()
  };
  entity_schema.properties.insert(
    "@odata.type".to_string(),
    ObjectOrReference::Object(ObjectSchema {
      schema_type: Some(SchemaTypeSet::Single(SchemaType::String)),
      ..Default::default()
    }),
  );
  entity_schema.properties.insert(
    "id".to_string(),
    ObjectOrReference::Object(ObjectSchema {
      schema_type: Some(SchemaTypeSet::Single(SchemaType::String)),
      ..Default::default()
    }),
  );
  entity_schema.required = vec!["@odata.type".to_string()];
  entity_schema.discriminator = Some(Discriminator {
    property_name: "@odata.type".to_string(),
    mapping: Some(BTreeMap::from([(
      "#microsoft.graph.user".to_string(),
      "#/components/schemas/User".to_string(),
    )])),
  });

  let graph = create_test_graph(BTreeMap::from([("Entity".to_string(), entity_schema)]));
  let converter = SchemaConverter::new(&graph, FieldOptionalityPolicy::standard(), default_config());
  let result = converter.convert_schema("Entity", graph.get_schema("Entity").unwrap(), None)?;

  let struct_def = result
    .iter()
    .find_map(|ty| match ty {
      RustType::Struct(def) if def.name == "EntityBase" => Some(def),
      _ => None,
    })
    .expect("EntityBase struct should be present");

  let odata_field = struct_def
    .fields
    .iter()
    .find(|f| f.name == "odata_type")
    .expect("odata_type field should exist");

  assert!(
    odata_field.extra_attrs.iter().any(|a| a.contains("doc(hidden)")),
    "odata_type field should be hidden"
  );
  assert!(
    odata_field.serde_attrs.contains(&SerdeAttribute::Skip),
    "odata_type field should be skipped"
  );

  Ok(())
}

#[test]
fn test_schema_merger_merge_child_with_parent() {
  let mut parent = ObjectSchema::default();
  parent.properties.insert(
    "parent_prop".to_string(),
    ObjectOrReference::Object(ObjectSchema {
      schema_type: Some(SchemaTypeSet::Single(SchemaType::String)),
      ..Default::default()
    }),
  );
  parent.required.push("parent_prop".to_string());

  let mut child = ObjectSchema::default();
  child.properties.insert(
    "child_prop".to_string(),
    ObjectOrReference::Object(ObjectSchema {
      schema_type: Some(SchemaTypeSet::Single(SchemaType::Integer)),
      ..Default::default()
    }),
  );
  child.all_of.push(ObjectOrReference::Ref {
    ref_path: "#/components/schemas/Parent".to_string(),
    summary: None,
    description: None,
  });

  let mut graph_map = BTreeMap::new();
  graph_map.insert("Parent".to_string(), parent.clone());
  graph_map.insert("Child".to_string(), child.clone());

  let graph = create_test_graph(graph_map);
  let merger = SchemaMerger::new(graph);
  let merged_schema = merger.merge_child_schema_with_parent(&child, &parent).unwrap();

  assert!(merged_schema.properties.contains_key("parent_prop"));
  assert!(merged_schema.properties.contains_key("child_prop"));
  assert!(merged_schema.required.contains(&"parent_prop".to_string()));
}

#[test]
fn test_schema_merger_conflict_resolution() {
  let mut parent = ObjectSchema::default();
  parent.properties.insert(
    "prop".to_string(),
    ObjectOrReference::Object(ObjectSchema {
      schema_type: Some(SchemaTypeSet::Single(SchemaType::String)),
      ..Default::default()
    }),
  );

  let mut child = ObjectSchema::default();
  child.properties.insert(
    "prop".to_string(),
    ObjectOrReference::Object(ObjectSchema {
      schema_type: Some(SchemaTypeSet::Single(SchemaType::Integer)),
      ..Default::default()
    }),
  );
  child.all_of.push(ObjectOrReference::Ref {
    ref_path: "#/components/schemas/Parent".to_string(),
    summary: None,
    description: None,
  });

  let mut graph_map = BTreeMap::new();
  graph_map.insert("Parent".to_string(), parent.clone());
  graph_map.insert("Child".to_string(), child.clone());

  let graph = create_test_graph(graph_map);
  let merger = SchemaMerger::new(graph);
  let merged_schema = merger.merge_child_schema_with_parent(&child, &parent).unwrap();

  // Child should override parent
  let prop = merged_schema.properties.get("prop").unwrap();
  if let ObjectOrReference::Object(schema) = prop {
    assert_eq!(schema.schema_type, Some(SchemaTypeSet::Single(SchemaType::Integer)));
  } else {
    panic!("Expected Object schema");
  }
}

#[test]
fn test_discriminator_handler_detect_parent() {
  let mut parent_schema = ObjectSchema::default();
  parent_schema.properties.insert(
    "type".to_string(),
    ObjectOrReference::Object(ObjectSchema {
      schema_type: Some(SchemaTypeSet::Single(SchemaType::String)),
      ..Default::default()
    }),
  );
  parent_schema.discriminator = Some(Discriminator {
    property_name: "type".to_string(),
    mapping: Some(BTreeMap::from([(
      "child".to_string(),
      "#/components/schemas/Child".to_string(),
    )])),
  });

  let mut child_schema = ObjectSchema::default();
  child_schema.all_of.push(ObjectOrReference::Ref {
    ref_path: "#/components/schemas/Parent".to_string(),
    summary: None,
    description: None,
  });

  let mut graph_map = BTreeMap::new();
  graph_map.insert("Parent".to_string(), parent_schema);
  graph_map.insert("Child".to_string(), child_schema.clone());

  let graph = create_test_graph(graph_map);
  let handler = DiscriminatorHandler::new(graph, None);

  let mut cache = HashMap::new();
  let result = handler.detect_discriminated_parent(&child_schema, &mut cache);

  assert!(result.is_some());
  assert!(result.unwrap().discriminator.is_some());
}

#[test]
fn test_field_optionality_policy() {
  let schema = ObjectSchema::default();
  let policy = FieldOptionalityPolicy::standard();

  let ctx_required = FieldContext {
    is_required: true,
    ..Default::default()
  };
  let is_optional_when_required = policy.is_optional("required_field", &schema, ctx_required);
  assert!(!is_optional_when_required);

  let ctx_not_required = FieldContext {
    is_required: false,
    ..Default::default()
  };
  let is_optional_when_not_required = policy.is_optional("other_field", &schema, ctx_not_required);
  assert!(is_optional_when_not_required);
}

fn make_field(name: &str, deprecated: bool) -> FieldDef {
  FieldDef {
    name: name.to_string(),
    deprecated,
    ..Default::default()
  }
}

#[test]
fn test_deduplicate_field_names_no_duplicates() {
  let mut fields = vec![
    make_field("foo", false),
    make_field("bar", false),
    make_field("baz", false),
  ];

  StructConverter::deduplicate_field_names(&mut fields);

  assert_eq!(fields.len(), 3);
  assert_eq!(fields[0].name, "foo");
  assert_eq!(fields[1].name, "bar");
  assert_eq!(fields[2].name, "baz");
}

#[test]
fn test_deduplicate_field_names_empty() {
  let mut fields: Vec<FieldDef> = vec![];
  StructConverter::deduplicate_field_names(&mut fields);
  assert!(fields.is_empty());
}

#[test]
fn test_deduplicate_field_names_all_non_deprecated_renamed() {
  let mut fields = vec![
    make_field("foo", false),
    make_field("foo", false),
    make_field("foo", false),
  ];

  StructConverter::deduplicate_field_names(&mut fields);

  assert_eq!(fields.len(), 3);
  assert_eq!(fields[0].name, "foo");
  assert_eq!(fields[1].name, "foo_2");
  assert_eq!(fields[2].name, "foo_3");
}

#[test]
fn test_deduplicate_field_names_deprecated_removed_when_mixed() {
  let mut fields = vec![
    make_field("foo", true),
    make_field("foo", false),
    make_field("bar", false),
  ];

  StructConverter::deduplicate_field_names(&mut fields);

  assert_eq!(fields.len(), 2);
  assert_eq!(fields[0].name, "foo");
  assert!(!fields[0].deprecated);
  assert_eq!(fields[1].name, "bar");
}

#[test]
fn test_deduplicate_field_names_all_deprecated_renamed() {
  let mut fields = vec![make_field("foo", true), make_field("foo", true)];

  StructConverter::deduplicate_field_names(&mut fields);

  assert_eq!(fields.len(), 2);
  assert_eq!(fields[0].name, "foo");
  assert_eq!(fields[1].name, "foo_2");
}

#[test]
fn test_deduplicate_field_names_multiple_groups() {
  let mut fields = vec![
    make_field("foo", false),
    make_field("bar", true),
    make_field("foo", false),
    make_field("bar", false),
  ];

  StructConverter::deduplicate_field_names(&mut fields);

  assert_eq!(fields.len(), 3);
  let names: Vec<_> = fields.iter().map(|f| f.name.as_str()).collect();
  assert!(names.contains(&"foo"));
  assert!(names.contains(&"foo_2"));
  assert!(names.contains(&"bar"));
  assert!(!fields.iter().any(|f| f.name == "bar" && f.deprecated));
}

fn make_metadata_with_docs() -> FieldMetadata {
  FieldMetadata {
    docs: vec!["/// Some docs".to_string()],
    validation_attrs: vec![ValidationAttribute::Email],
    default_value: None,
    deprecated: false,
    multiple_of: None,
  }
}

fn make_string_type_ref() -> TypeRef {
  TypeRef::new(RustPrimitive::String)
}

fn make_integer_type_ref() -> TypeRef {
  TypeRef::new(RustPrimitive::I64)
}

#[test]
fn test_apply_discriminator_attributes_none_returns_unchanged() {
  let metadata = make_metadata_with_docs();
  let serde_attrs = vec![SerdeAttribute::Rename("original".to_string())];
  let type_ref = make_string_type_ref();

  let result = FieldProcessor::apply_discriminator_attributes(metadata.clone(), serde_attrs.clone(), &type_ref, None);

  assert_eq!(result.metadata.docs, metadata.docs);
  assert_eq!(result.metadata.validation_attrs.len(), 1);
  assert_eq!(result.serde_attrs, serde_attrs);
  assert!(result.extra_attrs.is_empty());
}

#[test]
fn test_apply_discriminator_attributes_child_discriminator_hides_and_sets_value() {
  let metadata = make_metadata_with_docs();
  let serde_attrs = vec![];
  let type_ref = make_string_type_ref();

  let disc_info = DiscriminatorInfo {
    value: Some(DefaultAtom::from("child_type")),
    is_base: false,
    has_enum: false,
  };

  let result = FieldProcessor::apply_discriminator_attributes(metadata, serde_attrs, &type_ref, Some(&disc_info));

  assert!(result.metadata.docs.is_empty(), "docs should be cleared");
  assert!(
    result.metadata.validation_attrs.is_empty(),
    "validation attrs should be cleared"
  );
  assert_eq!(
    result.metadata.default_value,
    Some(serde_json::Value::String("child_type".to_string()))
  );
  assert!(result.serde_attrs.contains(&SerdeAttribute::SkipDeserializing));
  assert!(result.serde_attrs.contains(&SerdeAttribute::Default));
  assert!(result.extra_attrs.iter().any(|a| a.contains("doc(hidden)")));
}

#[test]
fn test_apply_discriminator_attributes_base_without_enum_hides_and_skips() {
  let metadata = make_metadata_with_docs();
  let serde_attrs = vec![];
  let type_ref = make_string_type_ref();

  let disc_info = DiscriminatorInfo {
    value: None,
    is_base: true,
    has_enum: false,
  };

  let result = FieldProcessor::apply_discriminator_attributes(metadata, serde_attrs, &type_ref, Some(&disc_info));

  assert!(result.metadata.docs.is_empty(), "docs should be cleared");
  assert!(
    result.metadata.validation_attrs.is_empty(),
    "validation attrs should be cleared"
  );
  assert_eq!(
    result.metadata.default_value,
    Some(serde_json::Value::String(String::new())),
    "string type should get empty default"
  );
  assert!(result.serde_attrs.contains(&SerdeAttribute::Skip));
  assert!(!result.serde_attrs.contains(&SerdeAttribute::SkipDeserializing));
  assert!(result.extra_attrs.iter().any(|a| a.contains("doc(hidden)")));
}

#[test]
fn test_apply_discriminator_attributes_base_without_enum_non_string_no_default() {
  let metadata = make_metadata_with_docs();
  let serde_attrs = vec![];
  let type_ref = make_integer_type_ref();

  let disc_info = DiscriminatorInfo {
    value: None,
    is_base: true,
    has_enum: false,
  };

  let result = FieldProcessor::apply_discriminator_attributes(metadata, serde_attrs, &type_ref, Some(&disc_info));

  assert!(
    result.metadata.default_value.is_none(),
    "non-string type should not get default"
  );
  assert!(result.serde_attrs.contains(&SerdeAttribute::Skip));
}

#[test]
fn test_apply_discriminator_attributes_base_with_enum_remains_visible() {
  let metadata = make_metadata_with_docs();
  let serde_attrs = vec![SerdeAttribute::Rename("role".to_string())];
  let type_ref = make_string_type_ref();

  let disc_info = DiscriminatorInfo {
    value: None,
    is_base: true,
    has_enum: true,
  };

  let result =
    FieldProcessor::apply_discriminator_attributes(metadata.clone(), serde_attrs.clone(), &type_ref, Some(&disc_info));

  assert_eq!(result.metadata.docs, metadata.docs, "docs should be preserved");
  assert_eq!(
    result.metadata.validation_attrs.len(),
    1,
    "validation attrs should be preserved"
  );
  assert_eq!(result.serde_attrs, serde_attrs, "serde attrs should be unchanged");
  assert!(result.extra_attrs.is_empty(), "should not be hidden");
}

#[test]
fn test_discriminated_child_with_defaults_has_serde_default() -> anyhow::Result<()> {
  let mut parent_schema = ObjectSchema {
    schema_type: Some(SchemaTypeSet::Single(SchemaType::Object)),
    ..Default::default()
  };
  parent_schema.properties.insert(
    "type".to_string(),
    ObjectOrReference::Object(ObjectSchema {
      schema_type: Some(SchemaTypeSet::Single(SchemaType::String)),
      ..Default::default()
    }),
  );
  parent_schema.required = vec!["type".to_string()];
  parent_schema.discriminator = Some(Discriminator {
    property_name: "type".to_string(),
    mapping: Some(BTreeMap::from([(
      "child".to_string(),
      "#/components/schemas/Child".to_string(),
    )])),
  });

  let mut child_schema = ObjectSchema::default();
  child_schema.all_of.push(ObjectOrReference::Ref {
    ref_path: "#/components/schemas/Parent".to_string(),
    summary: None,
    description: None,
  });
  child_schema.properties.insert(
    "count".to_string(),
    ObjectOrReference::Object(ObjectSchema {
      schema_type: Some(SchemaTypeSet::Single(SchemaType::Integer)),
      default: Some(serde_json::json!(0)),
      ..Default::default()
    }),
  );

  let graph = create_test_graph(BTreeMap::from([
    ("Parent".to_string(), parent_schema),
    ("Child".to_string(), child_schema),
  ]));

  let converter = SchemaConverter::new(&graph, FieldOptionalityPolicy::standard(), default_config());
  let result = converter.convert_schema("Child", graph.get_schema("Child").unwrap(), None)?;

  let struct_def = result
    .iter()
    .find_map(|ty| match ty {
      RustType::Struct(def) if def.name == "Child" => Some(def),
      _ => None,
    })
    .expect("Child struct should be present");

  assert!(
    struct_def.serde_attrs.contains(&SerdeAttribute::Default),
    "Struct with default field values should have #[serde(default)]"
  );

  Ok(())
}

#[test]
fn test_schema_merger_merge_all_of() {
  let mut base_schema = ObjectSchema::default();
  base_schema.properties.insert(
    "base_prop".to_string(),
    ObjectOrReference::Object(ObjectSchema {
      schema_type: Some(SchemaTypeSet::Single(SchemaType::String)),
      ..Default::default()
    }),
  );
  base_schema.required.push("base_prop".to_string());

  let mut mixin_schema = ObjectSchema::default();
  mixin_schema.properties.insert(
    "mixin_prop".to_string(),
    ObjectOrReference::Object(ObjectSchema {
      schema_type: Some(SchemaTypeSet::Single(SchemaType::Integer)),
      ..Default::default()
    }),
  );

  let mut composite_schema = ObjectSchema::default();
  composite_schema.all_of.push(ObjectOrReference::Ref {
    ref_path: "#/components/schemas/Base".to_string(),
    summary: None,
    description: None,
  });
  composite_schema.all_of.push(ObjectOrReference::Ref {
    ref_path: "#/components/schemas/Mixin".to_string(),
    summary: None,
    description: None,
  });
  composite_schema.properties.insert(
    "own_prop".to_string(),
    ObjectOrReference::Object(ObjectSchema {
      schema_type: Some(SchemaTypeSet::Single(SchemaType::Boolean)),
      ..Default::default()
    }),
  );

  let graph = create_test_graph(BTreeMap::from([
    ("Base".to_string(), base_schema),
    ("Mixin".to_string(), mixin_schema),
    ("Composite".to_string(), composite_schema.clone()),
  ]));

  let merger = SchemaMerger::new(graph);
  let merged = merger.merge_all_of_schema(&composite_schema).unwrap();

  assert!(merged.properties.contains_key("base_prop"));
  assert!(merged.properties.contains_key("mixin_prop"));
  assert!(merged.properties.contains_key("own_prop"));
  assert!(merged.required.contains(&"base_prop".to_string()));
}

#[test]
fn test_schema_merger_preserves_discriminator() {
  let mut parent_schema = ObjectSchema::default();
  parent_schema.properties.insert(
    "type".to_string(),
    ObjectOrReference::Object(ObjectSchema {
      schema_type: Some(SchemaTypeSet::Single(SchemaType::String)),
      ..Default::default()
    }),
  );
  parent_schema.discriminator = Some(Discriminator {
    property_name: "type".to_string(),
    mapping: Some(BTreeMap::from([(
      "child".to_string(),
      "#/components/schemas/Child".to_string(),
    )])),
  });

  let mut child_schema = ObjectSchema::default();
  child_schema.all_of.push(ObjectOrReference::Ref {
    ref_path: "#/components/schemas/Parent".to_string(),
    summary: None,
    description: None,
  });

  let graph = create_test_graph(BTreeMap::from([
    ("Parent".to_string(), parent_schema.clone()),
    ("Child".to_string(), child_schema.clone()),
  ]));

  let merger = SchemaMerger::new(graph);
  let merged = merger
    .merge_child_schema_with_parent(&child_schema, &parent_schema)
    .unwrap();

  assert!(merged.discriminator.is_some());
  assert_eq!(merged.discriminator.as_ref().unwrap().property_name, "type");
}

#[test]
fn test_discriminator_handler_no_parent_returns_none() {
  let schema = ObjectSchema::default();
  let graph = create_test_graph(BTreeMap::new());
  let handler = DiscriminatorHandler::new(graph, None);

  let mut cache = HashMap::new();
  let result = handler.detect_discriminated_parent(&schema, &mut cache);

  assert!(result.is_none());
}

#[test]
fn test_discriminator_handler_inline_all_of_returns_none() {
  let mut schema = ObjectSchema::default();
  schema.all_of.push(ObjectOrReference::Object(ObjectSchema {
    schema_type: Some(SchemaTypeSet::Single(SchemaType::Object)),
    ..Default::default()
  }));

  let graph = create_test_graph(BTreeMap::new());
  let handler = DiscriminatorHandler::new(graph, None);

  let mut cache = HashMap::new();
  let result = handler.detect_discriminated_parent(&schema, &mut cache);

  assert!(
    result.is_none(),
    "Inline schemas should not be considered discriminated parents"
  );
}

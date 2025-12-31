use std::collections::{BTreeMap, BTreeSet};

use oas3::spec::{BooleanSchema, Discriminator, ObjectOrReference, ObjectSchema, Schema, SchemaType, SchemaTypeSet};

use crate::{
  generator::{
    ast::{
      Documentation, FieldDef, FieldNameToken, RustPrimitive, RustType, SerdeAttribute, TypeRef, ValidationAttribute,
    },
    converter::{
      SchemaConverter, discriminator::DiscriminatorHandler, structs::StructConverter,
      type_resolver::TypeResolverBuilder,
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
  let converter = SchemaConverter::new(&graph, &default_config());
  let result = converter.convert_schema("Entity", graph.get("Entity").unwrap(), None)?;

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
  let converter = SchemaConverter::new(&graph, &default_config());
  let result = converter.convert_schema("Message", graph.get("Message").unwrap(), None)?;

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
    !role_field.doc_hidden,
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
  let converter = SchemaConverter::new(&graph, &default_config());
  let result = converter.convert_schema("Entity", graph.get("Entity").unwrap(), None)?;

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

  assert!(odata_field.doc_hidden, "odata_type field should be hidden");
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
  let merged_schema = graph.merged("Child").expect("merged schema should exist for Child");

  assert!(merged_schema.schema.properties.contains_key("parent_prop"));
  assert!(merged_schema.schema.properties.contains_key("child_prop"));
  assert!(merged_schema.schema.required.contains(&"parent_prop".to_string()));

  let effective_schema = graph.resolved("Child").unwrap();
  assert_eq!(effective_schema.properties.len(), merged_schema.schema.properties.len());
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
  let merged_schema = graph.merged("Child").expect("merged schema should exist for Child");

  let prop = merged_schema.schema.properties.get("prop").unwrap();
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
  let handler = DiscriminatorHandler::new(&graph, None);

  let result = handler.detect_discriminated_parent("Child");

  let info = result.expect("parent should be detected");
  assert_eq!(info.parent_name, "Parent");
}

fn make_field(name: &str, deprecated: bool) -> FieldDef {
  FieldDef::builder()
    .name(FieldNameToken::from_raw(name))
    .rust_type(TypeRef::new(RustPrimitive::String))
    .docs(make_docs())
    .deprecated(deprecated)
    .build()
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
  assert_eq!(fields[0].name.as_str(), "foo");
  assert_eq!(fields[1].name.as_str(), "bar");
  assert_eq!(fields[2].name.as_str(), "baz");
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
  assert_eq!(fields[0].name.as_str(), "foo");
  assert_eq!(fields[1].name.as_str(), "foo_2");
  assert_eq!(fields[2].name.as_str(), "foo_3");
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
  assert_eq!(fields[0].name.as_str(), "foo");
  assert!(!fields[0].deprecated);
  assert_eq!(fields[1].name.as_str(), "bar");
}

#[test]
fn test_deduplicate_field_names_all_deprecated_renamed() {
  let mut fields = vec![make_field("foo", true), make_field("foo", true)];

  StructConverter::deduplicate_field_names(&mut fields);

  assert_eq!(fields.len(), 2);
  assert_eq!(fields[0].name.as_str(), "foo");
  assert_eq!(fields[1].name.as_str(), "foo_2");
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

fn make_docs() -> Documentation {
  vec!["Some docs".to_string()].into()
}

fn make_string_type_ref() -> TypeRef {
  TypeRef::new(RustPrimitive::String)
}

fn make_integer_type_ref() -> TypeRef {
  TypeRef::new(RustPrimitive::I64)
}

fn make_base_field(type_ref: TypeRef) -> FieldDef {
  FieldDef::builder()
    .name(FieldNameToken::from_raw("test_field"))
    .docs(make_docs())
    .rust_type(type_ref)
    .serde_attrs(BTreeSet::from([SerdeAttribute::Rename("original".to_string())]))
    .validation_attrs(vec![ValidationAttribute::Email])
    .build()
}

#[test]
fn test_with_discriminator_behavior_child_discriminator_hides_and_sets_value() {
  let field = make_base_field(make_string_type_ref());
  let result = field.with_discriminator_behavior(Some("child_type"), false);

  assert!(result.docs.is_empty(), "docs should be cleared");
  assert!(result.validation_attrs.is_empty(), "validation should be cleared");
  assert_eq!(
    result.default_value,
    Some(serde_json::Value::String("child_type".to_string()))
  );
  assert!(result.serde_attrs.contains(&SerdeAttribute::SkipDeserializing));
  assert!(result.serde_attrs.contains(&SerdeAttribute::Default));
  assert!(result.doc_hidden);
}

#[test]
fn test_with_discriminator_behavior_base_hides_and_skips_string() {
  let field = make_base_field(make_string_type_ref());
  let result = field.with_discriminator_behavior(None, true);

  assert!(result.docs.is_empty(), "docs should be cleared");
  assert!(result.validation_attrs.is_empty(), "validation should be cleared");
  assert_eq!(result.default_value, Some(serde_json::Value::String(String::new())));
  assert!(result.serde_attrs.contains(&SerdeAttribute::Skip));
  assert!(
    !result
      .serde_attrs
      .contains(&SerdeAttribute::Rename("original".to_string()))
  );
  assert!(result.doc_hidden);
}

#[test]
fn test_with_discriminator_behavior_base_non_string_no_default() {
  let field = make_base_field(make_integer_type_ref());
  let result = field.with_discriminator_behavior(None, true);

  assert!(result.default_value.is_none(), "non-string type should not get default");
  assert!(result.serde_attrs.contains(&SerdeAttribute::Skip));
  assert!(result.doc_hidden);
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

  let converter = SchemaConverter::new(&graph, &default_config());
  let result = converter.convert_schema("Child", graph.get("Child").unwrap(), None)?;

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

  let merged_schema = graph
    .merged("Composite")
    .expect("merged schema should exist for Composite");

  assert!(merged_schema.schema.properties.contains_key("base_prop"));
  assert!(merged_schema.schema.properties.contains_key("mixin_prop"));
  assert!(merged_schema.schema.properties.contains_key("own_prop"));
  assert!(merged_schema.schema.required.contains(&"base_prop".to_string()));
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

  let merged_schema = graph.merged("Child").expect("merged schema should exist for Child");

  assert_eq!(merged_schema.discriminator_parent.as_deref(), Some("Parent"));
  assert!(merged_schema.schema.discriminator.is_some());
  assert_eq!(
    merged_schema
      .schema
      .discriminator
      .as_ref()
      .expect("discriminator should exist")
      .property_name,
    "type"
  );
}

#[test]
fn test_discriminator_handler_no_parent_returns_none() {
  let graph = create_test_graph(BTreeMap::new());
  let handler = DiscriminatorHandler::new(&graph, None);
  let result = handler.detect_discriminated_parent("Unknown");

  assert!(result.is_none());
}

#[test]
fn test_discriminator_handler_inline_all_of_returns_none() {
  let mut schema = ObjectSchema::default();
  schema.all_of.push(ObjectOrReference::Object(ObjectSchema {
    schema_type: Some(SchemaTypeSet::Single(SchemaType::Object)),
    ..Default::default()
  }));

  let graph = create_test_graph(BTreeMap::from([("Inline".to_string(), schema.clone())]));
  let handler = DiscriminatorHandler::new(&graph, None);

  let result = handler.detect_discriminated_parent("Inline");

  assert!(
    result.is_none(),
    "Inline schemas should not be considered discriminated parents"
  );
}

#[test]
fn test_discriminator_handler_deduplicates_same_schema_mappings() -> anyhow::Result<()> {
  let base_schema = ObjectSchema {
    schema_type: Some(SchemaTypeSet::Single(SchemaType::Object)),
    properties: BTreeMap::from([(
      "type".to_string(),
      ObjectOrReference::Object(ObjectSchema {
        schema_type: Some(SchemaTypeSet::Single(SchemaType::String)),
        ..Default::default()
      }),
    )]),
    discriminator: Some(Discriminator {
      property_name: "type".to_string(),
      mapping: Some(BTreeMap::from([
        ("child_event".to_string(), "#/components/schemas/ChildEvent".to_string()),
        ("ChildEvent".to_string(), "#/components/schemas/ChildEvent".to_string()),
      ])),
    }),
    ..Default::default()
  };

  let child_schema = ObjectSchema {
    schema_type: Some(SchemaTypeSet::Single(SchemaType::Object)),
    properties: BTreeMap::from([(
      "data".to_string(),
      ObjectOrReference::Object(ObjectSchema {
        schema_type: Some(SchemaTypeSet::Single(SchemaType::String)),
        ..Default::default()
      }),
    )]),
    ..Default::default()
  };

  let graph = create_test_graph(BTreeMap::from([
    ("BaseEvent".to_string(), base_schema.clone()),
    ("ChildEvent".to_string(), child_schema),
  ]));

  let type_resolver = TypeResolverBuilder::default()
    .config(default_config())
    .graph(graph.clone())
    .build()
    .unwrap();

  let result = type_resolver.create_discriminated_enum("BaseEvent", &base_schema, "BaseEventBase")?;

  let RustType::DiscriminatedEnum(enum_def) = result else {
    panic!("Expected DiscriminatedEnum");
  };

  assert_eq!(
    enum_def.variants.len(),
    1,
    "Expected 1 variant but got {}: {:?}",
    enum_def.variants.len(),
    enum_def.variants.iter().map(|v| &v.variant_name).collect::<Vec<_>>()
  );

  assert_eq!(enum_def.variants[0].type_name.base_type.to_string(), "ChildEvent");

  assert!(enum_def.fallback.is_some());
  assert_eq!(
    enum_def.fallback.as_ref().unwrap().type_name.base_type.to_string(),
    "BaseEventBase"
  );

  Ok(())
}

#[test]
fn test_extract_discriminator_children_returns_alphabetical_order() {
  let base_schema = ObjectSchema {
    schema_type: Some(SchemaTypeSet::Single(SchemaType::Object)),
    properties: BTreeMap::from([(
      "type".to_string(),
      ObjectOrReference::Object(ObjectSchema {
        schema_type: Some(SchemaTypeSet::Single(SchemaType::String)),
        ..Default::default()
      }),
    )]),
    discriminator: Some(Discriminator {
      property_name: "type".to_string(),
      mapping: Some(BTreeMap::from([
        ("zebra".to_string(), "#/components/schemas/Zebra".to_string()),
        ("alpha".to_string(), "#/components/schemas/Alpha".to_string()),
        ("middle".to_string(), "#/components/schemas/Middle".to_string()),
        ("beta".to_string(), "#/components/schemas/Beta".to_string()),
      ])),
    }),
    ..Default::default()
  };

  let empty_schema = ObjectSchema {
    schema_type: Some(SchemaTypeSet::Single(SchemaType::Object)),
    ..Default::default()
  };

  let graph = create_test_graph(BTreeMap::from([
    ("Base".to_string(), base_schema.clone()),
    ("Alpha".to_string(), empty_schema.clone()),
    ("Beta".to_string(), empty_schema.clone()),
    ("Middle".to_string(), empty_schema.clone()),
    ("Zebra".to_string(), empty_schema.clone()),
  ]));

  let handler = DiscriminatorHandler::new(&graph, None);
  let children = handler.extract_discriminator_children(&base_schema);

  let schema_names: Vec<&str> = children.iter().map(|(_, name)| name.as_str()).collect();
  assert_eq!(
    schema_names,
    vec!["Alpha", "Beta", "Middle", "Zebra"],
    "Children should be in alphabetical order by schema name"
  );
}

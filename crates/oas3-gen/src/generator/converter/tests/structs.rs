use std::collections::BTreeMap;

use oas3::spec::{BooleanSchema, Discriminator, ObjectOrReference, ObjectSchema, Schema, SchemaType, SchemaTypeSet};

use crate::{
  generator::{
    ast::{RustType, SerdeAttribute},
    converter::{SchemaConverter, discriminator::DiscriminatorConverter},
  },
  tests::common::{create_test_context, create_test_graph, default_config},
};

#[test]
fn discriminated_base_struct_renamed() -> anyhow::Result<()> {
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
  let context = create_test_context(graph.clone(), default_config());
  let converter = SchemaConverter::new(&context);
  let result = converter.convert_schema("Entity", graph.get("Entity").unwrap())?;

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
fn discriminator_with_enum_remains_visible() -> anyhow::Result<()> {
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
  let context = create_test_context(graph.clone(), default_config());
  let converter = SchemaConverter::new(&context);
  let result = converter.convert_schema("Message", graph.get("Message").unwrap())?;

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
fn discriminator_without_enum_is_hidden() -> anyhow::Result<()> {
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
  let context = create_test_context(graph.clone(), default_config());
  let converter = SchemaConverter::new(&context);
  let result = converter.convert_schema("Entity", graph.get("Entity").unwrap())?;

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
fn discriminator_handler_detect_parent() {
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
    ("Parent".to_string(), parent_schema),
    ("Child".to_string(), child_schema),
  ]));
  let context = create_test_context(graph.clone(), default_config());
  let handler = DiscriminatorConverter::new(context);

  let result = handler.detect_discriminated_parent("Child");

  let info = result.expect("parent should be detected");
  assert_eq!(info.parent_name, "Parent");
}

#[test]
fn discriminated_child_with_defaults_has_serde_default() -> anyhow::Result<()> {
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

  let context = create_test_context(graph.clone(), default_config());
  let converter = SchemaConverter::new(&context);
  let result = converter.convert_schema("Child", graph.get("Child").unwrap())?;

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
fn discriminator_deduplicates_same_schema_mappings() -> anyhow::Result<()> {
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

  let context = create_test_context(graph.clone(), default_config());
  let schema_converter = SchemaConverter::new(&context);

  let result = schema_converter.discriminated_enum("BaseEvent", &base_schema, "BaseEventBase")?;

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
fn discriminator_mappings_alphabetical_order() {
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

  let context = create_test_context(graph.clone(), default_config());
  let handler = DiscriminatorConverter::new(context);
  let mappings = handler.build_variants_from_mapping("Base", &base_schema);

  let variant_names: Vec<&str> = mappings.iter().map(|v| v.variant_name.as_str()).collect();
  assert_eq!(
    variant_names,
    vec!["Alpha", "Beta", "Middle", "Zebra"],
    "Mappings should be in alphabetical order by schema name"
  );
}

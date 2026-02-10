use std::collections::BTreeMap;

use oas3::spec::{BooleanSchema, Discriminator, ObjectOrReference, ObjectSchema, Schema, SchemaType, SchemaTypeSet};

use crate::{
  generator::{
    ast::{RustType, SerdeAttribute},
    converter::{discriminator::DiscriminatorConverter, SchemaConverter},
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
      "#microsoft.graph.corgi".to_string(),
      "#/components/schemas/Corgi".to_string(),
    )])),
  });

  let graph = create_test_graph(BTreeMap::from([("Cardigan".to_string(), entity_schema)]));
  let context = create_test_context(graph.clone(), default_config());
  let converter = SchemaConverter::new(&context);
  let result = converter.convert_schema("Cardigan", graph.get("Cardigan").unwrap())?;

  let struct_def = result
    .iter()
    .find_map(|ty| match ty {
      RustType::Struct(def) => Some(def),
      _ => None,
    })
    .expect("Backing struct should be present");

  assert_eq!(struct_def.name, "CardiganBase");
  assert!(struct_def.serde_attrs.contains(&SerdeAttribute::DenyUnknownFields));
  Ok(())
}

#[test]
fn discriminator_with_enum_remains_visible() -> anyhow::Result<()> {
  let mut bark_schema = ObjectSchema {
    schema_type: Some(SchemaTypeSet::Single(SchemaType::Object)),
    additional_properties: Some(Schema::Boolean(BooleanSchema(false))),
    ..Default::default()
  };
  bark_schema.properties.insert(
    "sploot_role".to_string(),
    ObjectOrReference::Object(ObjectSchema {
      schema_type: Some(SchemaTypeSet::Single(SchemaType::String)),
      enum_values: vec![
        serde_json::Value::String("corgi".to_string()),
        serde_json::Value::String("frappe".to_string()),
      ],
      ..Default::default()
    }),
  );
  bark_schema.properties.insert(
    "bark_content".to_string(),
    ObjectOrReference::Object(ObjectSchema {
      schema_type: Some(SchemaTypeSet::Single(SchemaType::String)),
      ..Default::default()
    }),
  );
  bark_schema.required = vec!["sploot_role".to_string(), "bark_content".to_string()];
  bark_schema.discriminator = Some(Discriminator {
    property_name: "sploot_role".to_string(),
    mapping: None,
  });

  let graph = create_test_graph(BTreeMap::from([("Bark".to_string(), bark_schema)]));
  let context = create_test_context(graph.clone(), default_config());
  let converter = SchemaConverter::new(&context);
  let result = converter.convert_schema("Bark", graph.get("Bark").unwrap())?;

  let struct_def = result
    .iter()
    .find_map(|ty| match ty {
      RustType::Struct(def) => Some(def),
      _ => None,
    })
    .expect("Struct should be present");

  assert_eq!(struct_def.name, "Bark");

  let sploot_role_field = struct_def
    .fields
    .iter()
    .find(|f| f.name == "sploot_role")
    .expect("sploot_role field should exist");

  assert!(
    !sploot_role_field.doc_hidden,
    "sploot_role field should not be hidden when discriminator has enum values"
  );
  assert!(
    !sploot_role_field
      .serde_attrs
      .iter()
      .any(|a| matches!(a, SerdeAttribute::Skip | SerdeAttribute::SkipDeserializing)),
    "sploot_role field should not be skipped when discriminator has enum values"
  );
  assert!(
    !sploot_role_field.rust_type.to_rust_type().starts_with("Option<"),
    "sploot_role field should be required, not optional"
  );

  Ok(())
}

#[test]
fn discriminator_with_single_enum_is_hidden() -> anyhow::Result<()> {
  let mut howl_schema = ObjectSchema {
    schema_type: Some(SchemaTypeSet::Single(SchemaType::Object)),
    additional_properties: Some(Schema::Boolean(BooleanSchema(false))),
    ..Default::default()
  };
  howl_schema.properties.insert(
    "howl_role".to_string(),
    ObjectOrReference::Object(ObjectSchema {
      schema_type: Some(SchemaTypeSet::Single(SchemaType::String)),
      enum_values: vec![serde_json::Value::String("only_value".to_string())],
      ..Default::default()
    }),
  );
  howl_schema.properties.insert(
    "howl_content".to_string(),
    ObjectOrReference::Object(ObjectSchema {
      schema_type: Some(SchemaTypeSet::Single(SchemaType::String)),
      ..Default::default()
    }),
  );
  howl_schema.required = vec!["howl_role".to_string(), "howl_content".to_string()];
  howl_schema.discriminator = Some(Discriminator {
    property_name: "howl_role".to_string(),
    mapping: None,
  });

  let graph = create_test_graph(BTreeMap::from([("Howl".to_string(), howl_schema)]));
  let context = create_test_context(graph.clone(), default_config());
  let converter = SchemaConverter::new(&context);
  let result = converter.convert_schema("Howl", graph.get("Howl").unwrap())?;

  let struct_def = result
    .iter()
    .find_map(|ty| match ty {
      RustType::Struct(def) => Some(def),
      _ => None,
    })
    .expect("Struct should be present");

  let howl_role_field = struct_def
    .fields
    .iter()
    .find(|f| f.name == "howl_role")
    .expect("howl_role field should exist");

  assert!(
    howl_role_field.doc_hidden,
    "single-value enum discriminator should be hidden like const"
  );
  assert!(
    howl_role_field
      .serde_attrs
      .iter()
      .any(|a| matches!(a, SerdeAttribute::Skip)),
    "single-value enum discriminator should be skipped like const"
  );

  Ok(())
}

#[test]
fn discriminator_without_enum_is_hidden() -> anyhow::Result<()> {
  let mut cardigan_schema = ObjectSchema {
    schema_type: Some(SchemaTypeSet::Single(SchemaType::Object)),
    ..Default::default()
  };
  cardigan_schema.properties.insert(
    "@toebeans.type".to_string(),
    ObjectOrReference::Object(ObjectSchema {
      schema_type: Some(SchemaTypeSet::Single(SchemaType::String)),
      ..Default::default()
    }),
  );
  cardigan_schema.properties.insert(
    "tag_id".to_string(),
    ObjectOrReference::Object(ObjectSchema {
      schema_type: Some(SchemaTypeSet::Single(SchemaType::String)),
      ..Default::default()
    }),
  );
  cardigan_schema.required = vec!["@toebeans.type".to_string()];
  cardigan_schema.discriminator = Some(Discriminator {
    property_name: "@toebeans.type".to_string(),
    mapping: Some(BTreeMap::from([(
      "#microsoft.graph.corgi".to_string(),
      "#/components/schemas/Corgi".to_string(),
    )])),
  });

  let graph = create_test_graph(BTreeMap::from([("Cardigan".to_string(), cardigan_schema)]));
  let context = create_test_context(graph.clone(), default_config());
  let converter = SchemaConverter::new(&context);
  let result = converter.convert_schema("Cardigan", graph.get("Cardigan").unwrap())?;

  let struct_def = result
    .iter()
    .find_map(|ty| match ty {
      RustType::Struct(def) if def.name == "CardiganBase" => Some(def),
      _ => None,
    })
    .expect("CardiganBase struct should be present");

  let toebeans_field = struct_def
    .fields
    .iter()
    .find(|f| f.name == "toebeans_type")
    .expect("toebeans_type field should exist");

  assert!(toebeans_field.doc_hidden, "toebeans_type field should be hidden");
  assert!(
    toebeans_field.serde_attrs.contains(&SerdeAttribute::Skip),
    "toebeans_type field should be skipped"
  );

  Ok(())
}

#[test]
fn discriminator_handler_detect_parent() {
  let mut loaf_schema = ObjectSchema::default();
  loaf_schema.properties.insert(
    "type".to_string(),
    ObjectOrReference::Object(ObjectSchema {
      schema_type: Some(SchemaTypeSet::Single(SchemaType::String)),
      ..Default::default()
    }),
  );
  loaf_schema.discriminator = Some(Discriminator {
    property_name: "type".to_string(),
    mapping: Some(BTreeMap::from([(
      "nugget".to_string(),
      "#/components/schemas/Nugget".to_string(),
    )])),
  });

  let mut nugget_schema = ObjectSchema::default();
  nugget_schema.all_of.push(ObjectOrReference::Ref {
    ref_path: "#/components/schemas/Loaf".to_string(),
    summary: None,
    description: None,
  });

  let graph = create_test_graph(BTreeMap::from([
    ("Loaf".to_string(), loaf_schema),
    ("Nugget".to_string(), nugget_schema),
  ]));
  let context = create_test_context(graph.clone(), default_config());
  let handler = DiscriminatorConverter::new(context);

  let result = handler.detect_discriminated_parent("Nugget");

  let parent_name = result.expect("parent should be detected");
  assert_eq!(parent_name, "Loaf");
}

#[test]
fn discriminated_child_with_defaults_has_serde_default() -> anyhow::Result<()> {
  let mut loaf_schema = ObjectSchema {
    schema_type: Some(SchemaTypeSet::Single(SchemaType::Object)),
    ..Default::default()
  };
  loaf_schema.properties.insert(
    "type".to_string(),
    ObjectOrReference::Object(ObjectSchema {
      schema_type: Some(SchemaTypeSet::Single(SchemaType::String)),
      ..Default::default()
    }),
  );
  loaf_schema.required = vec!["type".to_string()];
  loaf_schema.discriminator = Some(Discriminator {
    property_name: "type".to_string(),
    mapping: Some(BTreeMap::from([(
      "nugget".to_string(),
      "#/components/schemas/Nugget".to_string(),
    )])),
  });

  let mut nugget_schema = ObjectSchema::default();
  nugget_schema.all_of.push(ObjectOrReference::Ref {
    ref_path: "#/components/schemas/Loaf".to_string(),
    summary: None,
    description: None,
  });
  nugget_schema.properties.insert(
    "count".to_string(),
    ObjectOrReference::Object(ObjectSchema {
      schema_type: Some(SchemaTypeSet::Single(SchemaType::Integer)),
      default: Some(serde_json::json!(0)),
      ..Default::default()
    }),
  );

  let graph = create_test_graph(BTreeMap::from([
    ("Loaf".to_string(), loaf_schema),
    ("Nugget".to_string(), nugget_schema),
  ]));

  let context = create_test_context(graph.clone(), default_config());
  let converter = SchemaConverter::new(&context);
  let result = converter.convert_schema("Nugget", graph.get("Nugget").unwrap())?;

  let struct_def = result
    .iter()
    .find_map(|ty| match ty {
      RustType::Struct(def) if def.name == "Nugget" => Some(def),
      _ => None,
    })
    .expect("Nugget struct should be present");

  assert!(
    struct_def.serde_attrs.contains(&SerdeAttribute::Default),
    "Struct with default field values should have #[serde(default)]"
  );

  Ok(())
}

#[test]
fn discriminator_deduplicates_same_schema_mappings() -> anyhow::Result<()> {
  let frappe_schema = ObjectSchema {
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
        (
          "sploot_frappe".to_string(),
          "#/components/schemas/SplootFrappe".to_string(),
        ),
        (
          "SplootFrappe".to_string(),
          "#/components/schemas/SplootFrappe".to_string(),
        ),
      ])),
    }),
    ..Default::default()
  };

  let sploot_frappe_schema = ObjectSchema {
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
    ("Frappe".to_string(), frappe_schema.clone()),
    ("SplootFrappe".to_string(), sploot_frappe_schema),
  ]));

  let context = create_test_context(graph.clone(), default_config());
  let schema_converter = SchemaConverter::new(&context);

  let result = schema_converter.discriminated_enum("Frappe", &frappe_schema, "FrappeBase")?;

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

  assert_eq!(enum_def.variants[0].type_name.base_type.to_string(), "SplootFrappe");

  assert!(enum_def.fallback.is_some());
  assert_eq!(
    enum_def.fallback.as_ref().unwrap().type_name.base_type.to_string(),
    "FrappeBase"
  );

  Ok(())
}

#[test]
fn discriminator_mappings_alphabetical_order() {
  let park_schema = ObjectSchema {
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
        ("stumpy".to_string(), "#/components/schemas/Stumpy".to_string()),
        ("floof".to_string(), "#/components/schemas/Floof".to_string()),
        ("frappe".to_string(), "#/components/schemas/Frappe".to_string()),
        ("sploot".to_string(), "#/components/schemas/Sploot".to_string()),
      ])),
    }),
    ..Default::default()
  };

  let empty_schema = ObjectSchema {
    schema_type: Some(SchemaTypeSet::Single(SchemaType::Object)),
    ..Default::default()
  };

  let graph = create_test_graph(BTreeMap::from([
    ("Park".to_string(), park_schema.clone()),
    ("Floof".to_string(), empty_schema.clone()),
    ("Sploot".to_string(), empty_schema.clone()),
    ("Frappe".to_string(), empty_schema.clone()),
    ("Stumpy".to_string(), empty_schema.clone()),
  ]));

  let context = create_test_context(graph.clone(), default_config());
  let handler = DiscriminatorConverter::new(context);
  let mappings = handler.build_variants_from_mapping("Park", &park_schema);

  let variant_names: Vec<&str> = mappings.iter().map(|v| v.variant_name.as_str()).collect();
  assert_eq!(
    variant_names,
    vec!["Floof", "Frappe", "Sploot", "Stumpy"],
    "Mappings should be in alphabetical order by schema name"
  );
}

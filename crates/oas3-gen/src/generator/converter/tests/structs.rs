use std::{collections::BTreeMap, sync::Arc};

use oas3::spec::Spec;
use serde_json::json;

use crate::{
  generator::{
    ast::{RustType, SerdeAttribute},
    converter::{SchemaConverter, discriminator::DiscriminatorConverter},
    metrics::GenerationStats,
    schema_registry::SchemaRegistry,
  },
  tests::common::{create_test_context, create_test_graph, default_config, parse_schema},
};

fn create_graph_from_json(
  components_schemas: &serde_json::Value,
) -> Arc<crate::generator::schema_registry::SchemaRegistry> {
  let spec_json = json!({
    "openapi": "3.1.0",
    "info": { "title": "Test API", "version": "1.0.0" },
    "paths": {},
    "components": { "schemas": components_schemas }
  });
  let spec = serde_json::from_value::<Spec>(spec_json).expect("failed to parse spec from JSON");
  let mut stats = GenerationStats::default();
  let mut graph = SchemaRegistry::new(&spec, &mut stats);
  let union_fingerprints = BTreeMap::new();
  graph.build_dependencies(&union_fingerprints);
  graph.detect_cycles();
  Arc::new(graph)
}

#[test]
fn discriminated_base_struct_renamed() -> anyhow::Result<()> {
  let entity_schema = parse_schema(json!({
    "type": "object",
    "additionalProperties": false,
    "properties": {
      "id": { "type": "string" },
      "@odata.type": { "type": "string" }
    },
    "discriminator": {
      "propertyName": "@odata.type",
      "mapping": {
        "#microsoft.graph.corgi": "#/components/schemas/Corgi"
      }
    }
  }));

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
  let bark_schema = parse_schema(json!({
    "type": "object",
    "additionalProperties": false,
    "properties": {
      "sploot_role": {
        "type": "string",
        "enum": ["corgi", "frappe"]
      },
      "bark_content": { "type": "string" }
    },
    "required": ["sploot_role", "bark_content"],
    "discriminator": {
      "propertyName": "sploot_role"
    }
  }));

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
  let howl_schema = parse_schema(json!({
    "type": "object",
    "additionalProperties": false,
    "properties": {
      "howl_role": {
        "type": "string",
        "enum": ["only_value"]
      },
      "howl_content": { "type": "string" }
    },
    "required": ["howl_role", "howl_content"],
    "discriminator": {
      "propertyName": "howl_role"
    }
  }));

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
  let cardigan_schema = parse_schema(json!({
    "type": "object",
    "properties": {
      "@toebeans.type": { "type": "string" },
      "tag_id": { "type": "string" }
    },
    "required": ["@toebeans.type"],
    "discriminator": {
      "propertyName": "@toebeans.type",
      "mapping": {
        "#microsoft.graph.corgi": "#/components/schemas/Corgi"
      }
    }
  }));

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
  let components = json!({
    "Loaf": {
      "type": "object",
      "properties": {
        "type": { "type": "string" }
      },
      "discriminator": {
        "propertyName": "type",
        "mapping": {
          "nugget": "#/components/schemas/Nugget"
        }
      }
    },
    "Nugget": {
      "allOf": [{ "$ref": "#/components/schemas/Loaf" }]
    }
  });

  let graph = create_graph_from_json(&components);
  let context = create_test_context(graph.clone(), default_config());
  let handler = DiscriminatorConverter::new(context);

  let result = handler.detect_discriminated_parent("Nugget");

  let parent_name = result.expect("parent should be detected");
  assert_eq!(parent_name, "Loaf");
}

#[test]
fn discriminated_child_with_defaults_has_serde_default() -> anyhow::Result<()> {
  let loaf_schema = parse_schema(json!({
    "type": "object",
    "properties": {
      "type": { "type": "string" }
    },
    "required": ["type"],
    "discriminator": {
      "propertyName": "type",
      "mapping": {
        "nugget": "#/components/schemas/Nugget"
      }
    }
  }));

  let nugget_schema = parse_schema(json!({
    "allOf": [{ "$ref": "#/components/schemas/Loaf" }],
    "properties": {
      "count": {
        "type": "integer",
        "default": 0
      }
    }
  }));

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
  let frappe_schema = parse_schema(json!({
    "type": "object",
    "properties": {
      "type": { "type": "string" }
    },
    "discriminator": {
      "propertyName": "type",
      "mapping": {
        "sploot_frappe": "#/components/schemas/SplootFrappe",
        "SplootFrappe": "#/components/schemas/SplootFrappe"
      }
    }
  }));

  let sploot_frappe_schema = parse_schema(json!({
    "type": "object",
    "properties": {
      "data": { "type": "string" }
    }
  }));

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
  let components = json!({
    "Park": {
      "type": "object",
      "properties": {
        "type": { "type": "string" }
      },
      "discriminator": {
        "propertyName": "type",
        "mapping": {
          "stumpy": "#/components/schemas/Stumpy",
          "floof": "#/components/schemas/Floof",
          "frappe": "#/components/schemas/Frappe",
          "sploot": "#/components/schemas/Sploot"
        }
      }
    },
    "Floof": { "type": "object" },
    "Sploot": { "type": "object" },
    "Frappe": { "type": "object" },
    "Stumpy": { "type": "object" }
  });

  let graph = create_graph_from_json(&components);
  let park_schema = graph.get("Park").unwrap();

  let context = create_test_context(graph.clone(), default_config());
  let handler = DiscriminatorConverter::new(context);
  let mappings = handler.build_variants_from_mapping("Park", park_schema);

  let variant_names: Vec<&str> = mappings.iter().map(|v| v.variant_name.as_str()).collect();
  assert_eq!(
    variant_names,
    vec!["Floof", "Frappe", "Sploot", "Stumpy"],
    "Mappings should be in alphabetical order by schema name"
  );
}

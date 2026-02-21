use std::collections::BTreeMap;

use oas3::spec::{ObjectOrReference, ObjectSchema, SchemaType, SchemaTypeSet, Spec};
use serde_json::json;

use crate::{
  generator::{
    metrics::{GenerationStats, GenerationWarning},
    schema_registry::SchemaRegistry,
  },
  tests::common::parse_schema,
  utils::parse_schema_ref_path,
};

const SCHEMA_REF_PREFIX: &str = "#/components/schemas/";

#[test]
fn test_parse_ref() {
  let cases = [
    ("#/components/schemas/Corgi", Some("Corgi")),
    ("#/components/schemas/NestedCorgi", Some("NestedCorgi")),
    ("#/other/path", None),
    ("InvalidRef", None),
  ];
  for (input, expected) in cases {
    let result = parse_schema_ref_path(input);
    assert_eq!(result.as_deref(), expected, "failed for input {input:?}");
  }
}

fn spec_with_schemas(schemas_json: &serde_json::Value) -> Spec {
  let spec_json = json!({
    "openapi": "3.0.0",
    "info": {"title": "Test", "version": "1.0.0"},
    "components": {"schemas": schemas_json}
  });
  serde_json::from_value(spec_json).expect("failed to parse spec from JSON")
}

#[test]
fn test_ref_collector() {
  let spec = spec_with_schemas(&json!({}));
  let mut stats = GenerationStats::default();
  let registry = SchemaRegistry::new(&spec, &mut stats);
  let union_fingerprints = BTreeMap::new();

  let schema = parse_schema(json!({
    "properties": {
      "related": {"$ref": "#/components/schemas/Corgi"}
    }
  }));
  let refs = registry.collect(&schema, &union_fingerprints);
  assert_eq!(refs.len(), 1, "simple ref: expected 1 ref");
  assert!(refs.contains("Corgi"), "simple ref: should contain Corgi");

  let schema = parse_schema(json!({
    "properties": {
      "waddler": {"$ref": "#/components/schemas/Corgi"},
      "sploot": {"$ref": "#/components/schemas/Sploot"}
    }
  }));
  let refs = registry.collect(&schema, &union_fingerprints);
  assert_eq!(refs.len(), 2, "multiple refs: expected 2 refs");
  assert!(refs.contains("Corgi"), "multiple refs: should contain Corgi");
  assert!(refs.contains("Sploot"), "multiple refs: should contain Sploot");

  let schema = parse_schema(json!({
    "oneOf": [{"$ref": "#/components/schemas/Corgi"}],
    "anyOf": [{"$ref": "#/components/schemas/Bark"}],
    "allOf": [{"$ref": "#/components/schemas/Frappe"}]
  }));
  let refs = registry.collect(&schema, &union_fingerprints);
  assert_eq!(refs.len(), 3, "combinators: expected 3 refs");
  assert!(refs.contains("Corgi"), "combinators: should contain Corgi");
  assert!(refs.contains("Bark"), "combinators: should contain Bark");
  assert!(refs.contains("Frappe"), "combinators: should contain Frappe");
}

#[test]
fn test_schema_registry() {
  let spec = spec_with_schemas(&json!({
    "Corgi": {"type": "object"},
    "Bark": {"type": "object"}
  }));
  let mut stats = GenerationStats::default();
  let registry = SchemaRegistry::new(&spec, &mut stats);

  assert!(registry.get("Corgi").is_some(), "should have Corgi schema");
  assert!(registry.get("Bark").is_some(), "should have Bark schema");
  assert!(registry.get("NonExistent").is_none(), "should not have NonExistent");
  assert_eq!(registry.keys().len(), 2, "should have 2 schemas");

  let spec = spec_with_schemas(&json!({
    "Corgi": {"type": "object"},
    "Bark": {
      "type": "object",
      "properties": {
        "related": {"$ref": "#/components/schemas/Corgi"}
      }
    }
  }));
  let mut stats = GenerationStats::default();
  let mut graph = SchemaRegistry::new(&spec, &mut stats);
  let union_fingerprints = BTreeMap::new();
  graph.build_dependencies(&union_fingerprints);

  assert_eq!(graph.keys().len(), 2, "build deps: should have 2 schemas");
  assert!(graph.get("Corgi").is_some(), "build deps: should have Corgi");
  assert!(graph.get("Bark").is_some(), "build deps: should have Bark");
}

#[test]
fn test_schema_graph_cycle_detection() {
  {
    let spec = spec_with_schemas(&json!({
      "A": {"type": "object"},
      "B": {
        "type": "object",
        "properties": {
          "a": {"$ref": "#/components/schemas/A"}
        }
      },
      "C": {
        "type": "object",
        "properties": {
          "b": {"$ref": "#/components/schemas/B"}
        }
      }
    }));
    let mut stats = GenerationStats::default();
    let mut graph = SchemaRegistry::new(&spec, &mut stats);
    let union_fingerprints = BTreeMap::new();
    graph.build_dependencies(&union_fingerprints);
    let cycles = graph.detect_cycles();

    assert!(cycles.is_empty(), "linear deps: should have no cycles");
    assert!(!graph.is_cyclic("A"), "linear deps: A should not be cyclic");
    assert!(!graph.is_cyclic("B"), "linear deps: B should not be cyclic");
    assert!(!graph.is_cyclic("C"), "linear deps: C should not be cyclic");
  }

  {
    let spec = spec_with_schemas(&json!({
      "A": {
        "type": "object",
        "properties": {
          "b": {"$ref": "#/components/schemas/B"}
        }
      },
      "B": {
        "type": "object",
        "properties": {
          "a": {"$ref": "#/components/schemas/A"}
        }
      }
    }));
    let mut stats = GenerationStats::default();
    let mut graph = SchemaRegistry::new(&spec, &mut stats);
    let union_fingerprints = BTreeMap::new();
    graph.build_dependencies(&union_fingerprints);
    let cycles = graph.detect_cycles();

    assert_eq!(cycles.len(), 1, "simple cycle: should detect 1 cycle");
    assert!(!cycles[0].is_empty(), "simple cycle: cycle should not be empty");
    assert!(graph.is_cyclic("A"), "simple cycle: A should be cyclic");
    assert!(graph.is_cyclic("B"), "simple cycle: B should be cyclic");
  }

  {
    let spec = spec_with_schemas(&json!({
      "A": {
        "type": "object",
        "properties": {
          "self_ref": {"$ref": "#/components/schemas/A"}
        }
      }
    }));
    let mut stats = GenerationStats::default();
    let mut graph = SchemaRegistry::new(&spec, &mut stats);
    let union_fingerprints = BTreeMap::new();
    graph.build_dependencies(&union_fingerprints);
    let cycles = graph.detect_cycles();

    assert_eq!(cycles.len(), 1, "self-ref: should detect 1 cycle");
    assert!(graph.is_cyclic("A"), "self-ref: A should be cyclic");
  }

  {
    let spec = spec_with_schemas(&json!({
      "User": {
        "type": "object",
        "properties": {
          "posts": {"$ref": "#/components/schemas/Post"}
        }
      },
      "Post": {
        "type": "object",
        "properties": {
          "author": {"$ref": "#/components/schemas/User"}
        }
      }
    }));
    let mut stats = GenerationStats::default();
    let mut graph = SchemaRegistry::new(&spec, &mut stats);
    let union_fingerprints = BTreeMap::new();
    graph.build_dependencies(&union_fingerprints);
    let cycles = graph.detect_cycles();

    assert!(!cycles.is_empty(), "user-post cycle: should detect cycles");
    assert!(graph.is_cyclic("User"), "user-post cycle: User should be cyclic");
    assert!(graph.is_cyclic("Post"), "user-post cycle: Post should be cyclic");
  }
}

#[test]
fn test_schema_registry_merges_all_of_properties_and_required() {
  let spec = spec_with_schemas(&json!({
    "Loaf": {
      "type": "object",
      "required": ["tag_id"],
      "properties": {
        "tag_id": {"type": "integer"}
      },
      "additionalProperties": true
    },
    "Nugget": {
      "type": "object",
      "required": ["name"],
      "properties": {
        "name": {"type": "string"}
      },
      "allOf": [{"$ref": "#/components/schemas/Loaf"}]
    }
  }));

  let mut stats = GenerationStats::default();
  let mut graph = SchemaRegistry::new(&spec, &mut stats);
  let union_fingerprints = BTreeMap::new();
  graph.build_dependencies(&union_fingerprints);
  graph.detect_cycles();

  let merged = graph.merged("Nugget").expect("merged schema should exist for Nugget");

  assert!(merged.schema.properties.contains_key("tag_id"));
  assert!(merged.schema.properties.contains_key("name"));
  assert!(merged.schema.required.contains(&"tag_id".to_string()));
  assert!(merged.schema.required.contains(&"name".to_string()));
  assert!(merged.schema.additional_properties.is_some());
}

#[test]
fn test_schema_registry_merges_and_tracks_discriminator_parents() {
  let spec = spec_with_schemas(&json!({
    "Loaf": {
      "type": "object",
      "properties": {
        "kind": {"type": "string"}
      },
      "discriminator": {
        "propertyName": "kind",
        "mapping": {
          "nugget": "#/components/schemas/Nugget"
        }
      }
    },
    "Nugget": {
      "type": "object",
      "properties": {
        "nugget_prop": {"type": "integer"}
      },
      "allOf": [{"$ref": "#/components/schemas/Loaf"}]
    }
  }));

  let mut stats = GenerationStats::default();
  let mut graph = SchemaRegistry::new(&spec, &mut stats);
  let union_fingerprints = BTreeMap::new();
  graph.build_dependencies(&union_fingerprints);
  graph.detect_cycles();

  let merged_nugget = graph.merged("Nugget").expect("merged schema should exist for Nugget");

  assert_eq!(merged_nugget.discriminator_parent.as_deref(), Some("Loaf"));
  assert!(merged_nugget.schema.properties.contains_key("kind"));
  assert!(merged_nugget.schema.properties.contains_key("nugget_prop"));

  let parent_name = graph.parent("Nugget").expect("discriminator parent should be tracked");

  assert_eq!(parent_name, "Loaf");

  let effective = graph.resolved("Nugget").unwrap();
  assert_eq!(effective.properties.len(), merged_nugget.schema.properties.len());
}

#[test]
fn schema_merger_conflict_resolution() {
  let spec = spec_with_schemas(&json!({
    "Loaf": {
      "type": "object",
      "properties": {
        "prop": {"type": "string"}
      }
    },
    "Nugget": {
      "type": "object",
      "properties": {
        "prop": {"type": "integer"}
      },
      "allOf": [{"$ref": "#/components/schemas/Loaf"}]
    }
  }));

  let mut stats = GenerationStats::default();
  let mut graph = SchemaRegistry::new(&spec, &mut stats);
  let union_fingerprints = BTreeMap::new();
  graph.build_dependencies(&union_fingerprints);
  graph.detect_cycles();

  let merged = graph.merged("Nugget").expect("merged schema should exist for Nugget");

  let prop = merged.schema.properties.get("prop").unwrap();
  if let ObjectOrReference::Object(schema) = prop {
    assert_eq!(
      schema.schema_type,
      Some(SchemaTypeSet::Single(SchemaType::Integer)),
      "nugget property should override loaf"
    );
  } else {
    panic!("Expected Object schema");
  }
}

#[test]
fn schema_merger_merge_multiple_all_of() {
  let spec = spec_with_schemas(&json!({
    "Corgi": {
      "type": "object",
      "required": ["corgi_prop"],
      "properties": {
        "corgi_prop": {"type": "string"}
      }
    },
    "Fluff": {
      "type": "object",
      "properties": {
        "fluff_prop": {"type": "integer"}
      }
    },
    "Composite": {
      "type": "object",
      "allOf": [
        {"$ref": "#/components/schemas/Corgi"},
        {"$ref": "#/components/schemas/Fluff"}
      ],
      "properties": {
        "own_prop": {"type": "boolean"}
      }
    }
  }));

  let mut stats = GenerationStats::default();
  let mut graph = SchemaRegistry::new(&spec, &mut stats);
  let union_fingerprints = BTreeMap::new();
  graph.build_dependencies(&union_fingerprints);
  graph.detect_cycles();

  let merged = graph
    .merged("Composite")
    .expect("merged schema should exist for Composite");

  assert!(
    merged.schema.properties.contains_key("corgi_prop"),
    "should have corgi_prop"
  );
  assert!(
    merged.schema.properties.contains_key("fluff_prop"),
    "should have fluff_prop"
  );
  assert!(
    merged.schema.properties.contains_key("own_prop"),
    "should have own_prop"
  );
  assert!(
    merged.schema.required.contains(&"corgi_prop".to_string()),
    "corgi_prop should be required"
  );
}

#[test]
fn implicit_discriminator_mapping_from_const_values() {
  let spec = spec_with_schemas(&json!({
    "Allergies": {
      "type": "object",
      "properties": {
        "type": {"type": "string", "const": "allergies"}
      },
      "required": ["type"]
    },
    "Diet": {
      "type": "object",
      "properties": {
        "type": {"type": "string", "const": "diet"}
      },
      "required": ["type"]
    },
    "Health": {
      "type": "object",
      "oneOf": [
        {"$ref": "#/components/schemas/Allergies"},
        {"$ref": "#/components/schemas/Diet"}
      ],
      "discriminator": {"propertyName": "type"}
    }
  }));

  let mut stats = GenerationStats::default();
  let registry = SchemaRegistry::new(&spec, &mut stats);

  let allergies_mapping = registry.mapping("Allergies");
  assert!(
    allergies_mapping.is_some(),
    "Allergies should have a synthesized mapping"
  );
  let am = allergies_mapping.unwrap();
  assert_eq!(am.field_name, "type", "field_name should be 'type'");
  assert_eq!(am.field_value, "allergies", "field_value should be 'allergies'");

  let diet_mapping = registry.mapping("Diet");
  assert!(diet_mapping.is_some(), "Diet should have a synthesized mapping");
  let dm = diet_mapping.unwrap();
  assert_eq!(dm.field_name, "type", "field_name should be 'type'");
  assert_eq!(dm.field_value, "diet", "field_value should be 'diet'");

  assert!(stats.warnings.is_empty(), "should have no warnings");
}

#[test]
fn implicit_discriminator_mapping_warns_on_missing_const() {
  let spec = spec_with_schemas(&json!({
    "Allergies": {
      "type": "object",
      "properties": {
        "type": {"type": "string", "const": "allergies"}
      },
      "required": ["type"]
    },
    "Diet": {
      "type": "object",
      "properties": {
        "type": {"type": "string"}
      }
    },
    "Health": {
      "type": "object",
      "oneOf": [
        {"$ref": "#/components/schemas/Allergies"},
        {"$ref": "#/components/schemas/Diet"}
      ],
      "discriminator": {"propertyName": "type"}
    }
  }));

  let mut stats = GenerationStats::default();
  let registry = SchemaRegistry::new(&spec, &mut stats);

  assert!(
    registry.mapping("Allergies").is_none(),
    "no mapping should be synthesized"
  );
  assert!(registry.mapping("Diet").is_none(), "no mapping should be synthesized");
  assert_eq!(stats.warnings.len(), 1, "should have one warning");
  assert!(
    matches!(&stats.warnings[0], GenerationWarning::DiscriminatorMappingFailed { schema_name, message }
      if schema_name == "Health" && message.contains("Diet") && message.contains("no string const")),
    "warning should mention Diet missing const: {:?}",
    stats.warnings[0]
  );
}

#[test]
fn implicit_discriminator_mapping_warns_on_duplicate_const() {
  let spec = spec_with_schemas(&json!({
    "Allergies": {
      "type": "object",
      "properties": {
        "type": {"type": "string", "const": "same_value"}
      },
      "required": ["type"]
    },
    "Diet": {
      "type": "object",
      "properties": {
        "type": {"type": "string", "const": "same_value"}
      },
      "required": ["type"]
    },
    "Health": {
      "type": "object",
      "oneOf": [
        {"$ref": "#/components/schemas/Allergies"},
        {"$ref": "#/components/schemas/Diet"}
      ],
      "discriminator": {"propertyName": "type"}
    }
  }));

  let mut stats = GenerationStats::default();
  let registry = SchemaRegistry::new(&spec, &mut stats);

  assert!(
    registry.mapping("Allergies").is_none(),
    "no mapping should be synthesized"
  );
  assert!(registry.mapping("Diet").is_none(), "no mapping should be synthesized");
  assert_eq!(stats.warnings.len(), 1, "should have one warning");
  assert!(
    matches!(&stats.warnings[0], GenerationWarning::DiscriminatorMappingFailed { schema_name, message }
      if schema_name == "Health" && message.contains("duplicate")),
    "warning should mention duplicate: {:?}",
    stats.warnings[0]
  );
}

#[test]
fn explicit_mapping_takes_precedence_over_const() {
  let spec = spec_with_schemas(&json!({
    "Allergies": {
      "type": "object",
      "properties": {
        "type": {"type": "string", "const": "allergies"}
      },
      "required": ["type"]
    },
    "Diet": {
      "type": "object",
      "properties": {
        "type": {"type": "string", "const": "diet"}
      },
      "required": ["type"]
    },
    "Health": {
      "type": "object",
      "oneOf": [
        {"$ref": "#/components/schemas/Allergies"},
        {"$ref": "#/components/schemas/Diet"}
      ],
      "discriminator": {
        "propertyName": "type",
        "mapping": {
          "allergy_override": "#/components/schemas/Allergies",
          "diet_override": "#/components/schemas/Diet"
        }
      }
    }
  }));

  let mut stats = GenerationStats::default();
  let registry = SchemaRegistry::new(&spec, &mut stats);

  let am = registry.mapping("Allergies").expect("Allergies should have a mapping");
  assert_eq!(
    am.field_value, "allergy_override",
    "explicit mapping should take precedence"
  );

  let dm = registry.mapping("Diet").expect("Diet should have a mapping");
  assert_eq!(
    dm.field_value, "diet_override",
    "explicit mapping should take precedence"
  );

  assert!(stats.warnings.is_empty(), "should have no warnings");
}

#[test]
fn effective_mapping_synthesizes_from_cache() {
  let health_json = json!({
    "type": "object",
    "oneOf": [
      {"$ref": "#/components/schemas/Allergies"},
      {"$ref": "#/components/schemas/Diet"}
    ],
    "discriminator": {"propertyName": "type"}
  });
  let spec = spec_with_schemas(&json!({
    "Allergies": {
      "type": "object",
      "properties": {
        "type": {"type": "string", "const": "allergies"}
      },
      "required": ["type"]
    },
    "Diet": {
      "type": "object",
      "properties": {
        "type": {"type": "string", "const": "diet"}
      },
      "required": ["type"]
    },
    "Health": health_json.clone()
  }));

  let mut stats = GenerationStats::default();
  let registry = SchemaRegistry::new(&spec, &mut stats);

  let health: ObjectSchema = serde_json::from_value(health_json).expect("failed to parse health schema");
  let effective = registry.effective_mapping(&health);
  assert!(effective.is_some(), "effective mapping should be available");

  let mapping = effective.unwrap();
  assert_eq!(mapping.len(), 2, "should have 2 entries");
  assert_eq!(
    mapping.get("allergies").map(std::string::String::as_str),
    Some(format!("{SCHEMA_REF_PREFIX}Allergies").as_str()),
    "allergies value should map to Allergies ref"
  );
  assert_eq!(
    mapping.get("diet").map(std::string::String::as_str),
    Some(format!("{SCHEMA_REF_PREFIX}Diet").as_str()),
    "diet value should map to Diet ref"
  );
}

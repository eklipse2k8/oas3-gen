use std::collections::{HashMap, HashSet};

use crate::generator::{ast::CodeMetadata, codegen::Visibility, orchestrator::Orchestrator};

fn make_orchestrator(spec: oas3::Spec, all_schemas: bool) -> Orchestrator {
  Orchestrator::new(
    spec,
    Visibility::default(),
    all_schemas,
    None,
    None,
    false,
    false,
    false,
    false,
    HashMap::new(),
  )
}

fn make_orchestrator_with_ops(
  spec: oas3::Spec,
  all_schemas: bool,
  only: Option<&HashSet<String>>,
  exclude: Option<&HashSet<String>>,
) -> Orchestrator {
  Orchestrator::new(
    spec,
    Visibility::default(),
    all_schemas,
    only,
    exclude,
    false,
    false,
    false,
    false,
    HashMap::new(),
  )
}

#[test]
fn test_metadata_and_header_generation() {
  let spec_json = include_str!("../../../fixtures/basic_api.json");
  let spec: oas3::Spec = oas3::from_json(spec_json).unwrap();
  let metadata = CodeMetadata::from(&spec);

  assert_eq!(metadata.title, "Basic Test API", "title mismatch");
  assert_eq!(metadata.version, "1.0.0", "version mismatch");
  assert_eq!(
    metadata.description.as_deref(),
    Some("A test API.\nWith multiple lines.\nFor testing documentation."),
    "description mismatch"
  );

  let orchestrator = make_orchestrator(spec, false);
  let result = orchestrator.generate_with_header("/path/to/spec.json");
  assert!(result.is_ok(), "generate_with_header failed");

  let (code, _) = result.unwrap();
  let header_checks = [
    ("AUTO-GENERATED CODE - DO NOT EDIT!", "auto-generated marker"),
    ("//! Basic Test API", "title in header"),
    ("//! Source: /path/to/spec.json", "source path"),
    ("//! Version: 1.0.0", "version in header"),
    ("//! A test API.", "description in header"),
    ("#![allow(clippy::doc_markdown)]", "clippy allow"),
    (
      "A test API.\n//! With multiple lines.\n//! For testing documentation.",
      "multiline description formatting",
    ),
  ];
  for (expected, context) in header_checks {
    assert!(code.contains(expected), "missing {context}: expected '{expected}'");
  }
}

#[test]
fn test_operation_filtering() {
  let spec_json = include_str!("../../../fixtures/operation_filtering.json");

  let mut excluded = HashSet::new();
  excluded.insert("create_user".to_string());
  let spec: oas3::Spec = oas3::from_json(spec_json).unwrap();
  let orchestrator = make_orchestrator_with_ops(spec, false, None, Some(&excluded));
  let (code, stats) = orchestrator.generate_with_header("test.json").unwrap();
  assert_eq!(stats.operations_converted, 2, "excluded create_user should leave 2 ops");
  assert!(
    !code.contains("create_user"),
    "create_user should be excluded from code"
  );

  let spec_full: oas3::Spec = oas3::from_json(spec_json).unwrap();
  let orchestrator_full = make_orchestrator(spec_full, false);
  let (code_full, stats_full) = orchestrator_full.generate_with_header("test.json").unwrap();

  let spec_filtered: oas3::Spec = oas3::from_json(spec_json).unwrap();
  let mut excluded_admin = HashSet::new();
  excluded_admin.insert("admin_action".to_string());
  let orchestrator_filtered = make_orchestrator_with_ops(spec_filtered, false, None, Some(&excluded_admin));
  let (code_filtered, stats_filtered) = orchestrator_filtered.generate_with_header("test.json").unwrap();

  assert_eq!(stats_full.operations_converted, 3, "full spec should have 3 ops");
  assert_eq!(
    stats_filtered.operations_converted, 2,
    "filtered spec should have 2 ops"
  );
  assert!(
    code_full.contains("AdminResponse"),
    "full code should contain AdminResponse"
  );
  assert!(
    !code_filtered.contains("AdminResponse"),
    "filtered code should not contain AdminResponse"
  );
  assert!(
    code_filtered.contains("UserList"),
    "filtered code should still contain UserList"
  );
  assert!(
    code_filtered.contains("User"),
    "filtered code should still contain User"
  );
}

#[test]
fn test_all_schemas_overrides_operation_filtering() {
  let spec_json = include_str!("../../../fixtures/operation_filtering.json");

  let mut only = HashSet::new();
  only.insert("list_users".to_string());

  let spec_without: oas3::Spec = oas3::from_json(spec_json).unwrap();
  let orchestrator_without = make_orchestrator_with_ops(spec_without, false, Some(&only), None);
  let (code_without, stats_without) = orchestrator_without.generate_with_header("test.json").unwrap();

  let spec_with: oas3::Spec = oas3::from_json(spec_json).unwrap();
  let orchestrator_with = make_orchestrator_with_ops(spec_with, true, Some(&only), None);
  let (code_with, stats_with) = orchestrator_with.generate_with_header("test.json").unwrap();

  assert_eq!(stats_without.operations_converted, 1, "without all_schemas: 1 op");
  assert_eq!(stats_with.operations_converted, 1, "with all_schemas: still 1 op");

  let without_checks = [
    (true, "UserList", "should contain UserList"),
    (true, "User", "should contain User"),
    (false, "AdminResponse", "should not contain AdminResponse"),
    (false, "UnreferencedSchema", "should not contain UnreferencedSchema"),
  ];
  for (should_contain, schema, context) in without_checks {
    assert_eq!(
      code_without.contains(schema),
      should_contain,
      "without all_schemas: {context}"
    );
  }

  let with_checks = ["UserList", "User", "AdminResponse", "UnreferencedSchema"];
  for schema in with_checks {
    assert!(code_with.contains(schema), "with all_schemas: should contain {schema}");
  }

  assert_eq!(
    stats_without.orphaned_schemas_count, 2,
    "without all_schemas: 2 orphaned"
  );
  assert_eq!(stats_with.orphaned_schemas_count, 0, "with all_schemas: 0 orphaned");
}

#[test]
fn test_content_types_generation() {
  let spec_json = include_str!("../../../fixtures/content_types.json");
  let spec: oas3::Spec = oas3::from_json(spec_json).unwrap();
  let orchestrator = make_orchestrator(spec, false);

  let (code, _) = orchestrator.generate_with_header("test.json").unwrap();

  let content_type_checks = [
    ("json_with_diagnostics", "JSON handling for application/json"),
    ("req.text().await?", "text handling for text/plain"),
    ("req.bytes().await?", "binary handling for image/png"),
  ];
  for (expected, context) in content_type_checks {
    assert!(code.contains(expected), "missing {context}");
  }
}

#[test]
fn test_enum_deduplication() {
  let cases = [
    (
      include_str!("../../../fixtures/enum_deduplication.json"),
      vec![
        ("pub enum Status", 1, "Status enum should be defined exactly once"),
        ("pub status: Option<Status>", 1, "StructA should use Status"),
        ("status: Option<Status>", 3, "Multiple structs should use Status"),
      ],
      vec![],
    ),
    (
      include_str!("../../../fixtures/relaxed_enum_deduplication.json"),
      vec![
        ("pub enum Status", 1, "Status enum should be defined"),
        ("pub enum ComplexStatusStatus", 1, "Outer enum should be defined"),
        ("Known(Status)", 1, "Outer enum should wrap Status"),
      ],
      vec![("pub enum ComplexStatusStatusKnown", "Inner enum should be deduplicated")],
    ),
  ];

  for (spec_json, presence_checks, absence_checks) in cases {
    let spec: oas3::Spec = oas3::from_json(spec_json).unwrap();
    let orchestrator = make_orchestrator(spec, true);
    let (code, _) = orchestrator.generate_with_header("test.json").unwrap();

    for (pattern, expected_count, context) in &presence_checks {
      let actual_count = code.matches(pattern).count();
      assert!(
        actual_count >= *expected_count,
        "{context}: expected at least {expected_count} occurrences of '{pattern}', found {actual_count}"
      );
    }

    for (pattern, context) in &absence_checks {
      assert!(!code.contains(pattern), "{context}: '{pattern}' should not appear");
    }
  }
}

fn make_orchestrator_with_customizations(
  spec: oas3::Spec,
  all_schemas: bool,
  customizations: HashMap<String, String>,
) -> Orchestrator {
  Orchestrator::new(
    spec,
    Visibility::default(),
    all_schemas,
    None,
    None,
    false,
    false,
    false,
    false,
    customizations,
  )
}

#[test]
fn test_customization_generates_serde_as_attributes() {
  let spec_json = r#"{
    "openapi": "3.0.0",
    "info": { "title": "Test API", "version": "1.0.0" },
    "paths": {},
    "components": {
      "schemas": {
        "Event": {
          "type": "object",
          "properties": {
            "id": { "type": "string" },
            "created_at": { "type": "string", "format": "date-time" },
            "updated_at": { "type": "string", "format": "date-time" }
          },
          "required": ["id", "created_at"]
        }
      }
    }
  }"#;

  let spec: oas3::Spec = oas3::from_json(spec_json).unwrap();
  let customizations = HashMap::from([("date_time".to_string(), "crate::MyDateTime".to_string())]);
  let orchestrator = make_orchestrator_with_customizations(spec, true, customizations);
  let (code, _) = orchestrator.generate_with_header("test.json").unwrap();

  assert!(
    code.contains("#[serde_with::serde_as]"),
    "Struct should have #[serde_with::serde_as] outer attribute"
  );
  assert!(
    code.contains(r#"#[serde_as(as = "crate::MyDateTime")]"#),
    "Required field should have serde_as attribute with custom type"
  );
  assert!(
    code.contains(r#"#[serde_as(as = "Option<crate::MyDateTime>")]"#),
    "Optional field should have serde_as attribute wrapped in Option"
  );
}

#[test]
fn test_customization_for_multiple_types() {
  let spec_json = r#"{
    "openapi": "3.0.0",
    "info": { "title": "Test API", "version": "1.0.0" },
    "paths": {},
    "components": {
      "schemas": {
        "Entity": {
          "type": "object",
          "properties": {
            "id": { "type": "string", "format": "uuid" },
            "created_at": { "type": "string", "format": "date-time" },
            "birth_date": { "type": "string", "format": "date" }
          },
          "required": ["id", "created_at", "birth_date"]
        }
      }
    }
  }"#;

  let spec: oas3::Spec = oas3::from_json(spec_json).unwrap();
  let customizations = HashMap::from([
    ("date_time".to_string(), "crate::MyDateTime".to_string()),
    ("date".to_string(), "crate::MyDate".to_string()),
    ("uuid".to_string(), "crate::MyUuid".to_string()),
  ]);
  let orchestrator = make_orchestrator_with_customizations(spec, true, customizations);
  let (code, _) = orchestrator.generate_with_header("test.json").unwrap();

  assert!(
    code.contains(r#"#[serde_as(as = "crate::MyDateTime")]"#),
    "date-time field should have custom type"
  );
  assert!(
    code.contains(r#"#[serde_as(as = "crate::MyDate")]"#),
    "date field should have custom type"
  );
  assert!(
    code.contains(r#"#[serde_as(as = "crate::MyUuid")]"#),
    "uuid field should have custom type"
  );
}

#[test]
fn test_customization_for_array_types() {
  let spec_json = r#"{
    "openapi": "3.0.0",
    "info": { "title": "Test API", "version": "1.0.0" },
    "paths": {},
    "components": {
      "schemas": {
        "Timeline": {
          "type": "object",
          "properties": {
            "timestamps": {
              "type": "array",
              "items": { "type": "string", "format": "date-time" }
            }
          },
          "required": ["timestamps"]
        }
      }
    }
  }"#;

  let spec: oas3::Spec = oas3::from_json(spec_json).unwrap();
  let customizations = HashMap::from([("date_time".to_string(), "crate::MyDateTime".to_string())]);
  let orchestrator = make_orchestrator_with_customizations(spec, true, customizations);
  let (code, _) = orchestrator.generate_with_header("test.json").unwrap();

  assert!(
    code.contains(r#"#[serde_as(as = "Vec<crate::MyDateTime>")]"#),
    "Array field should have serde_as with Vec wrapper"
  );
}

#[test]
fn test_no_customization_no_serde_as() {
  let spec_json = r#"{
    "openapi": "3.0.0",
    "info": { "title": "Test API", "version": "1.0.0" },
    "paths": {},
    "components": {
      "schemas": {
        "Event": {
          "type": "object",
          "properties": {
            "id": { "type": "string" },
            "created_at": { "type": "string", "format": "date-time" }
          },
          "required": ["id", "created_at"]
        }
      }
    }
  }"#;

  let spec: oas3::Spec = oas3::from_json(spec_json).unwrap();
  let orchestrator = make_orchestrator(spec, true);
  let (code, _) = orchestrator.generate_with_header("test.json").unwrap();

  assert!(
    !code.contains("#[serde_as(as ="),
    "Code should not contain serde_as field attribute without customizations"
  );
  assert!(
    !code.contains("#[serde_with::serde_as]") || !code.contains("Event"),
    "Event struct should not have serde_as outer attribute without customizations"
  );
}

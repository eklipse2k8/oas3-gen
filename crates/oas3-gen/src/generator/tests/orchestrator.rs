use std::collections::{HashMap, HashSet};

use crate::generator::{
  CodegenConfig, TypesMode,
  ast::{ClientRootNode, StructToken},
  codegen::{GeneratedFileType, Visibility},
  orchestrator::Orchestrator,
};

fn make_orchestrator(spec: oas3::Spec, all_schemas: bool) -> Orchestrator {
  Orchestrator::new(
    spec,
    Visibility::default(),
    CodegenConfig::default(),
    None,
    None,
    all_schemas,
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
    CodegenConfig::default(),
    only,
    exclude,
    all_schemas,
  )
}

#[test]
fn test_metadata_and_header_generation() {
  let spec_json = include_str!("../../../fixtures/basic_api.json");
  let spec: oas3::Spec = oas3::from_json(spec_json).unwrap();
  let metadata = ClientRootNode::builder()
    .name(StructToken::new("PembrokeApiClient"))
    .info(&spec.info)
    .servers(&spec.servers)
    .build();

  assert_eq!(metadata.title, "Basic Test API", "title mismatch");
  assert_eq!(metadata.version, "1.0.0", "version mismatch");
  assert_eq!(
    metadata.description.as_deref(),
    Some("A test API.\nWith multiple lines.\nFor testing documentation."),
    "description mismatch"
  );

  let orchestrator = make_orchestrator(spec, false);
  let result = orchestrator.generate(&TypesMode, "/path/to/spec.json");
  assert!(result.is_ok(), "generate_with_header failed");

  let output = result.unwrap();
  let code = output.code.code(&GeneratedFileType::Types).unwrap();
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
  excluded.insert("admin_action".to_string());
  let spec: oas3::Spec = oas3::from_json(spec_json).unwrap();
  let orchestrator = make_orchestrator_with_ops(spec, false, None, Some(&excluded));
  let output = orchestrator.generate(&TypesMode, "test.json").unwrap();
  let code = output.code.code(&GeneratedFileType::Types).unwrap();
  let stats = &output.stats;
  assert_eq!(
    stats.operations_converted, 2,
    "excluded admin_action should leave 2 ops"
  );
  assert!(
    !code.contains("admin_action"),
    "admin_action should be excluded from code"
  );

  let spec_full: oas3::Spec = oas3::from_json(spec_json).unwrap();
  let orchestrator_full = make_orchestrator(spec_full, false);
  let output_full = orchestrator_full.generate(&TypesMode, "test.json").unwrap();
  let code_full = output_full.code.code(&GeneratedFileType::Types).unwrap();
  let stats_full = &output_full.stats;

  let spec_filtered: oas3::Spec = oas3::from_json(spec_json).unwrap();
  let mut excluded_admin = HashSet::new();
  excluded_admin.insert("admin_action".to_string());
  let orchestrator_filtered = make_orchestrator_with_ops(spec_filtered, false, None, Some(&excluded_admin));
  let output_filtered = orchestrator_filtered.generate(&TypesMode, "test.json").unwrap();
  let code_filtered = output_filtered.code.code(&GeneratedFileType::Types).unwrap();
  let stats_filtered = &output_filtered.stats;

  assert_eq!(stats_full.operations_converted, 3, "full spec should have 3 ops");
  assert_eq!(
    stats_filtered.operations_converted, 2,
    "filtered spec should have 2 ops"
  );
  assert!(
    code_full.contains("AdminActionResponse"),
    "full code should contain AdminActionResponse"
  );
  assert!(
    !code_filtered.contains("AdminActionResponse"),
    "filtered code should not contain AdminActionResponse"
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
  let output_without = orchestrator_without.generate(&TypesMode, "test.json").unwrap();
  let code_without = output_without.code.code(&GeneratedFileType::Types).unwrap();
  let stats_without = &output_without.stats;

  let spec_with: oas3::Spec = oas3::from_json(spec_json).unwrap();
  let orchestrator_with = make_orchestrator_with_ops(spec_with, true, Some(&only), None);
  let output_with = orchestrator_with.generate(&TypesMode, "test.json").unwrap();
  let code_with = output_with.code.code(&GeneratedFileType::Types).unwrap();
  let stats_with = &output_with.stats;

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

  let output = orchestrator.generate(&TypesMode, "test.json").unwrap();
  let code = output.code.code(&GeneratedFileType::Types).unwrap();

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
    let output = orchestrator.generate(&TypesMode, "test.json").unwrap();
    let code = output.code.code(&GeneratedFileType::Types).unwrap();

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
  let config = CodegenConfig::builder().customizations(customizations).build();
  Orchestrator::new(spec, Visibility::default(), config, None, None, all_schemas)
}

#[test]
fn test_customization_generates_serde_as_attributes() {
  let spec_json = r#"{
    "openapi": "3.0.0",
    "info": { "title": "Test API", "version": "1.0.0" },
    "paths": {},
    "components": {
      "schemas": {
        "Frappe": {
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
  let output = orchestrator.generate(&TypesMode, "test.json").unwrap();
  let code = output.code.code(&GeneratedFileType::Types).unwrap();

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
    "info": { "title": "Pembroke API", "version": "1.0.0" },
    "paths": {},
    "components": {
      "schemas": {
        "Cardigan": {
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
  let output = orchestrator.generate(&TypesMode, "test.json").unwrap();
  let code = output.code.code(&GeneratedFileType::Types).unwrap();

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
    "info": { "title": "Pembroke API", "version": "1.0.0" },
    "paths": {},
    "components": {
      "schemas": {
        "WaddleLine": {
          "type": "object",
          "properties": {
            "toebeans": {
              "type": "array",
              "items": { "type": "string", "format": "date-time" }
            }
          },
          "required": ["toebeans"]
        }
      }
    }
  }"#;

  let spec: oas3::Spec = oas3::from_json(spec_json).unwrap();
  let customizations = HashMap::from([("date_time".to_string(), "crate::MyDateTime".to_string())]);
  let orchestrator = make_orchestrator_with_customizations(spec, true, customizations);
  let output = orchestrator.generate(&TypesMode, "test.json").unwrap();
  let code = output.code.code(&GeneratedFileType::Types).unwrap();

  assert!(
    code.contains(r#"#[serde_as(as = "Vec<crate::MyDateTime>")]"#),
    "Array field should have serde_as with Vec wrapper"
  );
}

#[test]
fn test_no_customization_no_serde_as() {
  let spec_json = r#"{
    "openapi": "3.0.0",
    "info": { "title": "Pembroke API", "version": "1.0.0" },
    "paths": {},
    "components": {
      "schemas": {
        "Frappe": {
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
  let output = orchestrator.generate(&TypesMode, "test.json").unwrap();
  let code = output.code.code(&GeneratedFileType::Types).unwrap();

  assert!(
    !code.contains("#[serde_as(as ="),
    "Code should not contain serde_as field attribute without customizations"
  );
  assert!(
    !code.contains("#[serde_with::serde_as]") || !code.contains("Frappe"),
    "Frappe struct should not have serde_as outer attribute without customizations"
  );
}

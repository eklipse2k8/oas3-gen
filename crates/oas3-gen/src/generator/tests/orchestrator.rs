use std::collections::HashMap;

use super::support::{
  assert_contains, assert_contains_all, assert_not_contains, assert_occurs_at_least, generate_types, make_orchestrator,
  make_orchestrator_with_customizations, make_orchestrator_with_ops, parse_spec, string_set,
};
use crate::generator::ast::{ClientRootNode, StructToken};

type PresenceCheck<'a> = (&'a str, usize, &'a str);
type AbsenceCheck<'a> = (&'a str, &'a str);
type EnumDedupCase<'a> = (&'a str, Vec<PresenceCheck<'a>>, Vec<AbsenceCheck<'a>>);

#[test]
fn test_metadata_and_header_generation() {
  let spec = parse_spec(include_str!("../../../fixtures/basic_api.json"));
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
  let output = generate_types(&orchestrator, "/path/to/spec.json");
  assert_contains_all(
    &output.code,
    &[
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
    ],
  );
}

#[test]
fn test_operation_filtering() {
  let spec_json = include_str!("../../../fixtures/operation_filtering.json");
  let excluded = string_set(&["admin_action"]);

  let full_orchestrator = make_orchestrator(parse_spec(spec_json), false);
  let full = generate_types(&full_orchestrator, "test.json");

  let filtered_orchestrator = make_orchestrator_with_ops(parse_spec(spec_json), false, None, Some(&excluded));
  let filtered = generate_types(&filtered_orchestrator, "test.json");

  assert_eq!(full.operations_converted, 3, "full spec should have 3 ops");
  assert_eq!(
    filtered.operations_converted, 2,
    "excluded admin_action should leave 2 ops"
  );
  assert_not_contains(
    &filtered.code,
    "admin_action",
    "admin_action should be excluded from generated code",
  );
  assert_contains(
    &full.code,
    "AdminActionResponse",
    "full code should contain AdminActionResponse",
  );
  assert_not_contains(
    &filtered.code,
    "AdminActionResponse",
    "filtered code should not contain AdminActionResponse",
  );
  assert_contains(
    &filtered.code,
    "UserList",
    "filtered code should still contain UserList",
  );
  assert_contains(&filtered.code, "User", "filtered code should still contain User");
}

#[test]
fn test_all_schemas_overrides_operation_filtering() {
  let spec_json = include_str!("../../../fixtures/operation_filtering.json");
  let only = string_set(&["list_users"]);

  let without_all_schemas_orchestrator = make_orchestrator_with_ops(parse_spec(spec_json), false, Some(&only), None);
  let without_all_schemas = generate_types(&without_all_schemas_orchestrator, "test.json");

  let with_all_schemas_orchestrator = make_orchestrator_with_ops(parse_spec(spec_json), true, Some(&only), None);
  let with_all_schemas = generate_types(&with_all_schemas_orchestrator, "test.json");

  assert_eq!(without_all_schemas.operations_converted, 1, "without all_schemas: 1 op");
  assert_eq!(with_all_schemas.operations_converted, 1, "with all_schemas: still 1 op");

  assert_contains(
    &without_all_schemas.code,
    "UserList",
    "without all_schemas should contain UserList",
  );
  assert_contains(
    &without_all_schemas.code,
    "User",
    "without all_schemas should contain User",
  );
  assert_not_contains(
    &without_all_schemas.code,
    "AdminResponse",
    "without all_schemas should not contain AdminResponse",
  );
  assert_not_contains(
    &without_all_schemas.code,
    "UnreferencedSchema",
    "without all_schemas should not contain UnreferencedSchema",
  );

  assert_contains_all(
    &with_all_schemas.code,
    &[
      ("UserList", "with all_schemas should contain UserList"),
      ("User", "with all_schemas should contain User"),
      ("AdminResponse", "with all_schemas should contain AdminResponse"),
      (
        "UnreferencedSchema",
        "with all_schemas should contain UnreferencedSchema",
      ),
    ],
  );

  assert_eq!(
    without_all_schemas.orphaned_schemas_count, 2,
    "without all_schemas: 2 orphaned"
  );
  assert_eq!(
    with_all_schemas.orphaned_schemas_count, 0,
    "with all_schemas: 0 orphaned"
  );
}

#[test]
fn test_content_types_generation() {
  let orchestrator = make_orchestrator(parse_spec(include_str!("../../../fixtures/content_types.json")), false);
  let output = generate_types(&orchestrator, "test.json");
  assert_contains_all(
    &output.code,
    &[
      (
        "json_with_diagnostics",
        "JSON handling for application/json should be generated",
      ),
      ("req.text().await?", "text handling for text/plain should be generated"),
      (
        "req.bytes().await?",
        "binary handling for image/png should be generated",
      ),
    ],
  );
}

#[test]
fn test_enum_deduplication() {
  let cases: [EnumDedupCase<'_>; 2] = [
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
    let orchestrator = make_orchestrator(parse_spec(spec_json), true);
    let output = generate_types(&orchestrator, "test.json");
    for (pattern, expected_count, context) in presence_checks {
      assert_occurs_at_least(&output.code, pattern, expected_count, context);
    }
    for (pattern, context) in absence_checks {
      assert_not_contains(&output.code, pattern, context);
    }
  }
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

  let customizations = HashMap::from([("date_time".to_string(), "crate::MyDateTime".to_string())]);
  let orchestrator = make_orchestrator_with_customizations(parse_spec(spec_json), true, customizations);
  let output = generate_types(&orchestrator, "test.json");

  assert_contains(
    &output.code,
    "#[serde_with::serde_as]",
    "Struct should have #[serde_with::serde_as] outer attribute",
  );
  assert_contains(
    &output.code,
    r#"#[serde_as(as = "crate::MyDateTime")]"#,
    "required field should have serde_as attribute with custom type",
  );
  assert_contains(
    &output.code,
    r#"#[serde_as(as = "Option<crate::MyDateTime>")]"#,
    "optional field should have serde_as attribute wrapped in Option",
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

  let customizations = HashMap::from([
    ("date_time".to_string(), "crate::MyDateTime".to_string()),
    ("date".to_string(), "crate::MyDate".to_string()),
    ("uuid".to_string(), "crate::MyUuid".to_string()),
  ]);
  let orchestrator = make_orchestrator_with_customizations(parse_spec(spec_json), true, customizations);
  let output = generate_types(&orchestrator, "test.json");

  assert_contains(
    &output.code,
    r#"#[serde_as(as = "crate::MyDateTime")]"#,
    "date-time field should have custom type",
  );
  assert_contains(
    &output.code,
    r#"#[serde_as(as = "crate::MyDate")]"#,
    "date field should have custom type",
  );
  assert_contains(
    &output.code,
    r#"#[serde_as(as = "crate::MyUuid")]"#,
    "uuid field should have custom type",
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

  let customizations = HashMap::from([("date_time".to_string(), "crate::MyDateTime".to_string())]);
  let orchestrator = make_orchestrator_with_customizations(parse_spec(spec_json), true, customizations);
  let output = generate_types(&orchestrator, "test.json");

  assert_contains(
    &output.code,
    r#"#[serde_as(as = "Vec<crate::MyDateTime>")]"#,
    "array field should have serde_as with Vec wrapper",
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

  let orchestrator = make_orchestrator(parse_spec(spec_json), true);
  let output = generate_types(&orchestrator, "test.json");
  assert_not_contains(
    &output.code,
    "#[serde_as(as =",
    "code should not contain serde_as field attribute without customizations",
  );
  assert!(
    !output.code.contains("#[serde_with::serde_as]") || !output.code.contains("Frappe"),
    "Frappe struct should not have serde_as outer attribute without customizations"
  );
}

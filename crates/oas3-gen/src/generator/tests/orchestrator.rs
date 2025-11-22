use std::collections::HashSet;

use crate::generator::{codegen::Visibility, converter::FieldOptionalityPolicy, orchestrator::Orchestrator};

#[test]
fn test_orchestrator_new_and_metadata() {
  let spec_json = include_str!("../../../fixtures/basic_api.json");
  let spec: oas3::Spec = oas3::from_json(spec_json).unwrap();
  let orchestrator = Orchestrator::new(
    spec,
    Visibility::default(),
    false,
    None,
    None,
    FieldOptionalityPolicy::standard(),
    false,
    false,
    false,
  );

  let metadata = orchestrator.metadata();
  assert_eq!(metadata.title, "Basic Test API");
  assert_eq!(metadata.version, "1.0.0");
  assert_eq!(
    metadata.description.as_deref(),
    Some("A test API.\nWith multiple lines.\nFor testing documentation.")
  );
}

#[test]
fn test_orchestrator_generate_with_header() {
  let spec_json = include_str!("../../../fixtures/basic_api.json");
  let spec: oas3::Spec = oas3::from_json(spec_json).unwrap();
  let orchestrator = Orchestrator::new(
    spec,
    Visibility::default(),
    false,
    None,
    None,
    FieldOptionalityPolicy::standard(),
    false,
    false,
    false,
  );

  let result = orchestrator.generate_with_header("/path/to/spec.json");
  assert!(result.is_ok());

  let (code, _) = result.unwrap();
  assert!(code.contains("AUTO-GENERATED CODE - DO NOT EDIT!"));
  assert!(code.contains("//! Basic Test API"));
  assert!(code.contains("//! Source: /path/to/spec.json"));
  assert!(code.contains("//! Version: 1.0.0"));
  assert!(code.contains("//! A test API."));
  assert!(code.contains("#![allow(clippy::doc_markdown)]"));
}

#[test]
fn test_header_generation_with_multiline_description() {
  let spec_json = include_str!("../../../fixtures/basic_api.json");
  let spec: oas3::Spec = oas3::from_json(spec_json).unwrap();
  let orchestrator = Orchestrator::new(
    spec,
    Visibility::default(),
    false,
    None,
    None,
    FieldOptionalityPolicy::standard(),
    false,
    false,
    false,
  );

  let (header, _) = orchestrator.generate_with_header("test.yaml").unwrap();
  assert!(header.contains("A test API.\n//! With multiple lines.\n//! For testing documentation."));
}

#[test]
fn test_operation_exclusion() {
  let spec_json = include_str!("../../../fixtures/operation_filtering.json");
  let spec: oas3::Spec = oas3::from_json(spec_json).unwrap();
  let mut excluded = HashSet::new();
  excluded.insert("create_user".to_string());

  let orchestrator = Orchestrator::new(
    spec,
    Visibility::default(),
    false,
    None,
    Some(&excluded),
    FieldOptionalityPolicy::standard(),
    false,
    false,
    false,
  );
  let result = orchestrator.generate_with_header("test.json");
  assert!(result.is_ok());

  let (code, stats) = result.unwrap();
  assert_eq!(stats.operations_converted, 2);
  assert!(!code.contains("create_user"));
}

#[test]
fn test_operation_exclusion_affects_schema_reachability() {
  let spec_json = include_str!("../../../fixtures/operation_filtering.json");
  let spec_full: oas3::Spec = oas3::from_json(spec_json).unwrap();
  let orchestrator_full = Orchestrator::new(
    spec_full,
    Visibility::default(),
    false,
    None,
    None,
    FieldOptionalityPolicy::standard(),
    false,
    false,
    false,
  );
  let result_full = orchestrator_full.generate_with_header("test.json");
  assert!(result_full.is_ok());
  let (code_full, stats_full) = result_full.unwrap();

  let spec_filtered: oas3::Spec = oas3::from_json(spec_json).unwrap();
  let mut excluded = HashSet::new();
  excluded.insert("admin_action".to_string());
  let orchestrator_filtered = Orchestrator::new(
    spec_filtered,
    Visibility::default(),
    false,
    None,
    Some(&excluded),
    FieldOptionalityPolicy::standard(),
    false,
    false,
    false,
  );
  let result_filtered = orchestrator_filtered.generate_with_header("test.json");
  assert!(result_filtered.is_ok());
  let (code_filtered, stats_filtered) = result_filtered.unwrap();

  assert_eq!(stats_full.operations_converted, 3);
  assert_eq!(stats_filtered.operations_converted, 2);

  assert!(code_full.contains("AdminResponse"));
  assert!(!code_filtered.contains("AdminResponse"));
  assert!(code_filtered.contains("UserList"));
  assert!(code_filtered.contains("User"));
}

#[test]
fn test_all_schemas_overrides_operation_filtering() {
  let spec_json = include_str!("../../../fixtures/operation_filtering.json");
  let spec_without_all: oas3::Spec = oas3::from_json(spec_json).unwrap();
  let mut only = HashSet::new();
  only.insert("list_users".to_string());
  let orchestrator_without = Orchestrator::new(
    spec_without_all,
    Visibility::default(),
    false,
    Some(&only),
    None,
    FieldOptionalityPolicy::standard(),
    false,
    false,
    false,
  );
  let result_without = orchestrator_without.generate_with_header("test.json");
  assert!(result_without.is_ok());
  let (code_without, stats_without) = result_without.unwrap();

  let spec_with_all: oas3::Spec = oas3::from_json(spec_json).unwrap();
  let mut only = HashSet::new();
  only.insert("list_users".to_string());
  let orchestrator_with = Orchestrator::new(
    spec_with_all,
    Visibility::default(),
    true,
    Some(&only),
    None,
    FieldOptionalityPolicy::standard(),
    false,
    false,
    false,
  );
  let result_with = orchestrator_with.generate_with_header("test.json");
  assert!(result_with.is_ok());
  let (code_with, stats_with) = result_with.unwrap();

  assert_eq!(stats_without.operations_converted, 1);
  assert_eq!(stats_with.operations_converted, 1);

  assert!(code_without.contains("UserList"));
  assert!(code_without.contains("User"));
  assert!(!code_without.contains("AdminResponse"));
  assert!(!code_without.contains("UnreferencedSchema"));

  assert!(code_with.contains("UserList"));
  assert!(code_with.contains("User"));
  assert!(code_with.contains("AdminResponse"));
  assert!(code_with.contains("UnreferencedSchema"));

  assert_eq!(stats_without.orphaned_schemas_count, 2);
  assert_eq!(stats_with.orphaned_schemas_count, 0);
}

#[test]
fn test_content_types_generation() {
  let spec_json = include_str!("../../../fixtures/content_types.json");
  let spec: oas3::Spec = oas3::from_json(spec_json).unwrap();
  let orchestrator = Orchestrator::new(
    spec,
    Visibility::default(),
    false,
    None,
    None,
    FieldOptionalityPolicy::standard(),
    false,
    false,
    false,
  );

  let result = orchestrator.generate_with_header("test.json");
  assert!(result.is_ok());

  let (code, _) = result.unwrap();

  // Check for JSON handling (default assumption usually, but checked via logic)
  // We expect 'json_with_diagnostics' for the 200 response which is application/json
  assert!(code.contains("json_with_diagnostics"));

  // Check for Text handling for 201 text/plain
  assert!(code.contains("req.text().await?"));

  // Check for Binary handling for 202 image/png (fallback to bytes)
  assert!(code.contains("req.bytes().await?"));
}

#[test]
fn test_enum_deduplication() {
  let spec_json = include_str!("../../../fixtures/enum_deduplication.json");
  let spec: oas3::Spec = oas3::from_json(spec_json).unwrap();
  let orchestrator = Orchestrator::new(
    spec,
    Visibility::default(),
    true,
    None,
    None,
    FieldOptionalityPolicy::standard(),
    false,
    false,
    false,
  );

  let (code, _) = orchestrator.generate_with_header("test.json").unwrap();

  // Should contain "pub enum Status" exactly once (definition)
  // But string matching "pub enum Status" might appear multiple times if I'm not careful?
  // No, definition appears once.
  assert_eq!(
    code.matches("pub enum Status").count(),
    1,
    "Status enum defined multiple times"
  );

  // StructA should use Status
  // "pub status: Option<Status>,"
  assert!(code.contains("pub status: Option<Status>"), "StructA should use Status");

  // StructB should use Status
  // Note: count matches for "status: Option<Status>" to ensure multiple uses
  let usage_count = code.matches("status: Option<Status>").count();
  assert!(
    usage_count >= 2,
    "Multiple structs should use Status (found {usage_count})"
  );

  // StructC should also use Status (values are sorted same)
  assert!(usage_count >= 3, "StructC should also use Status");
}

#[test]
fn test_relaxed_enum_deduplication() {
  let spec_json = include_str!("../../../fixtures/relaxed_enum_deduplication.json");
  let spec: oas3::Spec = oas3::from_json(spec_json).unwrap();
  let orchestrator = Orchestrator::new(
    spec,
    Visibility::default(),
    true,
    None,
    None,
    FieldOptionalityPolicy::standard(),
    false,
    false,
    false,
  );

  let (code, _) = orchestrator.generate_with_header("test.json").unwrap();

  // 1. Verify Status is defined
  assert!(code.contains("pub enum Status"), "Status enum missing");

  // 2. Verify ComplexStatusStatus is defined (outer enum)
  assert!(code.contains("pub enum ComplexStatusStatus"), "Outer enum missing");

  // 3. Verify inner enum "ComplexStatusStatusKnown" is NOT defined (deduplicated)
  assert!(
    !code.contains("pub enum ComplexStatusStatusKnown"),
    "Inner enum should be deduplicated"
  );

  if !code.contains("Known(Status)") {
    println!("Generated Code:\n{code}");
  }
  // 4. Verify usage: Known variant should wrap Status
  // "Known(Status)"
  assert!(code.contains("Known(Status)"), "Outer enum should wrap Status");
}

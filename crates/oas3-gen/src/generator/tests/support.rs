use std::collections::{HashMap, HashSet};

use oas3::Spec;

use crate::generator::{
  CodegenConfig, SchemaScope, TypesMode,
  codegen::{GeneratedFileType, Visibility},
  orchestrator::Orchestrator,
};

pub(super) struct GeneratedTypes {
  pub(super) code: String,
  pub(super) operations_converted: usize,
  pub(super) orphaned_schemas_count: usize,
}

pub(super) fn parse_spec(spec_json: &str) -> Spec {
  oas3::from_json(spec_json).expect("failed to parse test spec")
}

pub(super) fn string_set(values: &[&str]) -> HashSet<String> {
  values.iter().map(|value| (*value).to_string()).collect::<HashSet<_>>()
}

pub(super) fn make_orchestrator(spec: Spec, all_schemas: bool) -> Orchestrator {
  Orchestrator::new(
    spec,
    Visibility::default(),
    config_for_schema_scope(all_schemas),
    None,
    None,
  )
}

pub(super) fn make_orchestrator_with_ops(
  spec: Spec,
  all_schemas: bool,
  only: Option<&HashSet<String>>,
  exclude: Option<&HashSet<String>>,
) -> Orchestrator {
  Orchestrator::new(
    spec,
    Visibility::default(),
    config_for_schema_scope(all_schemas),
    only,
    exclude,
  )
}

pub(super) fn make_orchestrator_with_customizations(
  spec: Spec,
  all_schemas: bool,
  customizations: HashMap<String, String>,
) -> Orchestrator {
  let config = CodegenConfig::builder()
    .schema_scope(schema_scope(all_schemas))
    .customizations(customizations)
    .build();
  Orchestrator::new(spec, Visibility::default(), config, None, None)
}

pub(super) fn generate_types(orchestrator: &Orchestrator, source_path: &str) -> GeneratedTypes {
  let output = orchestrator
    .generate(&TypesMode, source_path)
    .expect("types generation should succeed");
  let code = output
    .code
    .code(&GeneratedFileType::Types)
    .expect("types output should exist")
    .clone();
  GeneratedTypes {
    code,
    operations_converted: output.stats.operations_converted,
    orphaned_schemas_count: output.stats.orphaned_schemas_count,
  }
}

pub(super) fn assert_contains(code: &str, expected: &str, context: &str) {
  assert!(code.contains(expected), "missing {context}: expected '{expected}'");
}

pub(super) fn assert_not_contains(code: &str, pattern: &str, context: &str) {
  assert!(!code.contains(pattern), "{context}: '{pattern}' should not appear");
}

pub(super) fn assert_contains_all(code: &str, checks: &[(&str, &str)]) {
  for (expected, context) in checks {
    assert_contains(code, expected, context);
  }
}

pub(super) fn assert_occurs_at_least(code: &str, pattern: &str, expected: usize, context: &str) {
  let actual = code.matches(pattern).count();
  assert!(
    actual >= expected,
    "{context}: expected at least {expected} occurrences of '{pattern}', found {actual}"
  );
}

fn config_for_schema_scope(all_schemas: bool) -> CodegenConfig {
  CodegenConfig {
    schema_scope: schema_scope(all_schemas),
    ..Default::default()
  }
}

fn schema_scope(all_schemas: bool) -> SchemaScope {
  if all_schemas {
    SchemaScope::All
  } else {
    SchemaScope::ReferencedOnly
  }
}

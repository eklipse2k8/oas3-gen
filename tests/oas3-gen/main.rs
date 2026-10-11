macro_rules! spec {
  ($name:literal) => {
    include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/../fixtures/specs/", $name))
  };
}

mod generator;
mod ui;
mod utils;

use std::collections::HashSet;

use oas3::{Spec, spec::ObjectSchema};
use oas3_gen::generator::{
  CodegenConfig, SchemaScope, TypesMode,
  codegen::{GeneratedFileType, Visibility},
  orchestrator::{GeneratedFinalOutput, Orchestrator},
};
use serde_json::Value;

struct GeneratedTypes {
  code: String,
  orphaned_schemas_count: usize,
}

#[track_caller]
fn parse_spec(spec_json: &str) -> Spec {
  oas3::from_json(spec_json).expect("test spec parses")
}

#[track_caller]
fn parse_schema(value: Value) -> ObjectSchema {
  serde_json::from_value(value).expect("schema parses from JSON")
}

fn string_set(values: &[&str]) -> HashSet<String> {
  values.iter().map(ToString::to_string).collect()
}

fn schema_scope(all_schemas: bool) -> SchemaScope {
  if all_schemas {
    SchemaScope::All
  } else {
    SchemaScope::ReferencedOnly
  }
}

fn make_orchestrator_with_config(spec: Spec, config: CodegenConfig) -> Orchestrator {
  Orchestrator::new(spec, Visibility::default(), config, None, None)
}

fn make_orchestrator(spec: Spec, all_schemas: bool) -> Orchestrator {
  make_orchestrator_with_ops(spec, all_schemas, None, None)
}

fn make_orchestrator_with_ops(
  spec: Spec,
  all_schemas: bool,
  only: Option<&HashSet<String>>,
  exclude: Option<&HashSet<String>>,
) -> Orchestrator {
  let config = CodegenConfig {
    schema_scope: schema_scope(all_schemas),
    ..CodegenConfig::default()
  };
  Orchestrator::new(spec, Visibility::default(), config, only, exclude)
}

#[track_caller]
fn generated_code(output: &GeneratedFinalOutput, file_type: &GeneratedFileType) -> String {
  let Some(code) = output.code.code(file_type) else {
    panic!("{file_type:?} output should exist");
  };
  code.clone()
}

#[track_caller]
fn generate_types(orchestrator: &Orchestrator, source_path: &str) -> GeneratedTypes {
  let output = orchestrator
    .generate(&TypesMode, source_path)
    .expect("types generation should succeed");
  GeneratedTypes {
    code: generated_code(&output, &GeneratedFileType::Types),
    orphaned_schemas_count: output.stats.orphaned_schemas_count,
  }
}

#[track_caller]
fn field_attributes<'a>(code: &'a str, name: &str) -> &'a str {
  let Some(end) = code.find(&format!("pub {name}:")) else {
    panic!("missing field {name}");
  };
  let start = code[..end].rfind(",\n").map_or(0, |start| start + 2);
  &code[start..end]
}

#[track_caller]
fn assert_contains(code: &str, expected: &str, context: &str) {
  assert!(code.contains(expected), "missing {context}: expected '{expected}'");
}

#[track_caller]
fn assert_not_contains(code: &str, pattern: &str, context: &str) {
  assert!(!code.contains(pattern), "{context}: '{pattern}' should not appear");
}

#[track_caller]
fn assert_contains_all(code: &str, checks: &[(&str, &str)]) {
  for (expected, context) in checks {
    assert_contains(code, expected, context);
  }
}

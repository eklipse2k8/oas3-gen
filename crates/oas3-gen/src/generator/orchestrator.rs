use std::{collections::HashSet, fmt};

use crate::generator::{
  analyzer::{self, ErrorAnalyzer},
  ast::{OperationInfo, RustType},
  codegen::{self, Visibility},
  converter::{SchemaConverter, TypeUsageRecorder, operations::OperationConverter},
  operation_registry::OperationRegistry,
  schema_graph::SchemaGraph,
};

const OAS3_GEN_VERSION: &str = env!("CARGO_PKG_VERSION");
const CLIPPY_ALLOWS: &[&str] = &[
  "clippy::doc_markdown",
  "clippy::large_enum_variant",
  "clippy::missing_panics_doc",
  "clippy::result_large_err",
];

#[derive(Debug)]
pub struct Orchestrator {
  spec: oas3::Spec,
  visibility: Visibility,
  include_unused_schemas: bool,
  operation_registry: OperationRegistry,
}

#[derive(Debug, Clone)]
pub struct CodeMetadata {
  pub title: String,
  pub version: String,
  pub description: Option<String>,
}

#[derive(Debug)]
pub struct GenerationStats {
  pub types_generated: usize,
  pub structs_generated: usize,
  pub enums_generated: usize,
  pub type_aliases_generated: usize,
  pub operations_converted: usize,
  pub cycles_detected: usize,
  pub cycle_details: Vec<Vec<String>>,
  pub warnings: Vec<GenerationWarning>,
  pub orphaned_schemas_count: usize,
}

struct GenerationArtifacts {
  graph: SchemaGraph,
  rust_types: Vec<RustType>,
  operations_info: Vec<OperationInfo>,
  usage_recorder: TypeUsageRecorder,
  stats: GenerationStats,
}

#[derive(Debug)]
pub enum GenerationWarning {
  SchemaConversionFailed {
    schema_name: String,
    error: String,
  },
  OperationConversionFailed {
    method: String,
    path: String,
    error: String,
  },
  OperationSpecific {
    operation_id: String,
    message: String,
  },
}

impl fmt::Display for GenerationWarning {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    match self {
      Self::SchemaConversionFailed { schema_name, error } => {
        write!(f, "Failed to convert schema '{schema_name}': {error}")
      }
      Self::OperationConversionFailed { method, path, error } => {
        write!(f, "Failed to convert operation '{method} {path}': {error}")
      }
      Self::OperationSpecific { operation_id, message } => write!(f, "[{operation_id}] {message}"),
    }
  }
}

impl Orchestrator {
  #[must_use]
  pub fn new(
    spec: oas3::Spec,
    visibility: Visibility,
    include_unused_schemas: bool,
    only_operations: Option<&HashSet<String>>,
    excluded_operations: Option<&HashSet<String>>,
  ) -> Self {
    let operation_registry = OperationRegistry::from_spec_filtered(&spec, only_operations, excluded_operations);
    Self {
      spec,
      visibility,
      include_unused_schemas,
      operation_registry,
    }
  }

  pub fn generate_client_with_header(&self, source_path: &str) -> anyhow::Result<(String, GenerationStats)> {
    let artifacts = self.collect_generation_artifacts()?;
    let GenerationArtifacts {
      operations_info, stats, ..
    } = artifacts;

    let client_tokens = codegen::client::generate_client(&self.spec, &operations_info)?;
    let formatted_code = Self::format_code(&client_tokens)?;
    let header = self.generate_header(source_path);
    let final_code = format!("{header}\n\n{formatted_code}\n\nfn main() {{}}\n");
    Ok((final_code, stats))
  }

  pub fn metadata(&self) -> CodeMetadata {
    CodeMetadata {
      title: self.spec.info.title.clone(),
      version: self.spec.info.version.clone(),
      description: self.spec.info.description.clone(),
    }
  }

  pub fn generate_with_header(&self, source_path: &str) -> anyhow::Result<(String, GenerationStats)> {
    let artifacts = self.collect_generation_artifacts()?;
    let GenerationArtifacts {
      graph,
      rust_types,
      operations_info,
      usage_recorder,
      stats,
    } = artifacts;

    let code_tokens = self.generate_code_from_artifacts(&graph, rust_types, &operations_info, usage_recorder);
    let formatted_code = Self::format_code(&code_tokens)?;
    let header = self.generate_header(source_path);
    let final_code = format!("{header}\n\n{formatted_code}\n\nfn main() {{}}\n");
    Ok((final_code, stats))
  }

  fn collect_generation_artifacts(&self) -> anyhow::Result<GenerationArtifacts> {
    let mut graph = SchemaGraph::new(self.spec.clone())?;
    graph.build_dependencies();
    let cycle_details = graph.detect_cycles();

    let operation_reachable = if self.include_unused_schemas {
      None
    } else {
      Some(graph.get_operation_reachable_schemas(&self.operation_registry))
    };

    let total_schemas = graph.schema_names().len();
    let orphaned_schemas_count = if let Some(ref reachable) = operation_reachable {
      total_schemas.saturating_sub(reachable.len())
    } else {
      0
    };

    let schema_converter = if let Some(ref reachable) = operation_reachable {
      SchemaConverter::new_with_filter(&graph, reachable.clone())
    } else {
      SchemaConverter::new(&graph)
    };
    let (schema_rust_types, schema_warnings) =
      Self::convert_all_schemas(&graph, &schema_converter, operation_reachable.as_ref());

    let (op_rust_types, operations_info, op_warnings, usage_recorder) =
      self.convert_all_operations(&graph, &schema_converter);

    let mut rust_types = schema_rust_types;
    rust_types.extend(op_rust_types);
    let mut warnings = schema_warnings;
    warnings.extend(op_warnings);

    let mut structs_generated = 0;
    let mut enums_generated = 0;
    let mut type_aliases_generated = 0;

    for rust_type in &rust_types {
      match rust_type {
        RustType::Struct(_) => structs_generated += 1,
        RustType::Enum(_) | RustType::DiscriminatedEnum(_) | RustType::ResponseEnum(_) => enums_generated += 1,
        RustType::TypeAlias(_) => type_aliases_generated += 1,
      }
    }

    let types_generated = structs_generated + enums_generated + type_aliases_generated;

    let stats = GenerationStats {
      types_generated,
      structs_generated,
      enums_generated,
      type_aliases_generated,
      operations_converted: operations_info.len(),
      cycles_detected: cycle_details.len(),
      cycle_details,
      warnings,
      orphaned_schemas_count,
    };

    Ok(GenerationArtifacts {
      graph,
      rust_types,
      operations_info,
      usage_recorder,
      stats,
    })
  }

  fn convert_all_schemas(
    graph: &SchemaGraph,
    schema_converter: &SchemaConverter,
    operation_reachable: Option<&std::collections::BTreeSet<String>>,
  ) -> (Vec<RustType>, Vec<GenerationWarning>) {
    let mut rust_types = Vec::new();
    let mut warnings = Vec::new();

    for schema_name in graph.schema_names() {
      if let Some(filter) = operation_reachable
        && !filter.contains(schema_name.as_str())
      {
        continue;
      }

      if let Some(schema) = graph.get_schema(schema_name) {
        match schema_converter.convert_schema(schema_name, schema) {
          Ok(types) => rust_types.extend(types),
          Err(e) => warnings.push(GenerationWarning::SchemaConversionFailed {
            schema_name: schema_name.clone(),
            error: e.to_string(),
          }),
        }
      }
    }
    (rust_types, warnings)
  }

  fn convert_all_operations(
    &self,
    graph: &SchemaGraph,
    schema_converter: &SchemaConverter,
  ) -> (
    Vec<RustType>,
    Vec<OperationInfo>,
    Vec<GenerationWarning>,
    TypeUsageRecorder,
  ) {
    let mut rust_types = Vec::new();
    let mut operations_info = Vec::new();
    let mut warnings = Vec::new();
    let mut usage_recorder = TypeUsageRecorder::new();

    let operation_converter = OperationConverter::new(schema_converter, graph.spec());

    for (stable_id, method, path, operation) in self.operation_registry.operations_with_details() {
      let operation_id = operation.operation_id.as_deref().unwrap_or("unknown");

      match operation_converter.convert(stable_id, operation_id, method, path, operation, &mut usage_recorder) {
        Ok((types, op_info)) => {
          warnings.extend(op_info.warnings.iter().map(|w| GenerationWarning::OperationSpecific {
            operation_id: op_info.operation_id.clone(),
            message: w.clone(),
          }));
          rust_types.extend(types);
          operations_info.push(op_info);
        }
        Err(e) => {
          warnings.push(GenerationWarning::OperationConversionFailed {
            method: method.to_string(),
            path: path.to_string(),
            error: e.to_string(),
          });
        }
      }
    }

    rust_types.extend(operation_converter.finish());
    (rust_types, operations_info, warnings, usage_recorder)
  }

  fn generate_code_from_artifacts(
    &self,
    graph: &SchemaGraph,
    mut rust_types: Vec<RustType>,
    operations_info: &[OperationInfo],
    usage_recorder: TypeUsageRecorder,
  ) -> proc_macro2::TokenStream {
    let seed_map = usage_recorder.into_usage_map();
    let type_usage = analyzer::build_type_usage_map(seed_map, &rust_types);
    analyzer::update_derives_from_usage(&mut rust_types, &type_usage);
    let error_schemas = ErrorAnalyzer::build_error_schema_set(operations_info, &rust_types);

    codegen::generate(&rust_types, &graph.all_headers(), &error_schemas, self.visibility)
  }

  fn format_code(code: &proc_macro2::TokenStream) -> anyhow::Result<String> {
    let syntax_tree = syn::parse2(code.clone())?;
    Ok(prettyplease::unparse(&syntax_tree))
  }

  fn generate_header(&self, source_path: &str) -> String {
    let metadata = self.metadata();
    let description = metadata.description.as_ref().map_or_else(
      || String::from("No description provided"),
      |d| d.replace('\n', "\n//! "),
    );

    let clippy_directives = CLIPPY_ALLOWS
      .iter()
      .map(|allow| format!("#![allow({allow})]"))
      .collect::<Vec<_>>()
      .join("\n");

    format!(
      r"{clippy_directives}
//!
//! AUTO-GENERATED CODE - DO NOT EDIT!
//!
//! {}
//! Source: {}
//! Version: {}
//! Generated by `oas3-gen v{}`
//!
//! {}",
      metadata.title, source_path, metadata.version, OAS3_GEN_VERSION, description
    )
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  fn create_empty_spec(title: &str, version: &str, description: Option<&str>) -> oas3::Spec {
    let spec_json = format!(
      r#"{{
        "openapi": "3.1.0",
        "info": {{
          "title": "{}",
          "version": "{}",
          "description": "{}"
        }},
        "paths": {{}}
      }}"#,
      title,
      version,
      description.unwrap_or("")
    );
    oas3::from_json(&spec_json).unwrap()
  }

  #[test]
  fn test_orchestrator_new_and_metadata() {
    let spec = create_empty_spec("Empty API", "1.0.0", Some("An empty spec."));
    let orchestrator = Orchestrator::new(spec, Visibility::default(), false, None, None);

    let metadata = orchestrator.metadata();
    assert_eq!(metadata.title, "Empty API");
    assert_eq!(metadata.version, "1.0.0");
    assert_eq!(metadata.description.as_deref(), Some("An empty spec."));
  }

  #[test]
  fn test_orchestrator_generate_with_header() {
    let spec = create_empty_spec("Test API", "2.0.0", Some("A test API."));
    let orchestrator = Orchestrator::new(spec, Visibility::default(), false, None, None);

    let result = orchestrator.generate_with_header("/path/to/spec.json");
    assert!(result.is_ok());

    let (code, _) = result.unwrap();
    assert!(code.contains("AUTO-GENERATED CODE - DO NOT EDIT!"));
    assert!(code.contains("//! Test API"));
    assert!(code.contains("//! Source: /path/to/spec.json"));
    assert!(code.contains("//! Version: 2.0.0"));
    assert!(code.contains("//! A test API."));
    assert!(code.contains("fn main()"));
    assert!(code.contains("#![allow(clippy::doc_markdown)]"));
  }

  #[test]
  fn test_header_generation_with_multiline_description() {
    let spec_json = r#"{
      "openapi": "3.1.0",
      "info": {
        "title": "Test API",
        "version": "1.0.0",
        "description": "Multi\nline\ndescription."
      },
      "paths": {}
    }"#;
    let spec: oas3::Spec = oas3::from_json(spec_json).unwrap();
    let orchestrator = Orchestrator::new(spec, Visibility::default(), false, None, None);

    let header = orchestrator.generate_header("test.yaml");
    assert!(header.contains("Multi\n//! line\n//! description."));
  }

  #[test]
  fn test_operation_exclusion() {
    let spec_json = r###"{
      "openapi": "3.1.0",
      "info": {
        "title": "Test API",
        "version": "1.0.0"
      },
      "paths": {
        "/users": {
          "get": {
            "operationId": "listUsers",
            "responses": {
              "200": {
                "description": "Success",
                "content": {
                  "application/json": {
                    "schema": {
                      "$ref": "#/components/schemas/UserList"
                    }
                  }
                }
              }
            }
          },
          "post": {
            "operationId": "createUser",
            "responses": {
              "201": {
                "description": "Created",
                "content": {
                  "application/json": {
                    "schema": {
                      "$ref": "#/components/schemas/User"
                    }
                  }
                }
              }
            }
          }
        },
        "/posts": {
          "get": {
            "operationId": "listPosts",
            "responses": {
              "200": {
                "description": "Success",
                "content": {
                  "application/json": {
                    "schema": {
                      "$ref": "#/components/schemas/PostList"
                    }
                  }
                }
              }
            }
          }
        }
      },
      "components": {
        "schemas": {
          "User": {
            "type": "object",
            "properties": {
              "id": { "type": "string" },
              "name": { "type": "string" }
            }
          },
          "UserList": {
            "type": "object",
            "properties": {
              "users": {
                "type": "array",
                "items": { "$ref": "#/components/schemas/User" }
              }
            }
          },
          "Post": {
            "type": "object",
            "properties": {
              "id": { "type": "string" },
              "title": { "type": "string" }
            }
          },
          "PostList": {
            "type": "object",
            "properties": {
              "posts": {
                "type": "array",
                "items": { "$ref": "#/components/schemas/Post" }
              }
            }
          }
        }
      }
    }"###;

    let spec: oas3::Spec = oas3::from_json(spec_json).unwrap();
    let mut excluded = HashSet::new();
    excluded.insert("create_user".to_string());

    let orchestrator = Orchestrator::new(spec, Visibility::default(), false, None, Some(&excluded));
    let result = orchestrator.generate_with_header("test.json");
    assert!(result.is_ok());

    let (code, stats) = result.unwrap();
    assert_eq!(stats.operations_converted, 2);
    assert!(!code.contains("create_user"));
  }

  #[test]
  fn test_operation_exclusion_affects_schema_reachability() {
    let spec_json = r###"{
      "openapi": "3.1.0",
      "info": {
        "title": "Test API",
        "version": "1.0.0"
      },
      "paths": {
        "/users": {
          "get": {
            "operationId": "listUsers",
            "responses": {
              "200": {
                "description": "Success",
                "content": {
                  "application/json": {
                    "schema": {
                      "$ref": "#/components/schemas/UserList"
                    }
                  }
                }
              }
            }
          }
        },
        "/admin": {
          "post": {
            "operationId": "adminAction",
            "responses": {
              "200": {
                "description": "Success",
                "content": {
                  "application/json": {
                    "schema": {
                      "$ref": "#/components/schemas/AdminResponse"
                    }
                  }
                }
              }
            }
          }
        }
      },
      "components": {
        "schemas": {
          "User": {
            "type": "object",
            "properties": {
              "id": { "type": "string" }
            }
          },
          "UserList": {
            "type": "object",
            "properties": {
              "users": {
                "type": "array",
                "items": { "$ref": "#/components/schemas/User" }
              }
            }
          },
          "AdminResponse": {
            "type": "object",
            "properties": {
              "status": { "type": "string" }
            }
          }
        }
      }
    }"###;

    let spec_full: oas3::Spec = oas3::from_json(spec_json).unwrap();
    let orchestrator_full = Orchestrator::new(spec_full, Visibility::default(), false, None, None);
    let result_full = orchestrator_full.generate_with_header("test.json");
    assert!(result_full.is_ok());
    let (code_full, stats_full) = result_full.unwrap();

    let spec_filtered: oas3::Spec = oas3::from_json(spec_json).unwrap();
    let mut excluded = HashSet::new();
    excluded.insert("admin_action".to_string());
    let orchestrator_filtered = Orchestrator::new(spec_filtered, Visibility::default(), false, None, Some(&excluded));
    let result_filtered = orchestrator_filtered.generate_with_header("test.json");
    assert!(result_filtered.is_ok());
    let (code_filtered, stats_filtered) = result_filtered.unwrap();

    assert_eq!(stats_full.operations_converted, 2);
    assert_eq!(stats_filtered.operations_converted, 1);

    assert!(code_full.contains("AdminResponse"));
    assert!(!code_filtered.contains("AdminResponse"));
    assert!(code_filtered.contains("UserList"));
    assert!(code_filtered.contains("User"));
  }

  #[test]
  fn test_all_schemas_overrides_operation_filtering() {
    let spec_json = r###"{
      "openapi": "3.1.0",
      "info": {
        "title": "Test API",
        "version": "1.0.0"
      },
      "paths": {
        "/users": {
          "get": {
            "operationId": "listUsers",
            "responses": {
              "200": {
                "description": "Success",
                "content": {
                  "application/json": {
                    "schema": {
                      "$ref": "#/components/schemas/UserList"
                    }
                  }
                }
              }
            }
          }
        }
      },
      "components": {
        "schemas": {
          "User": {
            "type": "object",
            "properties": {
              "id": { "type": "string" }
            }
          },
          "UserList": {
            "type": "object",
            "properties": {
              "users": {
                "type": "array",
                "items": { "$ref": "#/components/schemas/User" }
              }
            }
          },
          "AdminResponse": {
            "type": "object",
            "properties": {
              "status": { "type": "string" }
            }
          },
          "UnreferencedSchema": {
            "type": "object",
            "properties": {
              "data": { "type": "string" }
            }
          }
        }
      }
    }"###;

    let spec_without_all: oas3::Spec = oas3::from_json(spec_json).unwrap();
    let mut only = HashSet::new();
    only.insert("list_users".to_string());
    let orchestrator_without = Orchestrator::new(spec_without_all, Visibility::default(), false, Some(&only), None);
    let result_without = orchestrator_without.generate_with_header("test.json");
    assert!(result_without.is_ok());
    let (code_without, stats_without) = result_without.unwrap();

    let spec_with_all: oas3::Spec = oas3::from_json(spec_json).unwrap();
    let mut only = HashSet::new();
    only.insert("list_users".to_string());
    let orchestrator_with = Orchestrator::new(spec_with_all, Visibility::default(), true, Some(&only), None);
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
}

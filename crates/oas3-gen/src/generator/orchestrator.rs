use std::fmt;

use crate::generator::{
  analyzer::ErrorAnalyzer,
  ast::{OperationInfo, RustType},
  codegen::{self, Visibility},
  converter::{SchemaConverter, operations::OperationConverter},
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
  pub const fn new(spec: oas3::Spec, visibility: Visibility) -> Self {
    Self { spec, visibility }
  }

  pub fn metadata(&self) -> CodeMetadata {
    CodeMetadata {
      title: self.spec.info.title.clone(),
      version: self.spec.info.version.clone(),
      description: self.spec.info.description.clone(),
    }
  }

  pub fn generate_with_header(&self, source_path: &str) -> anyhow::Result<(String, GenerationStats)> {
    let (code_tokens, stats) = self.run_generation_pipeline()?;
    let formatted_code = Self::format_code(&code_tokens)?;
    let header = self.generate_header(source_path);
    let final_code = format!("{header}\n\n{formatted_code}\n\nfn main() {{}}\n");
    Ok((final_code, stats))
  }

  fn run_generation_pipeline(&self) -> anyhow::Result<(proc_macro2::TokenStream, GenerationStats)> {
    let mut graph = SchemaGraph::new(self.spec.clone())?;
    graph.build_dependencies();
    let cycle_details = graph.detect_cycles();

    let schema_converter = SchemaConverter::new(&graph);
    let (schema_rust_types, schema_warnings) = Self::convert_all_schemas(&graph, &schema_converter);

    let (op_rust_types, operations_info, op_warnings) = Self::convert_all_operations(&graph, &schema_converter);

    let mut rust_types = schema_rust_types;
    rust_types.extend(op_rust_types);
    let mut warnings = schema_warnings;
    warnings.extend(op_warnings);

    let code = self.generate_code_from_artifacts(&graph, &rust_types, &operations_info);

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

    let stats = GenerationStats {
      types_generated: rust_types.len(),
      structs_generated,
      enums_generated,
      type_aliases_generated,
      operations_converted: operations_info.len(),
      cycles_detected: cycle_details.len(),
      cycle_details,
      warnings,
    };

    Ok((code, stats))
  }

  fn convert_all_schemas(
    graph: &SchemaGraph,
    schema_converter: &SchemaConverter,
  ) -> (Vec<RustType>, Vec<GenerationWarning>) {
    let mut rust_types = Vec::new();
    let mut warnings = Vec::new();

    for schema_name in graph.schema_names() {
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
    graph: &SchemaGraph,
    schema_converter: &SchemaConverter,
  ) -> (Vec<RustType>, Vec<OperationInfo>, Vec<GenerationWarning>) {
    let mut rust_types = Vec::new();
    let mut operations_info = Vec::new();
    let mut warnings = Vec::new();

    let operation_converter = OperationConverter::new(schema_converter, graph.spec());

    if let Some(ref paths) = graph.spec().paths {
      for (path, path_item) in paths {
        for (method, operation) in path_item.methods() {
          let method_str = method.as_str();
          let operation_id = operation.operation_id.as_deref().unwrap_or("unknown");

          match operation_converter.convert(operation_id, method_str, path, operation) {
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
                method: method_str.to_string(),
                path: path.clone(),
                error: e.to_string(),
              });
            }
          }
        }
      }
    }

    rust_types.extend(operation_converter.finish());
    (rust_types, operations_info, warnings)
  }

  fn generate_code_from_artifacts(
    &self,
    graph: &SchemaGraph,
    rust_types: &[RustType],
    operations_info: &[OperationInfo],
  ) -> proc_macro2::TokenStream {
    let type_usage = codegen::build_type_usage_map(operations_info);
    let error_schemas = ErrorAnalyzer::build_error_schema_set(operations_info, rust_types);

    codegen::generate(
      rust_types,
      &type_usage,
      &graph.all_headers(),
      &error_schemas,
      self.visibility,
    )
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
    let orchestrator = Orchestrator::new(spec, Visibility::default());

    let metadata = orchestrator.metadata();
    assert_eq!(metadata.title, "Empty API");
    assert_eq!(metadata.version, "1.0.0");
    assert_eq!(metadata.description.as_deref(), Some("An empty spec."));
  }

  #[test]
  fn test_orchestrator_generate_with_header() {
    let spec = create_empty_spec("Test API", "2.0.0", Some("A test API."));
    let orchestrator = Orchestrator::new(spec, Visibility::default());

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
    let orchestrator = Orchestrator::new(spec, Visibility::default());

    let header = orchestrator.generate_header("test.yaml");
    assert!(header.contains("Multi\n//! line\n//! description."));
  }
}

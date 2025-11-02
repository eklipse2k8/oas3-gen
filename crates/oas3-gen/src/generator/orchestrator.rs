use crate::generator::{
  code_generator::{CodeGenerator, Visibility},
  operation_converter::OperationConverter,
  schema_converter::SchemaConverter,
  schema_graph::SchemaGraph,
};

const OAS3_GEN_VERSION: &str = env!("CARGO_PKG_VERSION");

/// High-level orchestrator for OpenAPI to Rust code generation.
///
/// This struct encapsulates the entire generation pipeline and provides
/// simple methods for generating Rust code from OpenAPI specifications.
#[derive(Debug)]
pub struct Orchestrator {
  spec: oas3::Spec,
  visibility: Visibility,
}

/// Metadata about the OpenAPI specification for code header generation.
#[derive(Debug, Clone)]
pub struct CodeMetadata {
  /// API title from OpenAPI info object
  pub title: String,
  /// API version from OpenAPI info object
  pub version: String,
  /// Optional API description from OpenAPI info object
  pub description: Option<String>,
}

/// Statistics about the code generation process.
#[derive(Debug)]
pub struct GenerationStats {
  /// Total number of Rust types generated
  pub types_generated: usize,
  /// Number of API operations converted
  pub operations_converted: usize,
  /// Number of circular dependency cycles detected
  pub cycles_detected: usize,
  /// Detailed information about detected cycles (for verbose logging)
  pub cycle_details: Vec<Vec<String>>,
  /// Non-fatal warnings from the conversion process
  pub warnings: Vec<String>,
}

impl Orchestrator {
  /// Creates a new orchestrator from an OpenAPI specification.
  ///
  /// This constructor validates the spec and prepares it for code generation.
  /// The actual generation is performed when calling `generate()` or
  /// `generate_with_header()`.
  ///
  /// # Arguments
  ///
  /// * `spec` - The OpenAPI specification
  /// * `visibility` - Visibility level for generated types (public, crate, or file-private)
  #[must_use]
  pub const fn new(spec: oas3::Spec, visibility: Visibility) -> Self {
    Self { spec, visibility }
  }

  /// Extracts metadata from the OpenAPI specification.
  ///
  /// This metadata is used for generating file headers and documentation.
  pub fn metadata(&self) -> CodeMetadata {
    CodeMetadata {
      title: self.spec.info.title.clone(),
      version: self.spec.info.version.clone(),
      description: self.spec.info.description.clone(),
    }
  }

  /// Generates Rust code from the OpenAPI specification.
  ///
  /// This method orchestrates the complete pipeline:
  /// 1. Builds schema dependency graph
  /// 2. Detects circular dependencies
  /// 3. Converts schemas to Rust types
  /// 4. Converts operations to request/response types
  /// 5. Generates and formats the code
  ///
  /// The returned code does NOT include a file header. Use `generate_with_header()`
  /// if you want the auto-generated file header.
  ///
  /// # Returns
  ///
  /// A tuple of `(String, GenerationStats)` where:
  /// - First element: Formatted Rust code
  /// - Second element: Statistics about the generation process
  ///
  /// # Errors
  ///
  /// Returns an error if:
  /// - Schema graph cannot be built
  /// - Code generation produces invalid Rust syntax
  /// - Code formatter fails
  pub async fn generate(&self) -> anyhow::Result<(String, GenerationStats)> {
    let mut graph = SchemaGraph::new(self.spec.clone())?;
    graph.build_dependencies();
    let cycle_details = graph.detect_cycles();

    let schema_converter = SchemaConverter::new(&graph);
    let mut rust_types = Vec::new();
    let mut warnings = Vec::new();

    for schema_name in graph.schema_names() {
      if let Some(schema) = graph.get_schema(schema_name) {
        match schema_converter.convert_schema(schema_name, schema).await {
          Ok(types) => rust_types.extend(types),
          Err(e) => warnings.push(format!("Failed to convert schema {schema_name}: {e}")),
        }
      }
    }

    let operation_converter = OperationConverter::new(&schema_converter, graph.spec());
    let mut operations_info = Vec::new();

    if let Some(ref paths) = graph.spec().paths {
      for (path, path_item) in paths {
        for (method, operation) in path_item.methods() {
          let method_str = method.as_str();
          let operation_id = operation.operation_id.as_deref().unwrap_or("unknown");

          match operation_converter
            .convert_operation(operation_id, method_str, path, operation)
            .await
          {
            Ok((types, op_info)) => {
              rust_types.extend(types);
              operations_info.push(op_info);
            }
            Err(e) => {
              warnings.push(format!("Failed to convert operation {method_str} {path}: {e}"));
            }
          }
        }
      }
    }

    let type_usage = CodeGenerator::build_type_usage_map(&operations_info);
    let code = CodeGenerator::generate(&rust_types, &type_usage, &graph.all_headers(), self.visibility);
    let syntax_tree = syn::parse2(code)?;
    let formatted = prettyplease::unparse(&syntax_tree);

    let stats = GenerationStats {
      types_generated: rust_types.len(),
      operations_converted: operations_info.len(),
      cycles_detected: cycle_details.len(),
      cycle_details,
      warnings,
    };

    Ok((formatted, stats))
  }

  /// Generates Rust code with an auto-generated file header.
  ///
  /// This method calls `generate()` and then adds a file header containing:
  /// - Auto-generation warning
  /// - API title, version, and description
  /// - Source file path
  /// - Clippy allow attributes
  /// - Empty main function (for standalone compilation)
  ///
  /// # Arguments
  ///
  /// * `source_path` - Path to the input OpenAPI file (for documentation)
  ///
  /// # Returns
  ///
  /// A tuple of `(String, GenerationStats)` where:
  /// - First element: Complete Rust code with header
  /// - Second element: Statistics about the generation process
  ///
  /// # Errors
  ///
  /// Returns the same errors as `generate()`.
  pub async fn generate_with_header(&self, source_path: &str) -> anyhow::Result<(String, GenerationStats)> {
    let (code, stats) = self.generate().await?;
    let metadata = self.metadata();

    let description = metadata.description.as_ref().map_or_else(
      || String::from("No description provided"),
      |d| d.replace('\n', "\n//! "),
    );

    let final_code = format!(
      r"//! AUTO-GENERATED CODE - DO NOT EDIT!
//!
//! {}
//! Source: {}
//! Version: {}
//! Generated by `oas3-gen v{}`
//!
//! {}
#![allow(clippy::large_enum_variant)]

{}

fn main() {{}}
",
      metadata.title, source_path, metadata.version, OAS3_GEN_VERSION, description, code
    );

    Ok((final_code, stats))
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn test_orchestrator_empty_spec() {
    let spec_json = r#"{
      "openapi": "3.1.0",
      "info": {
        "title": "Empty API",
        "version": "1.0.0"
      },
      "paths": {}
    }"#;
    let spec: oas3::Spec = oas3::from_json(spec_json).unwrap();
    let orchestrator = Orchestrator::new(spec, Visibility::default());

    let metadata = orchestrator.metadata();
    assert_eq!(metadata.title, "Empty API");
    assert_eq!(metadata.version, "1.0.0");
  }

  #[tokio::test]
  async fn test_orchestrator_generate_empty() {
    let spec_json = r#"{
      "openapi": "3.1.0",
      "info": {
        "title": "Empty API",
        "version": "1.0.0"
      },
      "paths": {}
    }"#;
    let spec: oas3::Spec = oas3::from_json(spec_json).unwrap();
    let orchestrator = Orchestrator::new(spec, Visibility::default());

    let result = orchestrator.generate().await;
    assert!(result.is_ok());

    let (code, stats) = result.unwrap();
    assert!(!code.is_empty());
    assert_eq!(stats.types_generated, 0);
    assert_eq!(stats.operations_converted, 0);
    assert_eq!(stats.cycles_detected, 0);
    assert_eq!(stats.warnings.len(), 0);
  }

  #[tokio::test]
  async fn test_orchestrator_generate_with_header() {
    let spec_json = r#"{
      "openapi": "3.1.0",
      "info": {
        "title": "Test API",
        "version": "2.0.0",
        "description": "A test API"
      },
      "paths": {}
    }"#;
    let spec: oas3::Spec = oas3::from_json(spec_json).unwrap();
    let orchestrator = Orchestrator::new(spec, Visibility::default());

    let result = orchestrator.generate_with_header("/path/to/spec.json").await;
    assert!(result.is_ok());

    let (code, _) = result.unwrap();
    assert!(code.contains("//! Test API"));
    assert!(code.contains("//! Source: /path/to/spec.json"));
    assert!(code.contains("//! Version: 2.0.0"));
    assert!(code.contains("//! A test API"));
    assert!(code.contains("fn main()"));
  }

  #[test]
  fn test_code_metadata() {
    let spec_json = r#"{
      "openapi": "3.1.0",
      "info": {
        "title": "Test API",
        "version": "1.0.0",
        "description": "Multi\nline\ndescription"
      },
      "paths": {}
    }"#;
    let spec: oas3::Spec = oas3::from_json(spec_json).unwrap();
    let orchestrator = Orchestrator::new(spec, Visibility::default());
    let metadata = orchestrator.metadata();

    assert_eq!(metadata.title, "Test API");
    assert_eq!(metadata.version, "1.0.0");
    assert_eq!(metadata.description, Some("Multi\nline\ndescription".to_string()));
  }
}

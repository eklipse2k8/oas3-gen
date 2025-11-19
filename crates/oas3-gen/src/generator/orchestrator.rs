use std::{
  collections::{BTreeMap, BTreeSet, HashSet},
  fmt,
};

use super::converter::cache::SharedSchemaCache;
use crate::generator::{
  analyzer::{self, ErrorAnalyzer},
  ast::{OperationInfo, ParameterLocation, RustType, StructMethodKind, TypeRef},
  codegen::{self, Visibility},
  converter::{FieldOptionalityPolicy, SchemaConverter, TypeUsageRecorder, operations::OperationConverter},
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
  optionality_policy: FieldOptionalityPolicy,
  preserve_case_variants: bool,
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
  pub client_methods_generated: Option<usize>,
  pub client_headers_generated: Option<usize>,
}

struct GenerationArtifacts {
  rust_types: Vec<RustType>,
  operations_info: Vec<OperationInfo>,
  usage_recorder: TypeUsageRecorder,
  stats: GenerationStats,
}

type ResponseEnumSignature = Vec<(String, String, String, Option<String>)>;

struct DuplicateCandidate {
  index: usize,
  name: String,
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
    optionality_policy: FieldOptionalityPolicy,
    preserve_case_variants: bool,
  ) -> Self {
    let operation_registry = OperationRegistry::from_spec_filtered(&spec, only_operations, excluded_operations);
    Self {
      spec,
      visibility,
      include_unused_schemas,
      operation_registry,
      optionality_policy,
      preserve_case_variants,
    }
  }

  pub fn generate_client_with_header(&self, source_path: &str) -> anyhow::Result<(String, GenerationStats)> {
    let artifacts = self.collect_generation_artifacts();
    let GenerationArtifacts {
      mut rust_types,
      mut operations_info,
      mut stats,
      ..
    } = artifacts;

    Self::deduplicate_response_enums(&mut rust_types, &mut operations_info);

    let header_count = Self::count_unique_headers(&operations_info);
    stats.client_methods_generated = Some(operations_info.len());
    stats.client_headers_generated = Some(header_count);

    let client_tokens = codegen::client::generate_client(&self.spec, &operations_info)?;
    let formatted_code = Self::format_code(&client_tokens)?;
    let header = self.generate_header(source_path);
    let final_code = format!("{header}\n\n{formatted_code}\n");
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
    let artifacts = self.collect_generation_artifacts();
    let GenerationArtifacts {
      rust_types,
      mut operations_info,
      usage_recorder,
      stats,
    } = artifacts;

    let code_tokens = self.generate_code_from_artifacts(rust_types, &mut operations_info, usage_recorder);
    let formatted_code = Self::format_code(&code_tokens)?;
    let header = self.generate_header(source_path);
    let final_code = format!("{header}\n\n{formatted_code}\n");
    Ok((final_code, stats))
  }

  fn collect_generation_artifacts(&self) -> GenerationArtifacts {
    let (mut graph, mut warnings) = SchemaGraph::new(self.spec.clone());
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
      SchemaConverter::new_with_filter(
        &graph,
        reachable.clone(),
        self.optionality_policy.clone(),
        self.preserve_case_variants,
      )
    } else {
      SchemaConverter::new(&graph, self.optionality_policy.clone(), self.preserve_case_variants)
    };
    let (schema_rust_types, schema_warnings) =
      Self::convert_all_schemas(&graph, &schema_converter, operation_reachable.as_ref());

    let (op_rust_types, operations_info, op_warnings, usage_recorder) =
      self.convert_all_operations(&graph, &schema_converter);

    let mut rust_types = schema_rust_types;
    rust_types.extend(op_rust_types);
    warnings.extend(schema_warnings);
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
      client_methods_generated: None,
      client_headers_generated: None,
    };

    GenerationArtifacts {
      rust_types,
      operations_info,
      usage_recorder,
      stats,
    }
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
    let mut schema_cache = SharedSchemaCache::new();

    let operation_converter = OperationConverter::new(schema_converter, graph.spec());

    for (stable_id, method, path, operation) in self.operation_registry.operations_with_details() {
      let operation_id = operation.operation_id.as_deref().unwrap_or("unknown");

      match operation_converter.convert(
        stable_id,
        operation_id,
        method,
        path,
        operation,
        &mut usage_recorder,
        &mut schema_cache,
      ) {
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

    rust_types.extend(schema_cache.into_types());
    (rust_types, operations_info, warnings, usage_recorder)
  }

  fn generate_code_from_artifacts(
    &self,
    mut rust_types: Vec<RustType>,
    operations_info: &mut [OperationInfo],
    usage_recorder: TypeUsageRecorder,
  ) -> proc_macro2::TokenStream {
    Self::deduplicate_response_enums(&mut rust_types, operations_info);
    let seed_map = usage_recorder.into_usage_map();
    let type_usage = analyzer::build_type_usage_map(seed_map, &rust_types);
    analyzer::update_derives_from_usage(&mut rust_types, &type_usage);
    let error_schemas = ErrorAnalyzer::build_error_schema_set(operations_info, &rust_types);

    codegen::generate(&rust_types, &error_schemas, self.visibility)
  }

  fn deduplicate_response_enums(rust_types: &mut Vec<RustType>, operations_info: &mut [OperationInfo]) {
    // Map signature -> list of candidates
    // Signature tuple: (status_code, variant_name, schema_type_string, content_type)
    let mut signature_map: BTreeMap<ResponseEnumSignature, Vec<DuplicateCandidate>> = BTreeMap::new();

    for (i, rt) in rust_types.iter().enumerate() {
      if let RustType::ResponseEnum(def) = rt {
        let mut signature: Vec<_> = def
          .variants
          .iter()
          .map(|v| {
            (
              v.status_code.clone(),
              v.variant_name.clone(),
              v.schema_type
                .as_ref()
                .map_or_else(|| "None".to_string(), TypeRef::to_rust_type),
              v.content_type.clone(),
            )
          })
          .collect();
        // Sort to ensure order independence (variants set equality)
        signature.sort();

        signature_map.entry(signature).or_default().push(DuplicateCandidate {
          index: i,
          name: def.name.clone(),
        });
      }
    }

    let mut replacements: BTreeMap<String, String> = BTreeMap::new();
    let mut indices_to_remove = BTreeSet::new();

    for group in signature_map.values() {
      if group.len() > 1 {
        // Find canonical name (shortest, then lexicographically first)
        // Safe to unwrap because group.len() > 1
        let canonical = group
          .iter()
          .min_by(|a, b| a.name.len().cmp(&b.name.len()).then(a.name.cmp(&b.name)))
          .unwrap();

        for candidate in group {
          if candidate.name != canonical.name {
            replacements.insert(candidate.name.clone(), canonical.name.clone());
            indices_to_remove.insert(candidate.index);
          }
        }
      }
    }

    if replacements.is_empty() {
      return;
    }

    // Remove duplicates (in reverse order to preserve indices)
    for &idx in indices_to_remove.iter().rev() {
      rust_types.remove(idx);
    }

    // Update operations info
    for op in operations_info.iter_mut() {
      if let Some(ref current_name) = op.response_enum
        && let Some(new_name) = replacements.get(current_name)
      {
        op.response_enum = Some(new_name.clone());
      }
    }

    // Update StructDefs (ParseResponse methods)
    for rt in rust_types.iter_mut() {
      if let RustType::Struct(def) = rt {
        for method in &mut def.methods {
          if let StructMethodKind::ParseResponse { response_enum, .. } = &mut method.kind
            && let Some(new_name) = replacements.get(response_enum)
          {
            *response_enum = new_name.clone();
          }
        }
      }
    }
  }

  fn format_code(code: &proc_macro2::TokenStream) -> anyhow::Result<String> {
    let syntax_tree = syn::parse2(code.clone())?;
    Ok(prettyplease::unparse(&syntax_tree))
  }

  fn count_unique_headers(operations: &[OperationInfo]) -> usize {
    operations
      .iter()
      .flat_map(|op| &op.parameters)
      .filter(|param| matches!(param.location, ParameterLocation::Header))
      .map(|param| param.original_name.to_ascii_lowercase())
      .collect::<BTreeSet<_>>()
      .len()
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

  #[test]
  fn test_orchestrator_new_and_metadata() {
    let spec_json = include_str!("../../fixtures/basic_api.json");
    let spec: oas3::Spec = oas3::from_json(spec_json).unwrap();
    let orchestrator = Orchestrator::new(
      spec,
      Visibility::default(),
      false,
      None,
      None,
      FieldOptionalityPolicy::standard(),
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
    let spec_json = include_str!("../../fixtures/basic_api.json");
    let spec: oas3::Spec = oas3::from_json(spec_json).unwrap();
    let orchestrator = Orchestrator::new(
      spec,
      Visibility::default(),
      false,
      None,
      None,
      FieldOptionalityPolicy::standard(),
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
    let spec_json = include_str!("../../fixtures/basic_api.json");
    let spec: oas3::Spec = oas3::from_json(spec_json).unwrap();
    let orchestrator = Orchestrator::new(
      spec,
      Visibility::default(),
      false,
      None,
      None,
      FieldOptionalityPolicy::standard(),
      false,
    );

    let header = orchestrator.generate_header("test.yaml");
    assert!(header.contains("A test API.\n//! With multiple lines.\n//! For testing documentation."));
  }

  #[test]
  fn test_operation_exclusion() {
    let spec_json = include_str!("../../fixtures/operation_filtering.json");
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
    );
    let result = orchestrator.generate_with_header("test.json");
    assert!(result.is_ok());

    let (code, stats) = result.unwrap();
    assert_eq!(stats.operations_converted, 2);
    assert!(!code.contains("create_user"));
  }

  #[test]
  fn test_operation_exclusion_affects_schema_reachability() {
    let spec_json = include_str!("../../fixtures/operation_filtering.json");
    let spec_full: oas3::Spec = oas3::from_json(spec_json).unwrap();
    let orchestrator_full = Orchestrator::new(
      spec_full,
      Visibility::default(),
      false,
      None,
      None,
      FieldOptionalityPolicy::standard(),
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
    let spec_json = include_str!("../../fixtures/operation_filtering.json");
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
    let spec_json = include_str!("../../fixtures/content_types.json");
    let spec: oas3::Spec = oas3::from_json(spec_json).unwrap();
    let orchestrator = Orchestrator::new(
      spec,
      Visibility::default(),
      false,
      None,
      None,
      FieldOptionalityPolicy::standard(),
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
}

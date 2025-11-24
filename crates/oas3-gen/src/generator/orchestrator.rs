use std::collections::HashSet;

use strum::Display;

use super::converter::cache::SharedSchemaCache;
use crate::generator::{
  analyzer::{self, ErrorAnalyzer},
  ast::{LintConfig, OperationInfo, RustType},
  codegen::{self, Visibility, metadata::CodeMetadata},
  converter::{
    CodegenConfig, FieldOptionalityPolicy, SchemaConverter, TypeUsageRecorder, operations::OperationConverter,
    type_resolver::TypeResolver,
  },
  naming::inference::InlineTypeScanner,
  operation_registry::OperationRegistry,
  schema_graph::SchemaGraph,
};

const OAS3_GEN_VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Debug)]
pub struct Orchestrator {
  spec: oas3::Spec,
  visibility: Visibility,
  include_unused_schemas: bool,
  operation_registry: OperationRegistry,
  optionality_policy: FieldOptionalityPolicy,
  preserve_case_variants: bool,
  case_insensitive_enums: bool,
  no_helpers: bool,
}

#[derive(Debug)]
pub struct GenerationStats {
  pub types_generated: usize,
  pub structs_generated: usize,
  pub enums_generated: usize,
  pub enums_with_helpers_generated: usize,
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

#[derive(Debug, Display)]
pub enum GenerationWarning {
  #[strum(to_string = "Failed to convert schema '{schema_name}': {error}")]
  SchemaConversionFailed { schema_name: String, error: String },
  #[strum(to_string = "Failed to convert operation '{method} {path}': {error}")]
  OperationConversionFailed {
    method: String,
    path: String,
    error: String,
  },
  #[strum(to_string = "[{operation_id}] {message}")]
  OperationSpecific { operation_id: String, message: String },
}

impl Orchestrator {
  #[allow(clippy::too_many_arguments)]
  #[allow(clippy::fn_params_excessive_bools)]
  #[must_use]
  pub fn new(
    spec: oas3::Spec,
    visibility: Visibility,
    include_unused_schemas: bool,
    only_operations: Option<&HashSet<String>>,
    excluded_operations: Option<&HashSet<String>>,
    optionality_policy: FieldOptionalityPolicy,
    preserve_case_variants: bool,
    case_insensitive_enums: bool,
    no_helpers: bool,
  ) -> Self {
    let operation_registry = OperationRegistry::from_spec_filtered(&spec, only_operations, excluded_operations);
    Self {
      spec,
      visibility,
      include_unused_schemas,
      operation_registry,
      optionality_policy,
      preserve_case_variants,
      case_insensitive_enums,
      no_helpers,
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

    analyzer::deduplicate_response_enums(&mut rust_types, &mut operations_info);

    let header_count = analyzer::count_unique_headers(&operations_info);
    stats.client_methods_generated = Some(operations_info.len());
    stats.client_headers_generated = Some(header_count);

    let client_tokens = codegen::client::generate_client(&self.spec, &operations_info, &rust_types)?;
    let lint_config = LintConfig::default();
    let metadata = self.metadata();

    let final_code = codegen::generate_source(&client_tokens, &metadata, &lint_config, source_path, OAS3_GEN_VERSION)?;
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
      mut rust_types,
      mut operations_info,
      usage_recorder,
      stats,
    } = artifacts;

    let lint_config = LintConfig::default();
    let metadata = self.metadata();

    analyzer::deduplicate_response_enums(&mut rust_types, &mut operations_info);
    let seed_map = usage_recorder.into_usage_map();
    let type_usage = analyzer::build_type_usage_map(seed_map, &rust_types);
    analyzer::update_derives_from_usage(&mut rust_types, &type_usage);
    let error_schemas = ErrorAnalyzer::build_error_schema_set(&operations_info, &rust_types);

    let final_code = codegen::generate_file(
      &rust_types,
      &error_schemas,
      self.visibility,
      &metadata,
      &lint_config,
      source_path,
      OAS3_GEN_VERSION,
    )?;
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

    let config = CodegenConfig {
      preserve_case_variants: self.preserve_case_variants,
      case_insensitive_enums: self.case_insensitive_enums,
      no_helpers: self.no_helpers,
    };

    let schema_converter = if let Some(ref reachable) = operation_reachable {
      SchemaConverter::new_with_filter(&graph, reachable.clone(), self.optionality_policy.clone(), config)
    } else {
      SchemaConverter::new(&graph, self.optionality_policy.clone(), config)
    };

    let type_resolver = TypeResolver::new(&graph, config);
    let scanner = InlineTypeScanner::new(&graph, type_resolver);
    let scan_result = scanner.scan_and_compute_names().unwrap_or_default();

    let mut cache = SharedSchemaCache::new();
    cache.set_precomputed_names(scan_result.names, scan_result.enum_names);

    let (schema_rust_types, schema_warnings) =
      Self::convert_all_schemas(&graph, &schema_converter, operation_reachable.as_ref(), &mut cache);

    let (op_rust_types, operations_info, op_warnings, usage_recorder) =
      self.convert_all_operations(&graph, &schema_converter, &mut cache);

    let mut rust_types = schema_rust_types;
    rust_types.extend(op_rust_types);
    rust_types.extend(cache.into_types());

    warnings.extend(schema_warnings);
    warnings.extend(op_warnings);

    let mut structs_generated = 0;
    let mut enums_generated = 0;
    let mut enums_with_helpers_generated = 0;
    let mut type_aliases_generated = 0;

    for rust_type in &rust_types {
      match rust_type {
        RustType::Struct(_) => structs_generated += 1,
        RustType::Enum(def) => {
          enums_generated += 1;
          if !def.methods.is_empty() {
            enums_with_helpers_generated += 1;
          }
        }
        RustType::DiscriminatedEnum(_) | RustType::ResponseEnum(_) => enums_generated += 1,
        RustType::TypeAlias(_) => type_aliases_generated += 1,
      }
    }

    let types_generated = structs_generated + enums_generated + type_aliases_generated;

    let stats = GenerationStats {
      types_generated,
      structs_generated,
      enums_generated,
      enums_with_helpers_generated,
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
    cache: &mut SharedSchemaCache,
  ) -> (Vec<RustType>, Vec<GenerationWarning>) {
    let mut rust_types = vec![];
    let mut warnings = vec![];

    for schema_name in graph.schema_names() {
      if let Some(filter) = operation_reachable
        && !filter.contains(schema_name.as_str())
      {
        continue;
      }

      if let Some(schema) = graph.get_schema(schema_name)
        && !schema.enum_values.is_empty()
      {
        match schema_converter.convert_schema(schema_name, schema, Some(cache)) {
          Ok(types) => rust_types.extend(types),
          Err(e) => warnings.push(GenerationWarning::SchemaConversionFailed {
            schema_name: schema_name.clone(),
            error: e.to_string(),
          }),
        }
      }
    }

    for schema_name in graph.schema_names() {
      if let Some(filter) = operation_reachable
        && !filter.contains(schema_name.as_str())
      {
        continue;
      }

      if let Some(schema) = graph.get_schema(schema_name)
        && schema.enum_values.is_empty()
      {
        match schema_converter.convert_schema(schema_name, schema, Some(cache)) {
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
    cache: &mut SharedSchemaCache,
  ) -> (
    Vec<RustType>,
    Vec<OperationInfo>,
    Vec<GenerationWarning>,
    TypeUsageRecorder,
  ) {
    let mut rust_types = vec![];
    let mut operations_info = vec![];
    let mut warnings = vec![];
    let mut usage_recorder = TypeUsageRecorder::new();

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
        cache,
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

    (rust_types, operations_info, warnings, usage_recorder)
  }
}

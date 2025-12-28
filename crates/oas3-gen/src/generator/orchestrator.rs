use std::{collections::HashSet, sync::Arc};

use quote::ToTokens;
use strum::Display;

use super::converter::cache::SharedSchemaCache;
use crate::generator::{
  analyzer::TypeAnalyzer,
  ast::{LintConfig, OperationInfo, OperationKind, ParameterLocation, RustType},
  codegen::{self, Visibility, client::ClientGenerator, metadata::CodeMetadata, mod_file::ModFileGenerator},
  converter::{
    CodegenConfig, EnumCasePolicy, EnumDeserializePolicy, EnumHelperPolicy, ODataPolicy, SchemaConverter,
    TypeUsageRecorder, inline_scanner::InlineTypeScanner, operations::OperationConverter,
  },
  operation_registry::OperationRegistry,
  schema_registry::SchemaRegistry,
};

const OAS3_GEN_VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Debug)]
pub struct Orchestrator {
  spec: oas3::Spec,
  visibility: Visibility,
  include_unused_schemas: bool,
  operation_registry: OperationRegistry,
  odata_support: bool,
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
  pub webhooks_converted: usize,
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

#[derive(Debug)]
pub struct ClientModOutput {
  pub types_code: String,
  pub client_code: String,
  pub mod_code: String,
  pub stats: GenerationStats,
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

impl GenerationWarning {
  pub fn is_skipped_item(&self) -> bool {
    matches!(
      self,
      Self::SchemaConversionFailed { .. } | Self::OperationConversionFailed { .. }
    )
  }
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
    odata_support: bool,
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
      odata_support,
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
      usage_recorder,
    } = artifacts;

    let seed_map = usage_recorder.into_usage_map();
    let analyzer = TypeAnalyzer::new(&mut rust_types, &mut operations_info, seed_map);
    let _ = analyzer.analyze();

    stats.client_methods_generated = Some(operations_info.len());
    stats.client_headers_generated = Some(Self::count_unique_headers(&operations_info));

    let metadata = CodeMetadata::from_spec(&self.spec);
    let client_generator = ClientGenerator::new(&metadata, &operations_info, &rust_types, self.visibility);
    let client_tokens = client_generator.into_token_stream();
    let lint_config = LintConfig::default();

    let final_code = codegen::generate_source(
      &client_tokens,
      &metadata,
      Some(&lint_config),
      source_path,
      OAS3_GEN_VERSION,
    )?;
    Ok((final_code, stats))
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
    let metadata = CodeMetadata::from_spec(&self.spec);

    let seed_map = usage_recorder.into_usage_map();
    let analyzer = TypeAnalyzer::new(&mut rust_types, &mut operations_info, seed_map);
    let analysis = analyzer.analyze();

    let final_code = codegen::generate_file(
      &rust_types,
      &analysis.error_schemas,
      self.visibility,
      &metadata,
      &lint_config,
      source_path,
      OAS3_GEN_VERSION,
    )?;
    Ok((final_code, stats))
  }

  pub fn generate_client_mod(&self, source_path: &str) -> anyhow::Result<ClientModOutput> {
    let artifacts = self.collect_generation_artifacts();
    let GenerationArtifacts {
      mut rust_types,
      mut operations_info,
      usage_recorder,
      mut stats,
    } = artifacts;

    let metadata = CodeMetadata::from_spec(&self.spec);

    let seed_map = usage_recorder.into_usage_map();
    let analyzer = TypeAnalyzer::new(&mut rust_types, &mut operations_info, seed_map);
    let analysis = analyzer.analyze();

    let types_tokens = codegen::generate(&rust_types, &analysis.error_schemas, self.visibility);
    let types_code = codegen::generate_source(&types_tokens, &metadata, None, source_path, OAS3_GEN_VERSION)?;

    stats.client_methods_generated = Some(operations_info.len());
    stats.client_headers_generated = Some(Self::count_unique_headers(&operations_info));

    let client_generator =
      ClientGenerator::new(&metadata, &operations_info, &rust_types, self.visibility).with_types_import();
    let client_ident = client_generator.client_ident();
    let client_tokens = client_generator.into_token_stream();
    let client_code = codegen::generate_source(&client_tokens, &metadata, None, source_path, OAS3_GEN_VERSION)?;

    let mod_generator = ModFileGenerator::new(&metadata, &client_ident, self.visibility);
    let mod_code = mod_generator.generate(source_path, OAS3_GEN_VERSION)?;

    Ok(ClientModOutput {
      types_code,
      client_code,
      mod_code,
      stats,
    })
  }

  fn collect_generation_artifacts(&self) -> GenerationArtifacts {
    let (mut graph, mut warnings) = SchemaRegistry::new(self.spec.clone());
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
      enum_case: if self.preserve_case_variants {
        EnumCasePolicy::Preserve
      } else {
        EnumCasePolicy::Deduplicate
      },
      enum_helpers: if self.no_helpers {
        EnumHelperPolicy::Disable
      } else {
        EnumHelperPolicy::Generate
      },
      enum_deserialize: if self.case_insensitive_enums {
        EnumDeserializePolicy::CaseInsensitive
      } else {
        EnumDeserializePolicy::CaseSensitive
      },
      odata: if self.odata_support {
        ODataPolicy::Enabled
      } else {
        ODataPolicy::Disabled
      },
    };

    let graph = Arc::new(graph);

    let schema_converter = if let Some(ref reachable) = operation_reachable {
      SchemaConverter::new_with_filter(&graph, reachable.clone(), config)
    } else {
      SchemaConverter::new(&graph, config)
    };

    let scanner = InlineTypeScanner::new(&graph);
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

    let webhooks_converted = operations_info
      .iter()
      .filter(|op| matches!(op.kind, OperationKind::Webhook))
      .count();

    let stats = GenerationStats {
      types_generated,
      structs_generated,
      enums_generated,
      enums_with_helpers_generated,
      type_aliases_generated,
      operations_converted: operations_info.len(),
      webhooks_converted,
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

  /// Converts all schemas from the OpenAPI spec to Rust types.
  ///
  /// Processing order: Enums are converted before other schema types to ensure deterministic
  /// code generation. This ordering prevents issues where enum types might be referenced
  /// before they are defined, and ensures generated code is stable across multiple runs.
  ///
  /// Algorithm:
  /// 1. Collect all schema names from the graph
  /// 2. Sort schemas by `is_none_or(|s| s.enum_values.is_empty())` to place enums first:
  ///    - Schemas with non-empty `enum_values` sort as `false` (enums first)
  ///    - Schemas with empty `enum_values` sort as `true` (non-enums after)
  ///    - Missing schemas sort as `true` (last)
  /// 3. Process each schema in order, applying operation reachability filtering if enabled
  /// 4. Collect conversion errors as warnings rather than failing fast
  ///
  /// The `operation_reachable` filter, when provided, limits conversion to schemas that are
  /// referenced by at least one operation, reducing generated code size.
  fn convert_all_schemas(
    graph: &SchemaRegistry,
    schema_converter: &SchemaConverter,
    operation_reachable: Option<&std::collections::BTreeSet<String>>,
    cache: &mut SharedSchemaCache,
  ) -> (Vec<RustType>, Vec<GenerationWarning>) {
    let mut rust_types = vec![];
    let mut warnings = vec![];

    let mut schema_names = graph.schema_names();
    schema_names.sort_by_key(|name| {
      graph
        .get_schema(name)
        .is_none_or(|schema| schema.enum_values.is_empty())
    });

    for schema_name in &schema_names {
      if operation_reachable.is_some_and(|filter| !filter.contains(schema_name.as_str())) {
        continue;
      }
      if let Some(schema) = graph.get_schema(schema_name) {
        let _ = cache.register_top_level_schema(schema, schema_name);
      }
    }

    for schema_name in schema_names {
      if operation_reachable.is_some_and(|filter| !filter.contains(schema_name.as_str())) {
        continue;
      }

      let Some(schema) = graph.get_schema(schema_name) else {
        continue;
      };

      match schema_converter.convert_schema(schema_name, schema, Some(cache)) {
        Ok(types) => rust_types.extend(types),
        Err(e) => warnings.push(GenerationWarning::SchemaConversionFailed {
          schema_name: schema_name.clone(),
          error: e.to_string(),
        }),
      }
    }
    (rust_types, warnings)
  }

  fn count_unique_headers(operations: &[OperationInfo]) -> usize {
    operations
      .iter()
      .flat_map(|op| &op.parameters)
      .filter(|param| matches!(param.location, ParameterLocation::Header))
      .map(|param| param.original_name.to_ascii_lowercase())
      .collect::<std::collections::BTreeSet<_>>()
      .len()
  }

  fn convert_all_operations(
    &self,
    graph: &SchemaRegistry,
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

    for (stable_id, method, path, operation, kind) in self.operation_registry.operations_with_details() {
      let operation_id = operation.operation_id.as_deref().unwrap_or("unknown");

      match operation_converter.convert(
        stable_id,
        operation_id,
        method,
        path,
        kind,
        operation,
        &mut usage_recorder,
        cache,
      ) {
        Ok(result) => {
          warnings.extend(
            result
              .operation_info
              .warnings
              .iter()
              .map(|w| GenerationWarning::OperationSpecific {
                operation_id: result.operation_info.operation_id.clone(),
                message: w.clone(),
              }),
          );
          rust_types.extend(result.types);
          operations_info.push(result.operation_info);
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

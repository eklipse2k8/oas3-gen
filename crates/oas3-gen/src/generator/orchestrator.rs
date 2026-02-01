use std::{
  collections::{BTreeSet, HashSet},
  rc::Rc,
  sync::Arc,
};

use strum::Display;

use crate::generator::{
  ast::{ClientRootNode, OperationInfo, OperationKind, RustType, ServerRequestTraitDef, constants::HttpHeaderRef},
  codegen::{GeneratedResult, SchemaCodeGenerator, Visibility},
  converter::{
    CodegenConfig, ConverterContext, GenerationTarget, OperationsProcessor, SchemaConverter, TypeUsageRecorder,
    build_server_trait, cache::SharedSchemaCache,
  },
  mode::GenerationMode,
  operation_registry::OperationRegistry,
  postprocess::{PostprocessOutput, postprocess},
  schema_registry::SchemaRegistry,
};

const OAS3_GEN_VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Debug)]
pub struct Orchestrator {
  spec: oas3::Spec,
  visibility: Visibility,
  config: CodegenConfig,
  operation_registry: OperationRegistry,
  include_unused_schemas: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
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
  config: CodegenConfig,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GeneratedFinalOutput {
  pub code: GeneratedResult,
  pub stats: GenerationStats,
}

impl GeneratedFinalOutput {
  pub fn new(code: GeneratedResult, stats: GenerationStats) -> Self {
    Self { code, stats }
  }
}

struct ConversionArtifacts {
  config: CodegenConfig,
  types: Vec<RustType>,
  operations: Vec<OperationInfo>,
  header_refs: Vec<HttpHeaderRef>,
  uses: BTreeSet<String>,
  client: ClientRootNode,
  server_trait: Option<ServerRequestTraitDef>,
  stats: GenerationStats,
}

#[derive(Debug, Clone, PartialEq, Eq, Display)]
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
  #[must_use]
  pub fn new(
    spec: oas3::Spec,
    visibility: Visibility,
    config: CodegenConfig,
    only_operations: Option<&HashSet<String>>,
    excluded_operations: Option<&HashSet<String>>,
    include_unused_schemas: bool,
  ) -> Self {
    let operation_registry = OperationRegistry::with_filters(&spec, only_operations, excluded_operations);
    Self {
      spec,
      visibility,
      config,
      operation_registry,
      include_unused_schemas,
    }
  }

  pub fn generate(&self, mode: &dyn GenerationMode, source_path: &str) -> anyhow::Result<GeneratedFinalOutput> {
    let artifacts = self.run_conversion_and_analysis();

    let codegen = SchemaCodeGenerator::builder()
      .config(artifacts.config)
      .rust_types(artifacts.types)
      .operations(artifacts.operations)
      .header_refs(artifacts.header_refs)
      .uses(artifacts.uses)
      .client(artifacts.client)
      .maybe_server_trait(artifacts.server_trait)
      .visibility(self.visibility)
      .source_path(source_path.to_string())
      .gen_version(OAS3_GEN_VERSION.to_string())
      .build();

    let output = mode.generate(&codegen)?;
    Ok(GeneratedFinalOutput::new(output, artifacts.stats))
  }

  fn run_conversion_and_analysis(&self) -> ConversionArtifacts {
    let artifacts = self.collect_generation_artifacts();
    let GenerationArtifacts {
      rust_types,
      operations_info,
      usage_recorder,
      stats,
      config,
    } = artifacts;

    let client = ClientRootNode::from(&self.spec);

    let seed_map = usage_recorder.into_usage_map();
    let PostprocessOutput {
      types,
      operations,
      header_refs,
      uses,
    } = postprocess(rust_types, operations_info, seed_map, config.target);

    let server_trait = if config.target == GenerationTarget::Server {
      build_server_trait(&operations)
    } else {
      None
    };

    ConversionArtifacts {
      config,
      types,
      operations,
      header_refs,
      uses,
      client,
      server_trait,
      stats,
    }
  }

  fn collect_generation_artifacts(&self) -> GenerationArtifacts {
    let init_result = SchemaRegistry::from_spec(self.spec.clone());
    let mut graph = init_result.registry;
    let mut warnings = init_result.warnings;
    graph.build_dependencies();
    let cycle_details = graph.detect_cycles();

    let operation_reachable = if self.include_unused_schemas {
      None
    } else {
      Some(Arc::new(graph.reachable(&self.operation_registry)))
    };

    let total_schemas = graph.keys().len();
    let orphaned_schemas_count = if let Some(ref reachable) = operation_reachable {
      total_schemas.saturating_sub(reachable.len())
    } else {
      0
    };

    let config = self.config.clone();

    let scan_result = graph.scan_and_compute_names().unwrap_or_default();

    let graph = Arc::new(graph);

    let mut cache = SharedSchemaCache::new();
    cache.set_precomputed_names(scan_result.names, scan_result.enum_names, scan_result.schema_metadata);

    let context = Rc::new(ConverterContext::new(
      graph.clone(),
      config,
      cache,
      operation_reachable.clone(),
    ));

    let schema_converter = SchemaConverter::new(&context);

    let (schema_rust_types, schema_warnings) =
      Self::convert_all_schemas(&graph, &schema_converter, operation_reachable.as_deref());

    let operations_processor = OperationsProcessor::new(context.clone(), schema_converter.clone());
    let operations_output = operations_processor.process_all(self.operation_registry.operations());

    let mut rust_types = schema_rust_types;
    rust_types.extend(operations_output.types);
    rust_types.extend(context.cache.replace(SharedSchemaCache::new()).into_types());

    warnings.extend(schema_warnings);
    warnings.extend(operations_output.warnings);

    let operations_info = operations_output.operations;

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
      client_methods_generated: Some(operations_output.usage_recorder.methods_generated()),
      client_headers_generated: Some(operations_output.usage_recorder.headers_generated()),
    };

    let config = context.config.clone();

    GenerationArtifacts {
      rust_types,
      operations_info,
      usage_recorder: operations_output.usage_recorder,
      stats,
      config,
    }
  }

  /// Converts all schemas from the OpenAPI spec to Rust types.
  fn convert_all_schemas(
    graph: &SchemaRegistry,
    schema_converter: &SchemaConverter,
    operation_reachable: Option<&BTreeSet<String>>,
  ) -> (Vec<RustType>, Vec<GenerationWarning>) {
    let mut rust_types = vec![];
    let mut warnings = vec![];

    let mut schema_names = graph.keys();
    schema_names.sort_by_key(|name| graph.get(name).is_none_or(|schema| schema.enum_values.is_empty()));

    {
      let mut cache = schema_converter.context().cache.borrow_mut();
      for schema_name in &schema_names {
        if operation_reachable.is_some_and(|filter| !filter.contains(schema_name.as_str())) {
          continue;
        }
        if let Some(schema) = graph.get(schema_name) {
          let _ = cache.register_top_level_schema(schema, schema_name);
        }
      }
    }

    for schema_name in schema_names {
      if operation_reachable.is_some_and(|filter| !filter.contains(schema_name.as_str())) {
        continue;
      }

      let Some(schema) = graph.get(schema_name) else {
        continue;
      };

      match schema_converter.convert_schema(schema_name, schema) {
        Ok(types) => rust_types.extend(types),
        Err(e) => warnings.push(GenerationWarning::SchemaConversionFailed {
          schema_name: schema_name.clone(),
          error: e.to_string(),
        }),
      }
    }
    (rust_types, warnings)
  }
}

use std::{collections::HashSet, rc::Rc, sync::Arc};

use oas3::Spec;

use crate::generator::{
  ast::{ClientRootNode, OperationInfo, RustType},
  codegen::{GeneratedResult, SchemaCodeGenerator, Visibility},
  converter::{
    CodegenConfig, ConverterContext, GenerationTarget, OperationsProcessor, SchemaConverter, SerdeUsageRecorder,
    build_server_trait, cache::SharedSchemaCache,
  },
  metrics::GenerationStats,
  mode::GenerationMode,
  operation_registry::OperationRegistry,
  postprocess::PostprocessOutput,
  schema_registry::SchemaRegistry,
};

const OAS3_GEN_VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Debug)]
pub struct Orchestrator {
  spec: Spec,
  visibility: Visibility,
  config: CodegenConfig,
  operation_registry: OperationRegistry,
  include_unused_schemas: bool,
}

struct GenerationArtifacts {
  rust_types: Vec<RustType>,
  operations_info: Vec<OperationInfo>,
  serde_recorder: SerdeUsageRecorder,
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

impl Orchestrator {
  #[must_use]
  pub fn new(
    spec: Spec,
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
    let artifacts = self.collect_generation_artifacts();
    let serde_use = artifacts.serde_recorder.into_usage_map();
    let output = PostprocessOutput::new(
      artifacts.rust_types,
      artifacts.operations_info,
      serde_use,
      artifacts.config.target,
    );

    let server_trait = if artifacts.config.target == GenerationTarget::Server {
      build_server_trait(&output.operations)
    } else {
      None
    };

    let codegen = SchemaCodeGenerator::builder()
      .config(artifacts.config)
      .rust_types(output.types)
      .operations(output.operations)
      .header_refs(output.header_refs)
      .uses(output.uses)
      .client(ClientRootNode::from(&self.spec))
      .maybe_server_trait(server_trait)
      .visibility(self.visibility)
      .source_path(source_path.to_string())
      .gen_version(OAS3_GEN_VERSION.to_string())
      .build();

    let output = mode.generate(&codegen)?;
    Ok(GeneratedFinalOutput::new(output, artifacts.stats))
  }

  fn collect_generation_artifacts(&self) -> GenerationArtifacts {
    let mut stats = GenerationStats::default();
    let mut graph = SchemaRegistry::new(&self.spec, &mut stats);

    let mut cache = SharedSchemaCache::new();
    cache.initialize_from_schemas(graph.schemas());
    let union_fingerprints = cache.union_fingerprints().clone();

    let (cycle_details, reachable) = graph.initialize(
      &self.operation_registry,
      self.include_unused_schemas,
      &union_fingerprints,
    );

    let graph = Arc::new(graph);
    let scan_result = graph.scan_and_compute_names().unwrap_or_default();
    let reachable_schemas = reachable.map(Arc::new);

    cache.set_precomputed_names(scan_result.names, scan_result.enum_names, scan_result.schema_metadata);

    let context = Rc::new(ConverterContext::new(
      graph.clone(),
      self.config.clone(),
      cache,
      reachable_schemas.clone(),
    ));

    let schema_converter = SchemaConverter::new(&context);
    let mut schema_rust_types = schema_converter.convert_all_schemas(reachable_schemas.as_deref(), &mut stats);

    let operations_processor = OperationsProcessor::new(context.clone(), schema_converter.clone());
    let operations_output = operations_processor.process_all(self.operation_registry.operations());

    schema_rust_types.extend(operations_output.types);
    schema_rust_types.extend(context.cache.borrow_mut().take_types());

    stats.record_orphaned_schemas(if let Some(ref reachable_schemas) = reachable_schemas {
      let total_schemas = graph.keys().len();
      total_schemas.saturating_sub(reachable_schemas.len())
    } else {
      0
    });

    stats.record_warnings(operations_output.warnings);
    stats.record_rust_types(&schema_rust_types);
    stats.record_operations(&operations_output.operations);
    stats.record_cycles(cycle_details);
    stats.record_client_methods(operations_output.operations.len());
    stats.record_client_headers(operations_output.unique_headers.len());

    GenerationArtifacts {
      rust_types: schema_rust_types,
      operations_info: operations_output.operations,
      serde_recorder: operations_output.usage_recorder,
      stats,
      config: context.config.clone(),
    }
  }
}

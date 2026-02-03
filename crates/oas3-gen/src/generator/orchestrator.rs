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
    let serde_usage = artifacts.serde_recorder.into_usage_map();
    let postprocessed = PostprocessOutput::new(
      artifacts.rust_types,
      artifacts.operations_info,
      serde_usage,
      artifacts.config.target,
    );

    let server_trait_def = if artifacts.config.target == GenerationTarget::Server {
      build_server_trait(&postprocessed.operations)
    } else {
      None
    };

    let codegen = SchemaCodeGenerator::builder()
      .config(artifacts.config)
      .rust_types(postprocessed.types)
      .operations(postprocessed.operations)
      .header_refs(postprocessed.header_refs)
      .uses(postprocessed.uses)
      .client(ClientRootNode::from(&self.spec))
      .maybe_server_trait(server_trait_def)
      .visibility(self.visibility)
      .source_path(source_path.to_string())
      .gen_version(OAS3_GEN_VERSION.to_string())
      .build();

    let code = mode.generate(&codegen)?;
    Ok(GeneratedFinalOutput::new(code, artifacts.stats))
  }

  fn collect_generation_artifacts(&self) -> GenerationArtifacts {
    let mut stats = GenerationStats::default();
    let mut schema_graph = SchemaRegistry::new(&self.spec, &mut stats);

    let mut cache = SharedSchemaCache::new();
    cache.initialize_from_schemas(schema_graph.schemas());
    let union_fingerprints = cache.union_fingerprints().clone();

    let (cycle_info, filtered_schemas) = schema_graph.initialize(
      &self.operation_registry,
      self.include_unused_schemas,
      &union_fingerprints,
    );

    let schema_graph = Arc::new(schema_graph);
    let schema_names = schema_graph.scan_and_compute_names().unwrap_or_default();
    let filtered_schemas = filtered_schemas.map(Arc::new);

    cache.set_precomputed_names(
      schema_names.names,
      schema_names.enum_names,
      schema_names.schema_metadata,
    );

    let context = Rc::new(ConverterContext::new(
      schema_graph.clone(),
      self.config.clone(),
      cache,
      filtered_schemas.clone(),
    ));

    let converter = SchemaConverter::new(&context);
    let mut rust_types = converter.convert_all_schemas(filtered_schemas.as_deref(), &mut stats);

    let processor = OperationsProcessor::new(context.clone(), converter.clone());
    let operation_results = processor.process_all(self.operation_registry.operations());

    rust_types.extend(operation_results.types);
    rust_types.extend(context.cache.borrow_mut().take_types());

    stats.record_orphaned_schemas(if let Some(ref schemas) = filtered_schemas {
      let total = schema_graph.keys().len();
      total.saturating_sub(schemas.len())
    } else {
      0
    });

    stats.record_warnings(operation_results.warnings);
    stats.record_rust_types(&rust_types);
    stats.record_operations(&operation_results.operations);
    stats.record_cycles(cycle_info);
    stats.record_client_methods(operation_results.operations.len());
    stats.record_client_headers(operation_results.unique_headers.len());

    GenerationArtifacts {
      rust_types,
      operations_info: operation_results.operations,
      serde_recorder: operation_results.usage_recorder,
      stats,
      config: context.config.clone(),
    }
  }
}

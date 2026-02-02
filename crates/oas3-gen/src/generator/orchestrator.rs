use std::{
  collections::{BTreeSet, HashSet},
  rc::Rc,
  sync::Arc,
};

use oas3::Spec;

use crate::generator::{
  ast::{ClientRootNode, OperationInfo, RustType},
  codegen::{GeneratedResult, SchemaCodeGenerator, Visibility},
  converter::{
    CodegenConfig, ConverterContext, GenerationTarget, OperationsProcessor, SchemaConverter, TypeUsageRecorder,
    build_server_trait, cache::SharedSchemaCache,
  },
  metrics::{GenerationStats, GenerationWarning},
  mode::GenerationMode,
  operation_registry::OperationRegistry,
  postprocess::postprocess,
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
    let seed_map = artifacts.usage_recorder.into_usage_map();
    let output = postprocess(
      artifacts.rust_types,
      artifacts.operations_info,
      seed_map,
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

    let operation_reachable = reachable.map(Arc::new);

    let total_schemas = graph.keys().len();
    let orphaned_schemas_count = if let Some(ref reachable) = operation_reachable {
      total_schemas.saturating_sub(reachable.len())
    } else {
      0
    };
    let scan_result = graph.scan_and_compute_names().unwrap_or_default();

    let graph = Arc::new(graph);

    cache.set_precomputed_names(scan_result.names, scan_result.enum_names, scan_result.schema_metadata);

    let context = Rc::new(ConverterContext::new(
      graph.clone(),
      self.config.clone(),
      cache,
      operation_reachable.clone(),
    ));

    let schema_converter = SchemaConverter::new(&context);

    let mut schema_rust_types =
      Self::convert_all_schemas(&graph, &schema_converter, operation_reachable.as_deref(), &mut stats);

    let operations_processor = OperationsProcessor::new(context.clone(), schema_converter.clone());
    let operations_output = operations_processor.process_all(self.operation_registry.operations());

    schema_rust_types.extend(operations_output.types);
    schema_rust_types.extend(context.cache.borrow_mut().take_types());

    stats.record_warnings(operations_output.warnings);

    let operations_info = operations_output.operations;

    stats.record_rust_types(&schema_rust_types);
    stats.record_operations(&operations_info);
    stats.record_cycles(cycle_details);
    stats.record_orphaned_schemas(orphaned_schemas_count);
    stats.record_client_methods(operations_info.len());
    stats.record_client_headers(operations_output.unique_headers.len());

    let config = context.config.clone();

    GenerationArtifacts {
      rust_types: schema_rust_types,
      operations_info,
      usage_recorder: operations_output.usage_recorder,
      stats,
      config,
    }
  }

  /// Converts all schemas from the OpenAPI spec to Rust types.
  ///
  /// Processes schemas in two phases: first registers all top-level schemas to enable
  /// deduplication, then converts each to Rust types. Enums are processed before other
  /// types to ensure their names are available for reference.
  fn convert_all_schemas(
    graph: &SchemaRegistry,
    schema_converter: &SchemaConverter,
    operation_reachable: Option<&BTreeSet<String>>,
    stats: &mut GenerationStats,
  ) -> Vec<RustType> {
    let schemas = graph.schemas();

    let filtered = schemas
      .iter()
      .filter(|(name, _)| operation_reachable.is_none_or(|filter| filter.contains(name.as_str())))
      .collect::<Vec<_>>();

    let (enums, non_enums): (Vec<_>, Vec<_>) = filtered
      .into_iter()
      .partition(|(_, schema)| !schema.enum_values.is_empty());

    let ordered = enums.into_iter().chain(non_enums).collect::<Vec<_>>();

    {
      let mut cache = schema_converter.context().cache.borrow_mut();
      for (name, schema) in &ordered {
        let _ = cache.register_top_level_schema(schema, name);
      }
    }

    ordered.into_iter().fold(vec![], |mut acc, (name, schema)| {
      match schema_converter.convert_schema(name, schema) {
        Ok(types) => acc.extend(types),
        Err(e) => stats.record_warning(GenerationWarning::SchemaConversionFailed {
          schema_name: name.clone(),
          error: e.to_string(),
        }),
      }
      acc
    })
  }
}

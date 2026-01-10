use std::{
  collections::{HashMap, HashSet},
  rc::Rc,
  sync::Arc,
};

use strum::Display;

use super::converter::cache::SharedSchemaCache;
use crate::generator::{
  analyzer::TypeAnalyzer,
  ast::{ClientRootNode, OperationInfo, OperationKind, RustType, StructToken},
  codegen::{SchemaCodeGenerator, Visibility},
  converter::{
    CodegenConfig, ConverterContext, EnumCasePolicy, EnumDeserializePolicy, EnumHelperPolicy, GenerationTarget,
    ODataPolicy, SchemaConverter, TypeUsageRecorder, operations::OperationConverter,
  },
  naming::identifiers::to_rust_type_name,
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
  generation_target: GenerationTarget,
  customizations: HashMap<String, String>,
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

#[derive(Debug)]
pub struct ServerModOutput {
  pub types_code: String,
  pub server_code: String,
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
    generation_target: GenerationTarget,
    customizations: HashMap<String, String>,
  ) -> Self {
    let operation_registry = OperationRegistry::with_filters(&spec, only_operations, excluded_operations);
    Self {
      spec,
      visibility,
      include_unused_schemas,
      operation_registry,
      odata_support,
      preserve_case_variants,
      case_insensitive_enums,
      no_helpers,
      generation_target,
      customizations,
    }
  }

  // TODO: Create a client converter struct to encapsulate this logic
  fn client_struct_name(&self) -> StructToken {
    let title = self.spec.info.title.clone();

    let client_name = if title.is_empty() {
      "ApiClient".to_string()
    } else {
      format!("{}Client", to_rust_type_name(&title))
    };

    StructToken::new(client_name)
  }

  pub fn generate_client_with_header(&self, source_path: &str) -> anyhow::Result<(String, GenerationStats)> {
    let (rust_types, operations_info, client, mut stats) = self.run_conversion_and_analysis();

    let codegen = SchemaCodeGenerator::new(
      rust_types,
      operations_info,
      client,
      self.visibility,
      source_path.to_string(),
      OAS3_GEN_VERSION.to_string(),
    );
    let output = codegen.generate_client()?;

    stats.client_methods_generated = Some(output.methods_generated);
    stats.client_headers_generated = Some(output.headers_generated);

    Ok((output.code, stats))
  }

  pub fn generate_with_header(&self, source_path: &str) -> anyhow::Result<(String, GenerationStats)> {
    let (rust_types, operations_info, client, stats) = self.run_conversion_and_analysis();

    let codegen = SchemaCodeGenerator::new(
      rust_types,
      operations_info,
      client,
      self.visibility,
      source_path.to_string(),
      OAS3_GEN_VERSION.to_string(),
    );
    let output = codegen.generate_types()?;

    Ok((output.code, stats))
  }

  pub fn generate_client_mod(&self, source_path: &str) -> anyhow::Result<ClientModOutput> {
    let (rust_types, operations_info, client, mut stats) = self.run_conversion_and_analysis();

    let codegen = SchemaCodeGenerator::new(
      rust_types,
      operations_info,
      client,
      self.visibility,
      source_path.to_string(),
      OAS3_GEN_VERSION.to_string(),
    );
    let output = codegen.generate_client_mod()?;

    stats.client_methods_generated = Some(output.methods_generated);
    stats.client_headers_generated = Some(output.headers_generated);

    Ok(ClientModOutput {
      types_code: output.types_code,
      client_code: output.client_code,
      mod_code: output.mod_code,
      stats,
    })
  }

  pub fn generate_server_mod(&self, source_path: &str) -> anyhow::Result<ServerModOutput> {
    let (rust_types, operations_info, client, stats) = self.run_conversion_and_analysis();

    let codegen = SchemaCodeGenerator::new(
      rust_types,
      operations_info,
      client,
      self.visibility,
      source_path.to_string(),
      OAS3_GEN_VERSION.to_string(),
    );
    let output = codegen.generate_server_mod()?;

    Ok(ServerModOutput {
      types_code: output.types_code,
      server_code: output.server_code,
      mod_code: output.mod_code,
      stats,
    })
  }

  fn run_conversion_and_analysis(&self) -> (Vec<RustType>, Vec<OperationInfo>, ClientRootNode, GenerationStats) {
    let artifacts = self.collect_generation_artifacts();
    let GenerationArtifacts {
      mut rust_types,
      mut operations_info,
      usage_recorder,
      stats,
    } = artifacts;

    let client = ClientRootNode::builder()
      .name(self.client_struct_name())
      .info(&self.spec.info)
      .servers(&self.spec.servers)
      .build();

    let seed_map = usage_recorder.into_usage_map();
    let analyzer = TypeAnalyzer::new(&mut rust_types, &mut operations_info, seed_map);
    analyzer.analyze();

    (rust_types, operations_info, client, stats)
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
      target: self.generation_target,
      customizations: self.customizations.clone(),
    };

    let scan_result = graph.scan_and_compute_names().unwrap_or_default();

    let graph = Arc::new(graph);

    let mut cache = SharedSchemaCache::new();
    cache.set_precomputed_names(scan_result.names, scan_result.enum_names);

    let context = Rc::new(ConverterContext::new(
      graph.clone(),
      config,
      cache,
      operation_reachable.clone(),
    ));

    let schema_converter = SchemaConverter::new(&context);

    let (schema_rust_types, schema_warnings) =
      Self::convert_all_schemas(&graph, &schema_converter, operation_reachable.as_deref());

    let (op_rust_types, operations_info, op_warnings, usage_recorder) =
      self.convert_all_operations(&context, &schema_converter);

    let mut rust_types = schema_rust_types;
    rust_types.extend(op_rust_types);
    rust_types.extend(context.cache.replace(SharedSchemaCache::new()).into_types());

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
  fn convert_all_schemas(
    graph: &SchemaRegistry,
    schema_converter: &SchemaConverter,
    operation_reachable: Option<&std::collections::BTreeSet<String>>,
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

  fn convert_all_operations(
    &self,
    context: &Rc<ConverterContext>,
    schema_converter: &SchemaConverter,
  ) -> (
    Vec<RustType>,
    Vec<OperationInfo>,
    Vec<GenerationWarning>,
    TypeUsageRecorder,
  ) {
    let mut rust_types = vec![];
    let mut operations_info = vec![];
    let mut warnings = vec![];

    let operation_converter = OperationConverter::new(context.clone(), schema_converter.clone());

    for entry in self.operation_registry.operations() {
      match operation_converter.convert(entry) {
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
            method: entry.method.to_string(),
            path: entry.path.clone(),
            error: e.to_string(),
          });
        }
      }
    }

    (rust_types, operations_info, warnings, context.take_type_usage())
  }
}

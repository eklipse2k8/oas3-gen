use std::{
  collections::{BTreeSet, HashMap, HashSet},
  rc::Rc,
  sync::Arc,
};

use quote::ToTokens;
use strum::Display;

use super::converter::cache::SharedSchemaCache;
use crate::generator::{
  analyzer::TypeAnalyzer,
  ast::{ClientDef, LintConfig, OperationInfo, OperationKind, ParameterLocation, RustType, StructToken},
  codegen::{self, Visibility, client::ClientGenerator, mod_file::ModFileGenerator},
  converter::{
    CodegenConfig, ConverterContext, EnumCasePolicy, EnumDeserializePolicy, EnumHelperPolicy, ODataPolicy,
    SchemaConverter, TypeUsageRecorder, operations::OperationConverter,
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
    let artifacts = self.collect_generation_artifacts();
    let GenerationArtifacts {
      mut rust_types,
      mut operations_info,
      mut stats,
      usage_recorder,
    } = artifacts;

    let seed_map = usage_recorder.into_usage_map();
    let analyzer = TypeAnalyzer::new(&mut rust_types, &mut operations_info, seed_map);
    analyzer.analyze();

    stats.client_methods_generated = Some(operations_info.len());
    stats.client_headers_generated = Some(Self::count_unique_headers(&operations_info));

    let client = ClientDef::builder()
      .name(self.client_struct_name())
      .info(&self.spec.info)
      .servers(&self.spec.servers)
      .build();

    let client_generator = ClientGenerator::new(&client, &operations_info, &rust_types, self.visibility);
    let client_tokens = client_generator.into_token_stream();
    let lint_config = LintConfig::default();

    let final_code = codegen::generate_source(
      &client_tokens,
      &client,
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
    let client = ClientDef::builder()
      .name(self.client_struct_name())
      .info(&self.spec.info)
      .servers(&self.spec.servers)
      .build();

    let seed_map = usage_recorder.into_usage_map();
    let analyzer = TypeAnalyzer::new(&mut rust_types, &mut operations_info, seed_map);
    analyzer.analyze();

    let final_code = codegen::generate_file(
      &rust_types,
      self.visibility,
      &client,
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

    let client = ClientDef::builder()
      .name(self.client_struct_name())
      .info(&self.spec.info)
      .servers(&self.spec.servers)
      .build();

    let seed_map = usage_recorder.into_usage_map();
    let analyzer = TypeAnalyzer::new(&mut rust_types, &mut operations_info, seed_map);
    analyzer.analyze();

    let types_tokens = codegen::generate(&rust_types, self.visibility);
    let types_code = codegen::generate_source(&types_tokens, &client, None, source_path, OAS3_GEN_VERSION)?;

    stats.client_methods_generated = Some(operations_info.len());
    stats.client_headers_generated = Some(Self::count_unique_headers(&operations_info));

    let client_generator =
      ClientGenerator::new(&client, &operations_info, &rust_types, self.visibility).with_types_import();
    let client_tokens = client_generator.into_token_stream();
    let client_code = codegen::generate_source(&client_tokens, &client, None, source_path, OAS3_GEN_VERSION)?;

    let mod_generator = ModFileGenerator::new(&client, self.visibility);
    let mod_code = mod_generator.generate(source_path, OAS3_GEN_VERSION)?;

    Ok(ClientModOutput {
      types_code,
      client_code,
      mod_code,
      stats,
    })
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

  fn count_unique_headers(operations: &[OperationInfo]) -> usize {
    operations
      .iter()
      .flat_map(|op| &op.parameters)
      .filter(|param| matches!(param.parameter_location, Some(ParameterLocation::Header)))
      .filter_map(|param| param.original_name.as_deref())
      .map(str::to_ascii_lowercase)
      .collect::<BTreeSet<_>>()
      .len()
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
    let mut usage_recorder = TypeUsageRecorder::new();

    let operation_converter = OperationConverter::new(context.clone(), schema_converter);

    for entry in self.operation_registry.operations() {
      match operation_converter.convert(entry, &mut usage_recorder) {
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

    (rust_types, operations_info, warnings, usage_recorder)
  }
}

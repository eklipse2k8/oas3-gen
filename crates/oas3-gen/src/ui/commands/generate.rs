use std::{
  collections::{HashMap, HashSet},
  path::{Path, PathBuf},
};

use chrono::{Local, Timelike};
use crossterm::style::Stylize;

use crate::{
  generator::{
    ClientModMode, ClientMode, CodegenConfig, CollectionTypePolicy, EnumCasePolicy, EnumDeserializePolicy,
    EnumHelperPolicy, EnumLayoutPolicy, GenerationMode, GenerationTarget, HeaderScope, ODataPolicy, SchemaScope,
    ServerModMode, TypesMode,
    ast::documentation::init_doc_format,
    codegen::{CratePackage, GeneratedFileType, Visibility},
    metrics::GenerationStats,
    orchestrator::{GeneratedFinalOutput, Orchestrator},
  },
  ui::{Colors, EnumCaseMode, EnumLayout, GenerateCommand, GenerateMode},
  utils::spec::SpecLoader,
};

fn format_timestamp() -> String {
  let now = Local::now();
  format!("[{:02}:{:02}:{:02}]", now.hour(), now.minute(), now.second())
}

#[derive(Debug, Clone)]
#[allow(clippy::struct_excessive_bools)]
pub struct GenerateConfig {
  pub mode: GenerateMode,
  pub input: PathBuf,
  pub output: PathBuf,
  pub visibility: Visibility,
  pub verbose: bool,
  pub quiet: bool,
  pub all_schemas: bool,
  pub all_headers: bool,
  pub odata_support: bool,
  pub preserve_case_variants: bool,
  pub case_insensitive_enums: bool,
  pub enum_layout: EnumLayout,
  pub only_operations: Option<HashSet<String>>,
  pub excluded_operations: Option<HashSet<String>>,
  pub no_helpers: bool,
  pub enable_builders: bool,
  pub no_ordered_collections: bool,
  pub doc_format: bool,
  pub customizations: HashMap<String, String>,
  pub crate_package: Option<CratePackage>,
}

#[derive(Debug, Clone, Copy)]
struct EnumPolicies {
  preserve_case_variants: bool,
  case_insensitive_enums: bool,
}

impl GenerateConfig {
  async fn load_spec(&self) -> anyhow::Result<oas3::Spec> {
    SpecLoader::open(&self.input).await?.parse()
  }

  fn create_orchestrator(&self, spec: oas3::Spec) -> Orchestrator {
    let config = CodegenConfig::builder()
      .enum_case(if self.preserve_case_variants {
        EnumCasePolicy::Preserve
      } else {
        EnumCasePolicy::Deduplicate
      })
      .enum_helpers(if self.no_helpers {
        EnumHelperPolicy::Disable
      } else {
        EnumHelperPolicy::Generate
      })
      .enum_deserialize(if self.case_insensitive_enums {
        EnumDeserializePolicy::CaseInsensitive
      } else {
        EnumDeserializePolicy::CaseSensitive
      })
      .odata(if self.odata_support {
        ODataPolicy::Enabled
      } else {
        ODataPolicy::Disabled
      })
      .target(match self.mode {
        GenerateMode::ServerMod => GenerationTarget::Server,
        _ => GenerationTarget::Client,
      })
      .schema_scope(if self.all_schemas {
        SchemaScope::All
      } else {
        SchemaScope::ReferencedOnly
      })
      .header_scope(if self.all_headers {
        HeaderScope::All
      } else {
        HeaderScope::ReferencedOnly
      })
      .collection_types(if self.no_ordered_collections {
        CollectionTypePolicy::Hashed
      } else {
        CollectionTypePolicy::Ordered
      })
      .enum_layout(match self.enum_layout {
        EnumLayout::Spec => EnumLayoutPolicy::Spec,
        EnumLayout::Sorted => EnumLayoutPolicy::Sorted,
      })
      .enable_builders(self.enable_builders)
      .customizations(self.customizations.clone())
      .build();

    Orchestrator::new(
      spec,
      self.visibility,
      config,
      self.only_operations.as_ref(),
      self.excluded_operations.as_ref(),
    )
  }

  async fn write_output(&self, code: String) -> anyhow::Result<()> {
    if let Some(parent) = self.output.parent() {
      tokio::fs::create_dir_all(parent).await?;
    }
    tokio::fs::write(&self.output, code).await?;
    Ok(())
  }

  fn source_dir(&self) -> PathBuf {
    if self.crate_package.is_some() {
      self.output.join("src")
    } else {
      self.output.clone()
    }
  }

  fn root_file_name(&self) -> &'static str {
    if self.crate_package.is_some() {
      "lib.rs"
    } else {
      "mod.rs"
    }
  }

  async fn write_module_output(
    &self,
    output: &GeneratedFinalOutput,
    secondary: &GeneratedFileType,
    secondary_file_name: &str,
  ) -> anyhow::Result<()> {
    let source_dir = self.source_dir();
    tokio::fs::create_dir_all(&source_dir).await?;

    let types_code = output.code.code(&GeneratedFileType::Types).cloned().unwrap_or_default();
    let secondary_code = output.code.code(secondary).cloned().unwrap_or_default();
    let root_code = output
      .code
      .code(&GeneratedFileType::Module)
      .cloned()
      .unwrap_or_default();

    tokio::fs::write(source_dir.join("types.rs"), types_code).await?;
    tokio::fs::write(source_dir.join(secondary_file_name), secondary_code).await?;
    tokio::fs::write(source_dir.join(self.root_file_name()), root_code).await?;

    if let Some(manifest) = output.code.code(&GeneratedFileType::Manifest) {
      tokio::fs::write(self.output.join("Cargo.toml"), manifest).await?;
    }

    Ok(())
  }
}

impl GenerateConfig {
  pub fn from_command(command: GenerateCommand) -> anyhow::Result<Self> {
    let GenerateCommand {
      mode,
      input,
      output,
      visibility,
      odata_support,
      enum_mode,
      enum_layout,
      no_helpers,
      all_schemas,
      all_headers,
      enable_builders,
      no_ordered_collections,
      doc_format,
      workspace,
      module_version,
      only,
      exclude,
      verbose,
      quiet,
      customize,
    } = command;

    let output = match (&mode, output) {
      (GenerateMode::ClientMod | GenerateMode::ServerMod, None) => PathBuf::from("."),
      (_, None) => anyhow::bail!("Output path (-o) is required for types and client modes"),
      (_, Some(path)) => path,
    };
    let enum_policies = EnumPolicies::from(enum_mode);
    let customizations = parse_customizations(customize)?;
    let crate_package = resolve_crate_package(&mode, &output, workspace, module_version)?;

    Ok(Self {
      mode,
      input,
      output,
      visibility,
      verbose,
      quiet,
      all_schemas,
      all_headers,
      odata_support,
      preserve_case_variants: enum_policies.preserve_case_variants,
      case_insensitive_enums: enum_policies.case_insensitive_enums,
      enum_layout,
      only_operations: only.map(|ops| ops.into_iter().collect()),
      excluded_operations: exclude.map(|ops| ops.into_iter().collect()),
      no_helpers,
      enable_builders,
      no_ordered_collections,
      doc_format,
      customizations,
      crate_package,
    })
  }
}

pub(crate) fn resolve_crate_package(
  mode: &GenerateMode,
  output: &Path,
  workspace: bool,
  module_version: String,
) -> anyhow::Result<Option<CratePackage>> {
  if !workspace {
    return Ok(None);
  }

  anyhow::ensure!(
    matches!(mode, GenerateMode::ClientMod | GenerateMode::ServerMod),
    "The --workspace flag requires the client-mod or server-mod generation mode"
  );

  CratePackage::from_output_dir(output, module_version).map(Some)
}

pub(crate) fn parse_customizations(customize: Option<Vec<String>>) -> anyhow::Result<HashMap<String, String>> {
  let Some(entries) = customize else {
    return Ok(HashMap::new());
  };

  let mut map = HashMap::new();
  for entry in entries {
    let (key, value) = entry.split_once('=').ok_or_else(|| {
      anyhow::anyhow!("Invalid customize format '{entry}': expected TYPE=PATH (e.g., date_time=crate::MyDateTime)")
    })?;
    map.insert(key.to_string(), value.to_string());
  }
  Ok(map)
}

impl From<EnumCaseMode> for EnumPolicies {
  fn from(enum_mode: EnumCaseMode) -> Self {
    match enum_mode {
      EnumCaseMode::Merge => Self {
        preserve_case_variants: false,
        case_insensitive_enums: false,
      },
      EnumCaseMode::Preserve => Self {
        preserve_case_variants: true,
        case_insensitive_enums: false,
      },
      EnumCaseMode::Relaxed => Self {
        preserve_case_variants: false,
        case_insensitive_enums: true,
      },
    }
  }
}

struct GenerateLogger<'a> {
  config: &'a GenerateConfig,
  colors: &'a Colors,
}

impl<'a> GenerateLogger<'a> {
  fn new(config: &'a GenerateConfig, colors: &'a Colors) -> Self {
    Self { config, colors }
  }

  fn info(&self, message: &str) {
    if !self.config.quiet {
      println!("{} {message}", format_timestamp().with(self.colors.timestamp()));
    }
  }

  fn stat(&self, label: &str, value: String) {
    if !self.config.quiet {
      println!(
        "            {:<25} {}",
        label.with(self.colors.label()),
        value.with(self.colors.value())
      );
    }
  }

  fn log_loading(&self) {
    self.info(
      &format!("Loading OpenAPI spec from: {}", self.config.input.display())
        .with(self.colors.primary())
        .to_string(),
    );
  }

  fn log_generating(&self) {
    let message = match self.config.mode {
      GenerateMode::Types => "Generating Rust types...",
      GenerateMode::Client => "Generating Rust client...",
      GenerateMode::ClientMod => "Generating Rust client module...",
      GenerateMode::ServerMod => "Generating Rust server module...",
    };
    self.info(&message.with(self.colors.primary()).to_string());
  }

  fn print_statistics(&self, stats: &GenerationStats) {
    if self.config.quiet {
      return;
    }

    match self.config.mode {
      GenerateMode::Types => self.print_type_stats(stats),
      GenerateMode::Client => self.print_client_stats(stats),
      GenerateMode::ClientMod => {
        self.print_type_stats(stats);
        self.print_client_stats(stats);
      }
      GenerateMode::ServerMod => {
        self.print_type_stats(stats);
      }
    }

    self.print_common_stats(stats);
    self.print_cycles(stats);
    self.print_orphaned_schemas(stats);
    self.print_warnings(stats);
  }

  fn print_type_stats(&self, stats: &GenerationStats) {
    self.stat("Types generated:", stats.types_generated.to_string());
    self.stat("", format!("{} structs", stats.structs_generated));
    if stats.enums_with_helpers_generated > 0 {
      self.stat(
        "",
        format!(
          "{} enums, {} have helpers",
          stats.enums_generated, stats.enums_with_helpers_generated
        ),
      );
    } else {
      self.stat("", format!("{} enums", stats.enums_generated));
    }
    self.stat("", format!("{} type aliases", stats.type_aliases_generated));
    self.stat("Operations converted:", stats.operations_converted.to_string());
    if stats.webhooks_converted > 0 {
      self.stat("", format!("{} webhooks", stats.webhooks_converted));
    }
  }

  fn print_client_stats(&self, stats: &GenerationStats) {
    if stats.client_methods_generated > 0 {
      self.stat("Methods generated:", stats.client_methods_generated.to_string());
    }
    if stats.client_headers_generated > 0 {
      self.stat("Headers generated:", stats.client_headers_generated.to_string());
    }
  }

  fn print_common_stats(&self, stats: &GenerationStats) {
    if !stats.warnings.is_empty() {
      self.stat("Warnings:", stats.warnings.len().to_string());
    }
  }

  fn print_cycles(&self, stats: &GenerationStats) {
    if stats.cycles_detected == 0 {
      return;
    }

    self.stat("Cycles:", stats.cycles_detected.to_string());

    if self.config.verbose {
      for (i, cycle) in stats.cycle_details.iter().enumerate() {
        println!(
          "              {}: {}",
          format!("Cycle {}", i + 1).with(self.colors.accent()),
          cycle.join(" -> ").with(self.colors.info())
        );
      }
    }
  }

  fn print_orphaned_schemas(&self, stats: &GenerationStats) {
    if stats.orphaned_schemas_count > 0 && self.config.verbose {
      self.stat("Orphaned schemas:", stats.orphaned_schemas_count.to_string());
    }
  }

  fn print_warnings(&self, stats: &GenerationStats) {
    if stats.warnings.is_empty() || self.config.quiet {
      return;
    }

    let mut printed_header = false;
    for warning in &stats.warnings {
      let should_print = warning.is_skipped_item() || self.config.verbose;
      if !should_print {
        continue;
      }

      if !printed_header {
        println!();
        printed_header = true;
      }

      if warning.is_skipped_item() {
        eprintln!(
          "{} {}",
          "Skipped:".with(self.colors.accent()),
          format!("{warning}").with(self.colors.primary())
        );
      } else {
        eprintln!(
          "{} {}",
          "Warning:".with(self.colors.accent()),
          format!("{warning}").with(self.colors.primary())
        );
      }
    }
  }

  fn log_writing(&self) {
    self.info(
      &format!("Writing to: {}", self.config.output.display())
        .with(self.colors.primary())
        .to_string(),
    );

    if let Some(package) = &self.config.crate_package {
      self.stat("Crate package:", package.name().to_string());
    }
  }

  fn log_success(&self) {
    if !self.config.quiet {
      let message = match self.config.mode {
        GenerateMode::Types => "Successfully generated Rust types",
        GenerateMode::Client => "Successfully generated Rust client",
        GenerateMode::ClientMod => "Successfully generated Rust client module",
        GenerateMode::ServerMod => "Successfully generated Rust server module",
      };
      println!();
      println!(
        "{} {}",
        format_timestamp().with(self.colors.timestamp()),
        message.with(self.colors.success())
      );
    }
  }
}

pub async fn generate_code(config: GenerateConfig, colors: &Colors) -> anyhow::Result<()> {
  let logger = GenerateLogger::new(&config, colors);

  logger.log_loading();
  let spec = config.load_spec().await?;
  init_doc_format(config.doc_format);

  logger.log_generating();
  let orchestrator = config.create_orchestrator(spec);
  let source_path = config.input.display().to_string();

  let mode: Box<dyn GenerationMode> = match config.mode {
    GenerateMode::Types => Box::new(TypesMode),
    GenerateMode::Client => Box::new(ClientMode),
    GenerateMode::ClientMod => Box::new(ClientModMode::with_package(config.crate_package.clone())),
    GenerateMode::ServerMod => Box::new(ServerModMode::with_package(config.crate_package.clone())),
  };

  let output = orchestrator.generate(mode.as_ref(), &source_path)?;
  logger.print_statistics(&output.stats);
  logger.log_writing();

  match config.mode {
    GenerateMode::Types => {
      let code = output.code.code(&GeneratedFileType::Types).cloned().unwrap_or_default();
      config.write_output(code).await?;
    }
    GenerateMode::Client => {
      let code = output
        .code
        .code(&GeneratedFileType::Client)
        .cloned()
        .unwrap_or_default();
      config.write_output(code).await?;
    }
    GenerateMode::ClientMod => {
      config
        .write_module_output(&output, &GeneratedFileType::Client, "client.rs")
        .await?;
    }
    GenerateMode::ServerMod => {
      config
        .write_module_output(&output, &GeneratedFileType::Server, "server.rs")
        .await?;
    }
  }

  logger.log_success();
  Ok(())
}

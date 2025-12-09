use std::{collections::HashSet, path::PathBuf};

use chrono::{Local, Timelike};
use crossterm::style::Stylize;
use fmmap::tokio::{AsyncMmapFile, AsyncMmapFileExt};
use oas3::OpenApiV3Spec;

use crate::{
  generator::{
    codegen::Visibility,
    orchestrator::{GenerationStats, Orchestrator},
  },
  ui::{Colors, EnumCaseMode, GenerateMode},
};

fn format_timestamp() -> String {
  let now = Local::now();
  format!("[{:02}:{:02}:{:02}]", now.hour(), now.minute(), now.second())
}

pub struct GenerateConfig {
  pub mode: GenerateMode,
  pub input: PathBuf,
  pub output: PathBuf,
  pub visibility: Visibility,
  pub verbose: bool,
  pub quiet: bool,
  pub all_schemas: bool,
  pub odata_support: bool,
  pub preserve_case_variants: bool,
  pub case_insensitive_enums: bool,
  pub only_operations: Option<HashSet<String>>,
  pub excluded_operations: Option<HashSet<String>>,
  pub no_helpers: bool,
}

impl GenerateConfig {
  #[allow(clippy::too_many_arguments)]
  pub fn new(
    mode: GenerateMode,
    input: PathBuf,
    output: PathBuf,
    visibility: Visibility,
    verbose: bool,
    quiet: bool,
    all_schemas: bool,
    odata_support: bool,
    enum_mode: &EnumCaseMode,
    no_helpers: bool,
    only_operations: Option<Vec<String>>,
    excluded_operations: Option<Vec<String>>,
  ) -> Self {
    let only_operations = only_operations.map(|ops| ops.into_iter().collect());
    let excluded_operations = excluded_operations.map(|ops| ops.into_iter().collect());

    let (preserve_case_variants, case_insensitive_enums) = match enum_mode {
      EnumCaseMode::Merge => (false, false),
      EnumCaseMode::Preserve => (true, false),
      EnumCaseMode::Relaxed => (false, true),
    };

    Self {
      mode,
      input,
      output,
      visibility,
      verbose,
      quiet,
      all_schemas,
      odata_support,
      preserve_case_variants,
      case_insensitive_enums,
      only_operations,
      excluded_operations,
      no_helpers,
    }
  }

  async fn load_spec(&self) -> anyhow::Result<oas3::Spec> {
    let file = AsyncMmapFile::open(&self.input).await?;
    let spec = serde_json::from_slice::<OpenApiV3Spec>(file.as_slice())?;
    Ok(spec)
  }

  fn create_orchestrator(&self, spec: oas3::Spec) -> Orchestrator {
    Orchestrator::new(
      spec,
      self.visibility,
      self.all_schemas,
      self.only_operations.as_ref(),
      self.excluded_operations.as_ref(),
      self.odata_support,
      self.preserve_case_variants,
      self.case_insensitive_enums,
      self.no_helpers,
    )
  }

  async fn write_output(&self, code: String) -> anyhow::Result<()> {
    if let Some(parent) = self.output.parent() {
      tokio::fs::create_dir_all(parent).await?;
    }
    tokio::fs::write(&self.output, code).await?;
    Ok(())
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
  }

  fn print_client_stats(&self, stats: &GenerationStats) {
    if let Some(methods) = stats.client_methods_generated {
      self.stat("Methods generated:", methods.to_string());
    }
    if let Some(headers) = stats.client_headers_generated {
      self.stat("Headers generated:", headers.to_string());
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
    if stats.warnings.is_empty() || !self.config.verbose || self.config.quiet {
      return;
    }

    println!();
    for warning in &stats.warnings {
      eprintln!(
        "{} {}",
        "Warning:".with(self.colors.accent()),
        format!("{warning}").with(self.colors.primary())
      );
    }
  }

  fn log_writing(&self) {
    self.info(
      &format!("Writing to: {}", self.config.output.display())
        .with(self.colors.primary())
        .to_string(),
    );
  }

  fn log_success(&self) {
    if !self.config.quiet {
      let message = match self.config.mode {
        GenerateMode::Types => "Successfully generated Rust types",
        GenerateMode::Client => "Successfully generated Rust client",
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

  logger.log_generating();
  let orchestrator = config.create_orchestrator(spec);
  let source_path = config.input.display().to_string();
  let (code, stats) = match config.mode {
    GenerateMode::Types => orchestrator.generate_with_header(&source_path)?,
    GenerateMode::Client => orchestrator.generate_client_with_header(&source_path)?,
  };

  logger.print_statistics(&stats);

  logger.log_writing();
  config.write_output(code).await?;

  logger.log_success();

  Ok(())
}

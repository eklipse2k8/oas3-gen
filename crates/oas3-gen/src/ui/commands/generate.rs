use std::{collections::HashSet, path::PathBuf};

use chrono::{Local, Timelike};
use crossterm::style::Stylize;
use fmmap::tokio::{AsyncMmapFile, AsyncMmapFileExt};
use oas3::OpenApiV3Spec;

use crate::{
  generator::{
    codegen::Visibility,
    converter::FieldOptionalityPolicy,
    orchestrator::{GenerationStats, Orchestrator},
  },
  ui::{Colors, GenerateMode},
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
    preserve_case_variants: bool,
    case_insensitive_enums: bool,
    only_operations: Option<Vec<String>>,
    excluded_operations: Option<Vec<String>>,
  ) -> Self {
    let only_operations = only_operations.map(|ops| ops.into_iter().collect());
    let excluded_operations = excluded_operations.map(|ops| ops.into_iter().collect());
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
    }
  }

  async fn load_spec(&self) -> anyhow::Result<oas3::Spec> {
    let file = AsyncMmapFile::open(&self.input).await?;
    let spec = serde_json::from_slice::<OpenApiV3Spec>(file.as_slice())?;
    Ok(spec)
  }

  fn create_orchestrator(&self, spec: oas3::Spec) -> Orchestrator {
    let optionality_policy = if self.odata_support {
      FieldOptionalityPolicy::with_odata_support()
    } else {
      FieldOptionalityPolicy::standard()
    };

    Orchestrator::new(
      spec,
      self.visibility,
      self.all_schemas,
      self.only_operations.as_ref(),
      self.excluded_operations.as_ref(),
      optionality_policy,
      self.preserve_case_variants,
      self.case_insensitive_enums,
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

  fn log_loading(&self) {
    if !self.config.quiet {
      let timestamp = format_timestamp();
      println!(
        "{} {}",
        timestamp.with(self.colors.timestamp()),
        format!("Loading OpenAPI spec from: {}", self.config.input.display()).with(self.colors.primary())
      );
    }
  }

  fn log_generating(&self) {
    if !self.config.quiet {
      let timestamp = format_timestamp();
      let message = match self.config.mode {
        GenerateMode::Types => "Generating Rust types...".to_string(),
        GenerateMode::Client => "Generating Rust client...".to_string(),
      };
      println!(
        "{} {}",
        timestamp.with(self.colors.timestamp()),
        message.with(self.colors.primary())
      );
    }
  }

  fn print_statistics(&self, stats: &GenerationStats) {
    if self.config.quiet {
      return;
    }

    self.print_basic_stats(stats);
    self.print_cycles(stats);
    self.print_orphaned_schemas(stats);
  }

  fn print_basic_stats(&self, stats: &GenerationStats) {
    if let (Some(methods), Some(headers)) = (stats.client_methods_generated, stats.client_headers_generated) {
      println!(
        "            {:<25} {}",
        "Methods generated:".with(self.colors.label()),
        methods.to_string().with(self.colors.value())
      );
      println!(
        "            {:<25} {}",
        "Headers generated:".with(self.colors.label()),
        headers.to_string().with(self.colors.value())
      );
    } else {
      println!(
        "            {:<25} {}",
        "Types generated:".with(self.colors.label()),
        stats.types_generated.to_string().with(self.colors.value())
      );
      println!(
        "            {:<25}   {} structs",
        "".with(self.colors.label()),
        stats.structs_generated.to_string().with(self.colors.value())
      );
      println!(
        "            {:<25}   {} enums",
        "".with(self.colors.label()),
        stats.enums_generated.to_string().with(self.colors.value())
      );
      println!(
        "            {:<25}   {} type aliases",
        "".with(self.colors.label()),
        stats.type_aliases_generated.to_string().with(self.colors.value())
      );
      println!(
        "            {:<25} {}",
        "Operations converted:".with(self.colors.label()),
        stats.operations_converted.to_string().with(self.colors.value())
      );
    }

    if !stats.warnings.is_empty() {
      println!(
        "            {:<25} {}",
        "Warnings:".with(self.colors.label()),
        stats.warnings.len().to_string().with(self.colors.value())
      );
    }
  }

  fn print_cycles(&self, stats: &GenerationStats) {
    if stats.cycles_detected == 0 {
      return;
    }

    println!(
      "            {:<25} {}",
      "Cycles:".with(self.colors.label()),
      stats.cycles_detected.to_string().with(self.colors.value())
    );

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
      println!(
        "            {:<25} {}",
        "Orphaned schemas:".with(self.colors.label()),
        stats.orphaned_schemas_count.to_string().with(self.colors.value())
      );
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
    if !self.config.quiet {
      let timestamp = format_timestamp();
      println!(
        "{} {}",
        timestamp.with(self.colors.timestamp()),
        format!("Writing to: {}", self.config.output.display()).with(self.colors.primary())
      );
    }
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
  logger.print_warnings(&stats);

  logger.log_writing();
  config.write_output(code).await?;

  logger.log_success();

  Ok(())
}

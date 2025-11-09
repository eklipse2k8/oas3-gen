use std::{collections::HashSet, path::PathBuf};

use chrono::{Local, Timelike};
use crossterm::style::Stylize;

use crate::{
  generator::{
    codegen::Visibility,
    orchestrator::{GenerationStats, Orchestrator},
  },
  ui::Colors,
};

fn format_timestamp() -> String {
  let now = Local::now();
  format!("[{:02}:{:02}:{:02}]", now.hour(), now.minute(), now.second())
}

pub struct GenerateConfig {
  pub input: PathBuf,
  pub output: PathBuf,
  pub visibility: Visibility,
  pub verbose: bool,
  pub quiet: bool,
  pub all_schemas: bool,
  pub only_operations: Option<HashSet<String>>,
  pub excluded_operations: Option<HashSet<String>>,
}

impl GenerateConfig {
  #[allow(clippy::too_many_arguments)]
  pub fn new(
    input: PathBuf,
    output: PathBuf,
    visibility: &str,
    verbose: bool,
    quiet: bool,
    all_schemas: bool,
    only_operations: Option<Vec<String>>,
    excluded_operations: Option<Vec<String>>,
  ) -> Self {
    let visibility = Visibility::parse(visibility).unwrap_or(Visibility::Public);
    let only_operations = only_operations.map(|ops| ops.into_iter().collect());
    let excluded_operations = excluded_operations.map(|ops| ops.into_iter().collect());
    Self {
      input,
      output,
      visibility,
      verbose,
      quiet,
      all_schemas,
      only_operations,
      excluded_operations,
    }
  }

  async fn load_spec(&self) -> anyhow::Result<oas3::Spec> {
    let file_content = tokio::fs::read_to_string(&self.input).await?;
    Ok(oas3::from_json(file_content)?)
  }

  fn create_orchestrator(&self, spec: oas3::Spec) -> Orchestrator {
    Orchestrator::new(
      spec,
      self.visibility,
      self.all_schemas,
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
      println!(
        "{} {}",
        timestamp.with(self.colors.timestamp()),
        "Generating Rust types...".with(self.colors.primary())
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
      println!();
      println!(
        "{} {}",
        format_timestamp().with(self.colors.timestamp()),
        "Successfully generated Rust types".with(self.colors.success())
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
  let (code, stats) = orchestrator.generate_with_header(&config.input.display().to_string())?;

  logger.print_statistics(&stats);
  logger.print_warnings(&stats);

  logger.log_writing();
  config.write_output(code).await?;

  logger.log_success();

  Ok(())
}

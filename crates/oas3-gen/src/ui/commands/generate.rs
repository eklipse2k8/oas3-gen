use std::path::PathBuf;

use chrono::{Local, Timelike};
use crossterm::style::Stylize;

use crate::{
  generator::{codegen::Visibility, orchestrator::Orchestrator},
  ui::Colors,
};

fn format_timestamp() -> String {
  let now = Local::now();
  format!("[{:02}:{:02}:{:02}]", now.hour(), now.minute(), now.second())
}

pub async fn generate_code(
  input: PathBuf,
  output: PathBuf,
  visibility: String,
  verbose: bool,
  quiet: bool,
  all_schemas: bool,
  colors: &Colors,
) -> anyhow::Result<()> {
  let visibility = Visibility::parse(&visibility).unwrap_or(Visibility::Public);

  if !quiet {
    let timestamp = format_timestamp();
    println!(
      "{} {}",
      timestamp.with(colors.timestamp()),
      format!("Loading OpenAPI spec from: {}", input.display()).with(colors.primary())
    );
  }

  let file_content = tokio::fs::read_to_string(&input).await?;
  let spec = oas3::from_json(file_content)?;

  if !quiet {
    let timestamp = format_timestamp();
    println!(
      "{} {}",
      timestamp.with(colors.timestamp()),
      "Generating Rust types...".with(colors.primary())
    );
  }

  let orchestrator = Orchestrator::new(spec, visibility, all_schemas);
  let (code, stats) = orchestrator.generate_with_header(&input.display().to_string())?;

  if !quiet {
    println!(
      "            {:<25} {}",
      "Types generated:".with(colors.label()),
      stats.types_generated.to_string().with(colors.value())
    );
    println!(
      "            {:<25}   {} structs",
      "".with(colors.label()),
      stats.structs_generated.to_string().with(colors.value())
    );
    println!(
      "            {:<25}   {} enums",
      "".with(colors.label()),
      stats.enums_generated.to_string().with(colors.value())
    );
    println!(
      "            {:<25}   {} type aliases",
      "".with(colors.label()),
      stats.type_aliases_generated.to_string().with(colors.value())
    );
    println!(
      "            {:<25} {}",
      "Operations converted:".with(colors.label()),
      stats.operations_converted.to_string().with(colors.value())
    );

    if !stats.warnings.is_empty() {
      println!(
        "            {:<25} {}",
        "Warnings:".with(colors.label()),
        stats.warnings.len().to_string().with(colors.value())
      );
    }

    if stats.cycles_detected > 0 {
      println!(
        "            {:<25} {}",
        "Cycles:".with(colors.label()),
        stats.cycles_detected.to_string().with(colors.value())
      );

      if verbose {
        for (i, cycle) in stats.cycle_details.iter().enumerate() {
          println!(
            "              {}: {}",
            format!("Cycle {}", i + 1).with(colors.accent()),
            cycle.join(" -> ").with(colors.info())
          );
        }
      }
    }

    if stats.orphaned_schemas_count > 0 && verbose {
      println!(
        "            {:<25} {}",
        "Orphaned schemas:".with(colors.label()),
        stats.orphaned_schemas_count.to_string().with(colors.value())
      );
    }
  }

  if !stats.warnings.is_empty() && verbose && !quiet {
    println!();
    for warning in &stats.warnings {
      eprintln!(
        "{} {}",
        "Warning:".with(colors.accent()),
        format!("{warning}").with(colors.primary())
      );
    }
  }

  if !quiet {
    let timestamp = format_timestamp();
    println!(
      "{} {}",
      timestamp.with(colors.timestamp()),
      format!("Writing to: {}", output.display()).with(colors.primary())
    );
  }

  if let Some(parent) = output.parent() {
    tokio::fs::create_dir_all(parent).await?;
  }
  tokio::fs::write(&output, code).await?;

  if !quiet {
    println!();
    println!(
      "{} {}",
      format_timestamp().with(colors.timestamp()),
      "Successfully generated Rust types".with(colors.success())
    );
  }

  Ok(())
}

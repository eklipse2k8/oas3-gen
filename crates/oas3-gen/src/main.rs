use std::path::PathBuf;

use clap::Parser;

use crate::generator::{code_generator::Visibility, orchestrator::Orchestrator};

mod generator;
mod reserved;

/// OpenAPI to Rust code generator
///
/// Generates Rust type definitions from OpenAPI 3.x specifications with validation,
/// serde serialization, and comprehensive documentation.
#[derive(Parser, Debug)]
#[command(name = "openapi-gen")]
#[command(author, version, about, long_about = None)]
struct Cli {
  /// Path to the OpenAPI JSON specification file
  #[arg(short, long, value_name = "FILE")]
  input: PathBuf,

  /// Path where the generated Rust code will be written
  #[arg(short, long, value_name = "FILE")]
  output: PathBuf,

  /// Visibility level for generated types
  #[arg(long, value_name = "VISIBILITY", default_value = "public")]
  visibility: String,

  /// Enable verbose output with detailed progress information
  #[arg(short, long, default_value_t = false)]
  verbose: bool,

  /// Suppress non-essential output (errors only)
  #[arg(short, long, default_value_t = false)]
  quiet: bool,
}

macro_rules! log_info {
  ($cli:expr, $($arg:tt)*) => {
    if !$cli.quiet {
      println!($($arg)*);
    }
  };
}

macro_rules! log_verbose {
  ($cli:expr, $($arg:tt)*) => {
    if $cli.verbose && !$cli.quiet {
      println!($($arg)*);
    }
  };
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
  let cli = Cli::parse();

  // Parse visibility argument
  let visibility = Visibility::parse(&cli.visibility).unwrap_or(Visibility::Public);

  // Load and parse OpenAPI specification
  log_info!(cli, "Loading OpenAPI spec from: {}", cli.input.display());
  let file_content = tokio::fs::read_to_string(&cli.input).await?;
  let spec = oas3::from_json(file_content)?;

  // Create orchestrator and generate code
  log_info!(cli, "Generating Rust types...");
  let orchestrator = Orchestrator::new(spec, visibility)?;
  let (code, stats) = orchestrator.generate_with_header(&cli.input.display().to_string())?;

  // Report statistics
  log_verbose!(cli, "  Types generated: {}", stats.types_generated);
  log_verbose!(cli, "  Operations converted: {}", stats.operations_converted);

  if stats.cycles_detected > 0 {
    log_info!(cli, "  Found {} cycles", stats.cycles_detected);
    for (i, cycle) in stats.cycle_details.iter().enumerate() {
      log_verbose!(cli, "    Cycle {}: {}", i + 1, cycle.join(" -> "));
    }
  } else {
    log_verbose!(cli, "  No cycles detected");
  }

  // Report warnings
  if !stats.warnings.is_empty() {
    log_verbose!(cli, "");
    for warning in &stats.warnings {
      eprintln!("Warning: {}", warning);
    }
  }

  // Write output file
  log_info!(cli, "\nWriting to: {}", cli.output.display());
  if let Some(parent) = cli.output.parent() {
    tokio::fs::create_dir_all(parent).await?;
  }
  tokio::fs::write(&cli.output, code).await?;

  // Report success
  log_info!(cli, "\n✓ Successfully generated Rust types!");
  log_info!(cli, "  Input:  {}", cli.input.display());
  log_info!(cli, "  Output: {}", cli.output.display());
  log_info!(cli, "  Types:  {}", stats.types_generated);

  if stats.cycles_detected > 0 {
    log_info!(cli, "  Cycles: {}", stats.cycles_detected);
  }

  Ok(())
}

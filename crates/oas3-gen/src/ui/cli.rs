use std::path::PathBuf;

use clap::{Parser, Subcommand, ValueEnum};

use super::colors::{ColorMode, ThemeMode};

#[derive(Parser, Debug)]
#[command(name = "oas3-gen")]
#[command(author, version, about = "OpenAPI to Rust code generator")]
pub struct Cli {
  #[command(subcommand)]
  pub command: Commands,

  /// Control color output
  #[arg(long, value_enum, default_value = "auto", global = true)]
  pub color: ColorMode,

  /// Terminal theme (dark or light background)
  #[arg(long, value_enum, default_value = "auto", global = true)]
  pub theme: ThemeMode,
}

#[derive(Subcommand, Debug)]
pub enum Commands {
  /// List information from OpenAPI specification
  List {
    #[command(subcommand)]
    list_command: ListCommands,
  },
  /// Generate Rust code from OpenAPI specification
  Generate {
    /// Generation mode (types or client)
    #[arg(short, long, value_enum, default_value = "types")]
    mode: GenerateMode,

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

    /// Generate all schemas defined in the spec, including unreferenced schemas.
    /// When combined with --only or --exclude, this includes all schemas even if they
    /// are not referenced by the filtered operations (default: only schemas reachable
    /// from included operations)
    #[arg(long, default_value_t = false)]
    all_schemas: bool,

    /// Include only specific operations for generation (comma-separated stable IDs)
    #[arg(long, value_name = "IDS", value_delimiter = ',')]
    only: Option<Vec<String>>,

    /// Exclude specific operations from generation (comma-separated stable IDs)
    #[arg(long, value_name = "IDS", value_delimiter = ',')]
    exclude: Option<Vec<String>>,
  },
}

#[derive(ValueEnum, Clone, Debug)]
pub enum GenerateMode {
  Types,
  Client,
}

#[derive(Subcommand, Debug)]
pub enum ListCommands {
  /// List all operations defined in the OpenAPI specification
  Operations {
    /// Path to the OpenAPI JSON specification file
    #[arg(short, long, value_name = "FILE")]
    input: PathBuf,
  },
}

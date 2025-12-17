use std::path::PathBuf;

use clap::{ArgAction, Args, Parser, Subcommand, ValueEnum, ValueHint, builder::Styles};

use super::colors::{ColorMode, ThemeMode};
use crate::{generator::codegen::Visibility, ui::Colors};

const DARK_STYLE: Styles = Colors::clap_styles();

#[derive(Parser, Debug)]
#[command(name = "oas3-gen")]
#[command(author, version, about = "OpenAPI to Rust code generator")]
#[command(propagate_version = true)]
#[command(styles = DARK_STYLE)]
pub struct Cli {
  #[command(subcommand)]
  pub command: Commands,

  /// Coloring
  #[arg(
    long,
    value_enum,
    value_name = "WHEN",
    default_value = "auto",
    global = true,
    display_order = 100,
    help_heading = "Terminal Output"
  )]
  pub color: ColorMode,

  /// Theme
  #[arg(
    long,
    value_enum,
    value_name = "THEME",
    default_value = "auto",
    global = true,
    display_order = 100,
    help_heading = "Terminal Output"
  )]
  pub theme: ThemeMode,
}

#[derive(Subcommand, Debug)]
pub enum Commands {
  /// List information from OpenAPI specification
  List {
    #[command(subcommand)]
    list_command: ListCommands,
  },
  /// Generates idiomatic, type-safe Rust code from an OpenAPI v3.1 (OAS31) specification.
  Generate(GenerateCommand),
}

#[derive(Args, Debug, Clone)]
#[allow(clippy::struct_excessive_bools)]
pub struct GenerateCommand {
  /// Sets the generation mode
  #[arg(value_enum, default_value = "types")]
  pub mode: GenerateMode,

  /// Path to the OpenAPI specification file
  #[arg(
    short,
    long,
    value_name = "FILE",
    value_hint = ValueHint::AnyPath,
    display_order = 0,
    help_heading = "Required"
  )]
  pub input: PathBuf,

  /// Path for the generated rust output file
  #[arg(
    short,
    long,
    value_name = "FILE",
    value_hint = ValueHint::AnyPath,
    display_order = 0,
    help_heading = "Required"
  )]
  pub output: PathBuf,

  /// Module visibility for generated items
  #[arg(
    short = 'C',
    long,
    value_name = "PUB",
    default_value = "public",
    display_order = 10,
    help_heading = "Code Generation"
  )]
  pub visibility: Visibility,

  /// Enable OData-specific field optionality rules (makes @odata.* fields optional on concrete types)
  #[arg(long, default_value_t = false, display_order = 11, help_heading = "Code Generation")]
  pub odata_support: bool,

  /// Specifies how to handle enum case sensitivity and duplicates
  #[arg(
    long,
    value_enum,
    default_value_t,
    display_order = 12,
    help_heading = "Code Generation"
  )]
  pub enum_mode: EnumCaseMode,

  /// Disable generation of ergonomic helper methods for enum variants
  #[arg(long, default_value_t = false, display_order = 13, help_heading = "Code Generation")]
  pub no_helpers: bool,

  /// Generate all schemas, even those unreferenced by selected operations
  #[arg(
    group = "filter",
    long,
    default_value_t = false,
    display_order = 22,
    help_heading = "Operation Filtering"
  )]
  pub all_schemas: bool,

  /// Include only the specified comma-separated operation IDs
  #[arg(
    group = "filter",
    long, action = ArgAction::Append,
    value_name = "id_1,id_2,...",
    value_delimiter = ',',
    display_order = 20,
    help_heading = "Operation Filtering"
  )]
  pub only: Option<Vec<String>>,

  /// Exclude the specified comma-separated operation IDs
  #[arg(
    group = "filter",
    long, action = ArgAction::Append,
    value_name = "id_1,id_2,...",
    value_delimiter = ',',
    display_order = 21,
    help_heading = "Operation Filtering"
  )]
  pub exclude: Option<Vec<String>>,

  /// Enable verbose output with detailed progress information
  #[arg(
    short,
    long,
    default_value_t = false,
    global = true,
    display_order = 100,
    help_heading = "Terminal Output"
  )]
  pub verbose: bool,

  /// Suppress non-essential output (errors only)
  #[arg(
    short,
    long,
    default_value_t = false,
    global = true,
    display_order = 101,
    help_heading = "Terminal Output"
  )]
  pub quiet: bool,
}

#[derive(ValueEnum, Clone, Debug)]
pub enum GenerateMode {
  Types,
  Client,
}

#[derive(ValueEnum, Clone, Debug, Default)]
pub enum EnumCaseMode {
  #[default]
  /// Merge duplicates with strict matching (e.g., "ITEM" and "item" become "Item")
  Merge,
  /// Preserve case-variant duplicates as separate enum values
  Preserve,
  /// Merge duplicates and enable relaxed (case-insensitive) deserialization
  Relaxed,
}

#[derive(Subcommand, Debug)]
pub enum ListCommands {
  /// List all operations defined in the OpenAPI specification
  Operations {
    /// Path to the OpenAPI JSON specification file
    #[arg(short, long, value_name = "FILE", value_hint = ValueHint::AnyPath)]
    input: PathBuf,
  },
}

#![allow(clippy::doc_markdown)]
use std::{io::IsTerminal, path::PathBuf};

use chrono::{Local, Timelike};
use clap::{Parser, ValueEnum};
use crossterm::style::{Color, Stylize};

use crate::generator::{codegen::Visibility, orchestrator::Orchestrator};

mod generator;
mod reserved;

#[macro_use(cfg_if)]
extern crate cfg_if;

#[derive(Debug, Clone, Copy, ValueEnum)]
enum ColorMode {
  Always,
  Auto,
  Never,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum ThemeMode {
  Dark,
  Light,
  Auto,
}

/// `OpenAPI` to Rust code generator
///
/// Generates Rust type definitions from `OpenAPI` 3.x specifications with validation,
/// serde serialization, and comprehensive documentation.
#[derive(Parser, Debug)]
#[command(name = "openapi-gen")]
#[command(author, version, about, long_about = None)]
struct Cli {
  /// Path to the `OpenAPI` JSON specification file
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

  /// Control color output
  #[arg(long, value_enum, default_value = "auto")]
  color: ColorMode,

  /// Terminal theme (dark or light background)
  #[arg(long, value_enum, default_value = "auto")]
  theme: ThemeMode,
}

impl Cli {
  fn colors_enabled(&self) -> bool {
    match self.color {
      ColorMode::Always => true,
      ColorMode::Never => false,
      ColorMode::Auto => std::io::stdout().is_terminal(),
    }
  }

  fn detect_theme(&self) -> Theme {
    match self.theme {
      ThemeMode::Dark => Theme::Dark,
      ThemeMode::Light => Theme::Light,
      ThemeMode::Auto => Self::detect_terminal_theme(),
    }
  }

  fn detect_terminal_theme() -> Theme {
    if let Ok(colorfgbg) = std::env::var("COLORFGBG")
      && let Some(bg) = colorfgbg.split(';').next_back()
      && let Ok(bg_num) = bg.parse::<u8>()
    {
      return if bg_num >= 8 { Theme::Light } else { Theme::Dark };
    }

    if let Ok(term_program) = std::env::var("TERM_PROGRAM")
      && (term_program == "Apple_Terminal" || term_program == "iTerm.app")
      && let Ok(theme) = std::env::var("ITERM_PROFILE")
      && theme.to_lowercase().contains("light")
    {
      return Theme::Light;
    }

    Theme::Dark
  }
}

enum Theme {
  Dark,
  Light,
}

struct Colors {
  enabled: bool,
  theme: Theme,
}

impl Colors {
  fn new(enabled: bool, theme: Theme) -> Self {
    Self { enabled, theme }
  }

  fn timestamp(&self) -> Color {
    if !self.enabled {
      return Color::Reset;
    }

    match self.theme {
      Theme::Dark => Color::Rgb { r: 118, g: 166, b: 166 },
      Theme::Light => Color::Rgb { r: 92, g: 62, b: 38 },
    }
  }

  fn primary(&self) -> Color {
    if !self.enabled {
      return Color::Reset;
    }

    match self.theme {
      Theme::Dark => Color::Rgb { r: 191, g: 126, b: 4 },
      Theme::Light => Color::Rgb { r: 70, g: 42, b: 25 },
    }
  }

  fn accent(&self) -> Color {
    if !self.enabled {
      return Color::Reset;
    }

    match self.theme {
      Theme::Dark => Color::Rgb { r: 166, g: 84, b: 55 },
      Theme::Light => Color::Rgb { r: 211, g: 99, b: 70 },
    }
  }

  fn info(&self) -> Color {
    if !self.enabled {
      return Color::Reset;
    }

    match self.theme {
      Theme::Dark => Color::Rgb { r: 118, g: 166, b: 166 },
      Theme::Light => Color::Rgb { r: 40, g: 111, b: 170 },
    }
  }

  fn success(&self) -> Color {
    if !self.enabled {
      return Color::Reset;
    }

    match self.theme {
      Theme::Dark => Color::Rgb { r: 118, g: 166, b: 166 },
      Theme::Light => Color::Rgb { r: 34, g: 142, b: 90 },
    }
  }

  fn label(&self) -> Color {
    if !self.enabled {
      return Color::Reset;
    }

    match self.theme {
      Theme::Dark => Color::Rgb { r: 217, g: 164, b: 4 },
      Theme::Light => Color::Rgb { r: 176, g: 103, b: 66 },
    }
  }

  fn value(&self) -> Color {
    if !self.enabled {
      return Color::Reset;
    }

    match self.theme {
      Theme::Dark => Color::Rgb { r: 242, g: 211, b: 56 },
      Theme::Light => Color::Rgb { r: 199, g: 146, b: 76 },
    }
  }
}

fn format_timestamp() -> String {
  let now = Local::now();
  format!("[{:02}:{:02}:{:02}]", now.hour(), now.minute(), now.second())
}

macro_rules! log_info {
  ($cli:expr, $colors:expr, $($arg:tt)*) => {
    if !$cli.quiet {
      let timestamp = format_timestamp();
      println!("{} {}", timestamp.with($colors.timestamp()), format!($($arg)*).with($colors.primary()));
    }
  };
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
  let cli = Cli::parse();
  let colors = Colors::new(cli.colors_enabled(), cli.detect_theme());

  let visibility = Visibility::parse(&cli.visibility).unwrap_or(Visibility::Public);

  log_info!(cli, colors, "Loading OpenAPI spec from: {}", cli.input.display());
  let file_content = tokio::fs::read_to_string(&cli.input).await?;
  let spec = oas3::from_json(file_content)?;

  log_info!(cli, colors, "Generating Rust types...");
  let orchestrator = Orchestrator::new(spec, visibility);
  let (code, stats) = orchestrator.generate_with_header(&cli.input.display().to_string())?;

  if !cli.quiet {
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

      if cli.verbose {
        for (i, cycle) in stats.cycle_details.iter().enumerate() {
          println!(
            "              {}: {}",
            format!("Cycle {}", i + 1).with(colors.accent()),
            cycle.join(" -> ").with(colors.info())
          );
        }
      }
    }
  }

  if !stats.warnings.is_empty() && cli.verbose && !cli.quiet {
    println!();
    for warning in &stats.warnings {
      eprintln!(
        "{} {}",
        "Warning:".with(colors.accent()),
        format!("{warning}").with(colors.primary())
      );
    }
  }

  log_info!(cli, colors, "Writing to: {}", cli.output.display());
  if let Some(parent) = cli.output.parent() {
    tokio::fs::create_dir_all(parent).await?;
  }
  tokio::fs::write(&cli.output, code).await?;

  if !cli.quiet {
    println!();
    println!(
      "{} {}",
      format_timestamp().with(colors.timestamp()),
      "Successfully generated Rust types".with(colors.success())
    );
  }

  Ok(())
}

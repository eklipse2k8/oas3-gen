#![allow(clippy::doc_markdown)]
use clap::Parser;

use crate::ui::{Cli, Colors, Commands, ListCommands, colors};

mod generator;
mod reserved;
mod ui;

#[macro_use(cfg_if)]
extern crate cfg_if;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
  let cli = Cli::parse();
  let colors = Colors::new(colors::colors_enabled(cli.color), colors::detect_theme(cli.theme));

  match cli.command {
    Commands::List { list_command } => match list_command {
      ListCommands::Operations { input } => ui::commands::list_operations(&input, &colors).await?,
    },
    Commands::Generate {
      input,
      output,
      visibility,
      verbose,
      quiet,
      all_schemas,
    } => {
      ui::commands::generate_code(input, output, visibility, verbose, quiet, all_schemas, &colors).await?;
    }
  }

  Ok(())
}

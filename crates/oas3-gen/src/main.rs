#![allow(clippy::doc_markdown)]
#![allow(clippy::missing_errors_doc)]
#![allow(clippy::too_many_lines)]
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
      mode,
      input,
      output,
      visibility,
      odata_support,
      enum_mode,
      verbose,
      quiet,
      all_schemas,
      only,
      exclude,
    } => {
      let config = ui::commands::GenerateConfig::new(
        mode,
        input,
        output,
        visibility,
        verbose,
        quiet,
        all_schemas,
        odata_support,
        &enum_mode,
        only,
        exclude,
      );
      ui::commands::generate_code(config, &colors).await?;
    }
  }

  Ok(())
}

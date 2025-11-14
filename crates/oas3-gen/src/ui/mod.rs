pub mod cli;
pub mod colors;
pub mod commands;

pub use cli::{Cli, Commands, GenerateMode, ListCommands};
pub use colors::Colors;

fn term_width() -> u16 {
  if let Ok((width, _)) = crossterm::terminal::size() {
    width
  } else {
    80
  }
}

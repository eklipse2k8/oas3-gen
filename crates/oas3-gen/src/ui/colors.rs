use std::io::IsTerminal;

use clap::{ValueEnum, builder::styling::Ansi256Color};
use comfy_table::Color as ComfyColor;
use crossterm::style::Color;

#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum ColorMode {
  Always,
  Auto,
  Never,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum ThemeMode {
  Dark,
  Light,
  Auto,
}

pub enum Theme {
  Dark,
  Light,
}

pub struct Colors {
  enabled: bool,
  theme: Theme,
}

pub trait IntoComfyColor {
  fn into(self) -> ComfyColor;
}

impl IntoComfyColor for Color {
  fn into(self) -> ComfyColor {
    match self {
      Color::Reset => ComfyColor::Reset,
      Color::Black => ComfyColor::Black,
      Color::DarkGrey => ComfyColor::DarkGrey,
      Color::Red => ComfyColor::Red,
      Color::DarkRed => ComfyColor::DarkRed,
      Color::Green => ComfyColor::Green,
      Color::DarkGreen => ComfyColor::DarkGreen,
      Color::Yellow => ComfyColor::Yellow,
      Color::DarkYellow => ComfyColor::DarkYellow,
      Color::Blue => ComfyColor::Blue,
      Color::DarkBlue => ComfyColor::DarkBlue,
      Color::Magenta => ComfyColor::Magenta,
      Color::DarkMagenta => ComfyColor::DarkMagenta,
      Color::Cyan => ComfyColor::Cyan,
      Color::DarkCyan => ComfyColor::DarkCyan,
      Color::White => ComfyColor::White,
      Color::Grey => ComfyColor::Grey,
      Color::Rgb { r, g, b } => ComfyColor::Rgb { r, g, b },
      Color::AnsiValue(val) => ComfyColor::AnsiValue(val),
    }
  }
}

impl Colors {
  pub const fn new(enabled: bool, theme: Theme) -> Self {
    Self { enabled, theme }
  }

  pub const fn timestamp(&self) -> Color {
    if !self.enabled {
      return Color::Reset;
    }

    match self.theme {
      Theme::Dark => Color::Rgb { r: 118, g: 166, b: 166 },
      Theme::Light => Color::Rgb { r: 92, g: 62, b: 38 },
    }
  }

  pub const fn primary(&self) -> Color {
    if !self.enabled {
      return Color::Reset;
    }

    match self.theme {
      Theme::Dark => Color::Rgb { r: 191, g: 126, b: 4 },
      Theme::Light => Color::Rgb { r: 70, g: 42, b: 25 },
    }
  }

  pub const fn accent(&self) -> Color {
    if !self.enabled {
      return Color::Reset;
    }

    match self.theme {
      Theme::Dark => Color::Rgb { r: 166, g: 84, b: 55 },
      Theme::Light => Color::Rgb { r: 211, g: 99, b: 70 },
    }
  }

  pub const fn info(&self) -> Color {
    if !self.enabled {
      return Color::Reset;
    }

    match self.theme {
      Theme::Dark => Color::Rgb { r: 118, g: 166, b: 166 },
      Theme::Light => Color::Rgb { r: 40, g: 111, b: 170 },
    }
  }

  pub const fn success(&self) -> Color {
    if !self.enabled {
      return Color::Reset;
    }

    match self.theme {
      Theme::Dark => Color::Rgb { r: 118, g: 166, b: 166 },
      Theme::Light => Color::Rgb { r: 34, g: 142, b: 90 },
    }
  }

  pub const fn label(&self) -> Color {
    if !self.enabled {
      return Color::Reset;
    }

    match self.theme {
      Theme::Dark => Color::Rgb { r: 217, g: 164, b: 4 },
      Theme::Light => Color::Rgb { r: 176, g: 103, b: 66 },
    }
  }

  pub const fn value(&self) -> Color {
    if !self.enabled {
      return Color::Reset;
    }

    match self.theme {
      Theme::Dark => Color::Rgb { r: 242, g: 211, b: 56 },
      Theme::Light => Color::Rgb { r: 199, g: 146, b: 76 },
    }
  }

  const fn to_clap(color: Color) -> Option<clap::builder::styling::Color> {
    use clap::builder::styling::{AnsiColor, Color as ClapColor, RgbColor};

    match color {
      Color::Black => Some(ClapColor::Ansi(AnsiColor::Black)),
      Color::Blue | Color::DarkBlue => Some(ClapColor::Ansi(AnsiColor::Blue)),
      Color::Cyan | Color::DarkCyan => Some(ClapColor::Ansi(AnsiColor::Cyan)),
      Color::DarkGreen | Color::Green => Some(ClapColor::Ansi(AnsiColor::Green)),
      Color::DarkGrey | Color::Grey => Some(ClapColor::Ansi(AnsiColor::BrightBlack)),
      Color::DarkMagenta | Color::Magenta => Some(ClapColor::Ansi(AnsiColor::Magenta)),
      Color::DarkRed | Color::Red => Some(ClapColor::Ansi(AnsiColor::Red)),
      Color::DarkYellow | Color::Yellow => Some(ClapColor::Ansi(AnsiColor::Yellow)),
      Color::White => Some(ClapColor::Ansi(AnsiColor::White)),
      Color::AnsiValue(val) => Some(ClapColor::Ansi256(Ansi256Color(val))),
      Color::Rgb { r, g, b } => Some(ClapColor::Rgb(RgbColor(r, g, b))),
      Color::Reset => None,
    }
  }

  pub const fn clap_styles() -> clap::builder::Styles {
    use clap::builder::styling::{Style, Styles};

    let colors = Self::new(true, Theme::Dark);

    Styles::styled()
      .header(Style::new().bold().underline().fg_color(Self::to_clap(colors.label())))
      .usage(Style::new().bold().fg_color(Self::to_clap(colors.label())))
      .literal(Style::new().fg_color(Self::to_clap(colors.success())))
      .placeholder(Style::new().fg_color(Self::to_clap(colors.info())))
      .error(Style::new().bold().fg_color(Self::to_clap(colors.accent())))
      .valid(Style::new().fg_color(Self::to_clap(colors.success())))
      .invalid(Style::new().bold().fg_color(Self::to_clap(colors.accent())))
  }
}

pub fn colors_enabled(mode: ColorMode) -> bool {
  match mode {
    ColorMode::Always => true,
    ColorMode::Never => false,
    ColorMode::Auto => std::io::stdout().is_terminal(),
  }
}

pub fn detect_theme(mode: ThemeMode) -> Theme {
  match mode {
    ThemeMode::Dark => Theme::Dark,
    ThemeMode::Light => Theme::Light,
    ThemeMode::Auto => detect_terminal_theme(),
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

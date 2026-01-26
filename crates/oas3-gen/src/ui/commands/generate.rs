use std::{
  collections::{HashMap, HashSet},
  path::PathBuf,
};

use chrono::{Local, Timelike};
use crossterm::style::Stylize;

use crate::{
  generator::{
    codegen::{GeneratedFileType, Visibility},
    converter::GenerationTarget,
    orchestrator::{GeneratedFinalOutput, GenerationStats, Orchestrator},
  },
  ui::{Colors, EnumCaseMode, GenerateCommand, GenerateMode},
  utils::spec::SpecLoader,
};

fn format_timestamp() -> String {
  let now = Local::now();
  format!("[{:02}:{:02}:{:02}]", now.hour(), now.minute(), now.second())
}

#[derive(Debug, Clone)]
#[allow(clippy::struct_excessive_bools)]
pub struct GenerateConfig {
  pub mode: GenerateMode,
  pub input: PathBuf,
  pub output: PathBuf,
  pub visibility: Visibility,
  pub verbose: bool,
  pub quiet: bool,
  pub all_schemas: bool,
  pub odata_support: bool,
  pub preserve_case_variants: bool,
  pub case_insensitive_enums: bool,
  pub only_operations: Option<HashSet<String>>,
  pub excluded_operations: Option<HashSet<String>>,
  pub no_helpers: bool,
  pub customizations: HashMap<String, String>,
}

#[derive(Debug, Clone, Copy)]
struct EnumPolicies {
  preserve_case_variants: bool,
  case_insensitive_enums: bool,
}

impl GenerateConfig {
  async fn load_spec(&self) -> anyhow::Result<oas3::Spec> {
    SpecLoader::open(&self.input).await?.parse()
  }

  fn create_orchestrator(&self, spec: oas3::Spec) -> Orchestrator {
    let generation_target = match self.mode {
      GenerateMode::ServerMod => GenerationTarget::Server,
      _ => GenerationTarget::Client,
    };

    Orchestrator::new(
      spec,
      self.visibility,
      self.all_schemas,
      self.only_operations.as_ref(),
      self.excluded_operations.as_ref(),
      self.odata_support,
      self.preserve_case_variants,
      self.case_insensitive_enums,
      self.no_helpers,
      generation_target,
      self.customizations.clone(),
    )
  }

  async fn write_output(&self, code: String) -> anyhow::Result<()> {
    if let Some(parent) = self.output.parent() {
      tokio::fs::create_dir_all(parent).await?;
    }
    tokio::fs::write(&self.output, code).await?;
    Ok(())
  }

  async fn write_module_output(&self, output: &GeneratedFinalOutput) -> anyhow::Result<()> {
    tokio::fs::create_dir_all(&self.output).await?;
    let types_code = output.code.code(&GeneratedFileType::Types).cloned().unwrap_or_default();
    let client_code = output
      .code
      .code(&GeneratedFileType::Client)
      .cloned()
      .unwrap_or_default();
    let mod_code = output
      .code
      .code(&GeneratedFileType::Module)
      .cloned()
      .unwrap_or_default();
    tokio::fs::write(self.output.join("types.rs"), types_code).await?;
    tokio::fs::write(self.output.join("client.rs"), client_code).await?;
    tokio::fs::write(self.output.join("mod.rs"), mod_code).await?;
    Ok(())
  }

  async fn write_server_module_output(&self, output: &GeneratedFinalOutput) -> anyhow::Result<()> {
    tokio::fs::create_dir_all(&self.output).await?;
    let types_code = output.code.code(&GeneratedFileType::Types).cloned().unwrap_or_default();
    let server_code = output
      .code
      .code(&GeneratedFileType::Server)
      .cloned()
      .unwrap_or_default();
    let mod_code = output
      .code
      .code(&GeneratedFileType::Module)
      .cloned()
      .unwrap_or_default();
    tokio::fs::write(self.output.join("types.rs"), types_code).await?;
    tokio::fs::write(self.output.join("server.rs"), server_code).await?;
    tokio::fs::write(self.output.join("mod.rs"), mod_code).await?;
    Ok(())
  }
}

impl GenerateConfig {
  pub fn from_command(command: GenerateCommand) -> anyhow::Result<Self> {
    let GenerateCommand {
      mode,
      input,
      output,
      visibility,
      odata_support,
      enum_mode,
      no_helpers,
      all_schemas,
      only,
      exclude,
      verbose,
      quiet,
      customize,
    } = command;

    let output = match (&mode, output) {
      (GenerateMode::ClientMod | GenerateMode::ServerMod, None) => PathBuf::from("."),
      (_, None) => anyhow::bail!("Output path (-o) is required for types and client modes"),
      (_, Some(path)) => path,
    };
    let enum_policies = EnumPolicies::from(enum_mode);
    let customizations = parse_customizations(customize)?;

    Ok(Self {
      mode,
      input,
      output,
      visibility,
      verbose,
      quiet,
      all_schemas,
      odata_support,
      preserve_case_variants: enum_policies.preserve_case_variants,
      case_insensitive_enums: enum_policies.case_insensitive_enums,
      only_operations: only.map(|ops| ops.into_iter().collect()),
      excluded_operations: exclude.map(|ops| ops.into_iter().collect()),
      no_helpers,
      customizations,
    })
  }
}

fn parse_customizations(customize: Option<Vec<String>>) -> anyhow::Result<HashMap<String, String>> {
  let Some(entries) = customize else {
    return Ok(HashMap::new());
  };

  let mut map = HashMap::new();
  for entry in entries {
    let (key, value) = entry.split_once('=').ok_or_else(|| {
      anyhow::anyhow!("Invalid customize format '{entry}': expected TYPE=PATH (e.g., date_time=crate::MyDateTime)")
    })?;
    map.insert(key.to_string(), value.to_string());
  }
  Ok(map)
}

impl From<EnumCaseMode> for EnumPolicies {
  fn from(enum_mode: EnumCaseMode) -> Self {
    match enum_mode {
      EnumCaseMode::Merge => Self {
        preserve_case_variants: false,
        case_insensitive_enums: false,
      },
      EnumCaseMode::Preserve => Self {
        preserve_case_variants: true,
        case_insensitive_enums: false,
      },
      EnumCaseMode::Relaxed => Self {
        preserve_case_variants: false,
        case_insensitive_enums: true,
      },
    }
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

  fn info(&self, message: &str) {
    if !self.config.quiet {
      println!("{} {message}", format_timestamp().with(self.colors.timestamp()));
    }
  }

  fn stat(&self, label: &str, value: String) {
    if !self.config.quiet {
      println!(
        "            {:<25} {}",
        label.with(self.colors.label()),
        value.with(self.colors.value())
      );
    }
  }

  fn log_loading(&self) {
    self.info(
      &format!("Loading OpenAPI spec from: {}", self.config.input.display())
        .with(self.colors.primary())
        .to_string(),
    );
  }

  fn log_generating(&self) {
    let message = match self.config.mode {
      GenerateMode::Types => "Generating Rust types...",
      GenerateMode::Client => "Generating Rust client...",
      GenerateMode::ClientMod => "Generating Rust client module...",
      GenerateMode::ServerMod => "Generating Rust server module...",
    };
    self.info(&message.with(self.colors.primary()).to_string());
  }

  fn print_statistics(&self, stats: &GenerationStats) {
    if self.config.quiet {
      return;
    }

    match self.config.mode {
      GenerateMode::Types => self.print_type_stats(stats),
      GenerateMode::Client => self.print_client_stats(stats),
      GenerateMode::ClientMod => {
        self.print_type_stats(stats);
        self.print_client_stats(stats);
      }
      GenerateMode::ServerMod => {
        self.print_type_stats(stats);
      }
    }

    self.print_common_stats(stats);
    self.print_cycles(stats);
    self.print_orphaned_schemas(stats);
    self.print_warnings(stats);
  }

  fn print_type_stats(&self, stats: &GenerationStats) {
    self.stat("Types generated:", stats.types_generated.to_string());
    self.stat("", format!("{} structs", stats.structs_generated));
    if stats.enums_with_helpers_generated > 0 {
      self.stat(
        "",
        format!(
          "{} enums, {} have helpers",
          stats.enums_generated, stats.enums_with_helpers_generated
        ),
      );
    } else {
      self.stat("", format!("{} enums", stats.enums_generated));
    }
    self.stat("", format!("{} type aliases", stats.type_aliases_generated));
    self.stat("Operations converted:", stats.operations_converted.to_string());
    if stats.webhooks_converted > 0 {
      self.stat("", format!("{} webhooks", stats.webhooks_converted));
    }
  }

  fn print_client_stats(&self, stats: &GenerationStats) {
    if let Some(methods) = stats.client_methods_generated {
      self.stat("Methods generated:", methods.to_string());
    }
    if let Some(headers) = stats.client_headers_generated {
      self.stat("Headers generated:", headers.to_string());
    }
  }

  fn print_common_stats(&self, stats: &GenerationStats) {
    if !stats.warnings.is_empty() {
      self.stat("Warnings:", stats.warnings.len().to_string());
    }
  }

  fn print_cycles(&self, stats: &GenerationStats) {
    if stats.cycles_detected == 0 {
      return;
    }

    self.stat("Cycles:", stats.cycles_detected.to_string());

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
      self.stat("Orphaned schemas:", stats.orphaned_schemas_count.to_string());
    }
  }

  fn print_warnings(&self, stats: &GenerationStats) {
    if stats.warnings.is_empty() || self.config.quiet {
      return;
    }

    let mut printed_header = false;
    for warning in &stats.warnings {
      let should_print = warning.is_skipped_item() || self.config.verbose;
      if !should_print {
        continue;
      }

      if !printed_header {
        println!();
        printed_header = true;
      }

      if warning.is_skipped_item() {
        eprintln!(
          "{} {}",
          "Skipped:".with(self.colors.accent()),
          format!("{warning}").with(self.colors.primary())
        );
      } else {
        eprintln!(
          "{} {}",
          "Warning:".with(self.colors.accent()),
          format!("{warning}").with(self.colors.primary())
        );
      }
    }
  }

  fn log_writing(&self) {
    self.info(
      &format!("Writing to: {}", self.config.output.display())
        .with(self.colors.primary())
        .to_string(),
    );
  }

  fn log_success(&self) {
    if !self.config.quiet {
      let message = match self.config.mode {
        GenerateMode::Types => "Successfully generated Rust types",
        GenerateMode::Client => "Successfully generated Rust client",
        GenerateMode::ClientMod => "Successfully generated Rust client module",
        GenerateMode::ServerMod => "Successfully generated Rust server module",
      };
      println!();
      println!(
        "{} {}",
        format_timestamp().with(self.colors.timestamp()),
        message.with(self.colors.success())
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
  let source_path = config.input.display().to_string();

  match config.mode {
    GenerateMode::Types => {
      let output = orchestrator.generate_with_header(&source_path)?;
      logger.print_statistics(&output.stats);
      logger.log_writing();
      let code = output.code.code(&GeneratedFileType::Types).cloned().unwrap_or_default();
      config.write_output(code).await?;
    }
    GenerateMode::Client => {
      let output = orchestrator.generate_client_with_header(&source_path)?;
      logger.print_statistics(&output.stats);
      logger.log_writing();
      let code = output
        .code
        .code(&GeneratedFileType::Client)
        .cloned()
        .unwrap_or_default();
      config.write_output(code).await?;
    }
    GenerateMode::ClientMod => {
      let output = orchestrator.generate_client_mod(&source_path)?;
      logger.print_statistics(&output.stats);
      logger.log_writing();
      config.write_module_output(&output).await?;
    }
    GenerateMode::ServerMod => {
      let output = orchestrator.generate_server_mod(&source_path)?;
      logger.print_statistics(&output.stats);
      logger.log_writing();
      config.write_server_module_output(&output).await?;
    }
  }

  logger.log_success();
  Ok(())
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn test_parse_customizations_none() {
    let result = parse_customizations(None).unwrap();
    assert!(result.is_empty());
  }

  #[test]
  fn test_parse_customizations_empty_vec() {
    let result = parse_customizations(Some(vec![])).unwrap();
    assert!(result.is_empty());
  }

  #[test]
  fn test_parse_customizations_single_entry() {
    let result = parse_customizations(Some(vec!["date_time=crate::MyDateTime".to_string()])).unwrap();
    assert_eq!(result.len(), 1);
    assert_eq!(result.get("date_time"), Some(&"crate::MyDateTime".to_string()));
  }

  #[test]
  fn test_parse_customizations_multiple_entries() {
    let result = parse_customizations(Some(vec![
      "date_time=crate::MyDateTime".to_string(),
      "date=crate::MyDate".to_string(),
      "uuid=crate::MyUuid".to_string(),
    ]))
    .unwrap();

    assert_eq!(result.len(), 3);
    assert_eq!(result.get("date_time"), Some(&"crate::MyDateTime".to_string()));
    assert_eq!(result.get("date"), Some(&"crate::MyDate".to_string()));
    assert_eq!(result.get("uuid"), Some(&"crate::MyUuid".to_string()));
  }

  #[test]
  fn test_parse_customizations_with_module_path() {
    let result =
      parse_customizations(Some(vec!["date_time=my_crate::types::custom::IsoDateTime".to_string()])).unwrap();
    assert_eq!(
      result.get("date_time"),
      Some(&"my_crate::types::custom::IsoDateTime".to_string())
    );
  }

  #[test]
  fn test_parse_customizations_invalid_format_no_equals() {
    let result = parse_customizations(Some(vec!["date_time".to_string()]));
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(err.to_string().contains("Invalid customize format"));
  }

  #[test]
  fn test_parse_customizations_with_equals_in_value() {
    let result = parse_customizations(Some(vec!["date_time=crate::Type=Something".to_string()])).unwrap();
    assert_eq!(result.get("date_time"), Some(&"crate::Type=Something".to_string()));
  }
}

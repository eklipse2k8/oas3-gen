use strum::Display;

use crate::generator::ast::{OperationInfo, OperationKind, RustType};

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct GenerationStats {
  pub types_generated: usize,
  pub structs_generated: usize,
  pub enums_generated: usize,
  pub enums_with_helpers_generated: usize,
  pub type_aliases_generated: usize,
  pub operations_converted: usize,
  pub webhooks_converted: usize,
  pub cycles_detected: usize,
  pub cycle_details: Vec<Vec<String>>,
  pub warnings: Vec<GenerationWarning>,
  pub orphaned_schemas_count: usize,
  pub client_methods_generated: usize,
  pub client_headers_generated: usize,
}

impl GenerationStats {
  pub fn record_struct(&mut self) {
    self.structs_generated += 1;
    self.types_generated += 1;
  }

  pub fn record_enum(&mut self, has_helpers: bool) {
    self.enums_generated += 1;
    self.types_generated += 1;
    if has_helpers {
      self.enums_with_helpers_generated += 1;
    }
  }

  pub fn record_type_alias(&mut self) {
    self.type_aliases_generated += 1;
    self.types_generated += 1;
  }

  pub fn record_rust_type(&mut self, rust_type: &RustType) {
    match rust_type {
      RustType::Struct(_) => self.record_struct(),
      RustType::Enum(def) => self.record_enum(!def.methods.is_empty()),
      RustType::DiscriminatedEnum(_) | RustType::ResponseEnum(_) => self.record_enum(false),
      RustType::TypeAlias(_) => self.record_type_alias(),
    }
  }

  pub fn record_rust_types(&mut self, types: &[RustType]) {
    for rust_type in types {
      self.record_rust_type(rust_type);
    }
  }

  pub fn record_operation(&mut self, operation: &OperationInfo) {
    self.operations_converted += 1;
    if matches!(operation.kind, OperationKind::Webhook) {
      self.webhooks_converted += 1;
    }
  }

  pub fn record_operations(&mut self, operations: &[OperationInfo]) {
    for operation in operations {
      self.record_operation(operation);
    }
  }

  pub fn record_cycle(&mut self, cycle: Vec<String>) {
    self.cycles_detected += 1;
    self.cycle_details.push(cycle);
  }

  pub fn record_cycles(&mut self, cycles: Vec<Vec<String>>) {
    for cycle in cycles {
      self.record_cycle(cycle);
    }
  }

  pub fn record_orphaned_schemas(&mut self, count: usize) {
    self.orphaned_schemas_count += count;
  }

  pub fn record_client_methods(&mut self, count: usize) {
    self.client_methods_generated += count;
  }

  pub fn record_client_headers(&mut self, count: usize) {
    self.client_headers_generated += count;
  }

  pub fn record_warning(&mut self, warning: GenerationWarning) {
    self.warnings.push(warning);
  }

  pub fn record_warnings(&mut self, warnings: impl IntoIterator<Item = GenerationWarning>) {
    self.warnings.extend(warnings);
  }
}

#[derive(Debug, Clone, PartialEq, Eq, Display)]
pub enum GenerationWarning {
  #[strum(to_string = "Failed to convert schema '{schema_name}': {error}")]
  SchemaConversionFailed { schema_name: String, error: String },
  #[strum(to_string = "Failed to convert operation '{method} {path}': {error}")]
  OperationConversionFailed {
    method: String,
    path: String,
    error: String,
  },
  #[strum(to_string = "[{operation_id}] {message}")]
  OperationSpecific { operation_id: String, message: String },
  #[strum(to_string = "Schema '{schema_name}': {message}")]
  DiscriminatorMappingFailed { schema_name: String, message: String },
}

impl GenerationWarning {
  pub fn is_skipped_item(&self) -> bool {
    matches!(
      self,
      Self::SchemaConversionFailed { .. } | Self::OperationConversionFailed { .. }
    )
  }
}

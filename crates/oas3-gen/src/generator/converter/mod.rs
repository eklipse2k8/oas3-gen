pub(crate) mod cache;
pub(crate) mod common;
pub(crate) mod discriminator;
pub(crate) mod fields;
pub(crate) mod hashing;
pub(crate) mod inline_resolver;
pub(crate) mod methods;
pub(crate) mod operations;
pub(crate) mod parameters;
pub(crate) mod relaxed_enum;
pub(crate) mod requests;
pub(crate) mod responses;
pub(crate) mod structs;
pub(crate) mod type_resolver;
pub(crate) mod type_usage_recorder;
pub(crate) mod union_types;
pub(crate) mod unions;
pub(crate) mod value_enums;
pub(crate) mod variants;

use std::{
  cell::RefCell,
  collections::{BTreeSet, HashMap},
  rc::Rc,
  sync::Arc,
};

pub(crate) use common::{ConversionOutput, SchemaExt};
use oas3::spec::ObjectSchema;
pub(crate) use type_resolver::TypeResolver;
pub(crate) use type_usage_recorder::TypeUsageRecorder;

use super::ast::RustType;
use crate::generator::{converter::cache::SharedSchemaCache, schema_registry::SchemaRegistry};

/// Policy for handling enum variant name collisions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum EnumCasePolicy {
  /// Append index suffix to colliding variants (e.g., `Value`, `Value1`).
  Preserve,
  /// Merge colliding variants and add serde aliases.
  #[default]
  Deduplicate,
}

/// Policy for generating enum helper constructors.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum EnumHelperPolicy {
  /// Generate helper methods for creating enum variants.
  #[default]
  Generate,
  /// Disable helper method generation.
  Disable,
}

/// Policy for enum deserialization case sensitivity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum EnumDeserializePolicy {
  /// Use standard case-sensitive deserialization.
  #[default]
  CaseSensitive,
  /// Generate custom case-insensitive deserializer.
  CaseInsensitive,
}

/// Policy for OData-specific schema support.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ODataPolicy {
  /// Disable OData-specific handling.
  #[default]
  Disabled,
  /// Enable OData support (makes `@odata.*` fields optional).
  Enabled,
}

/// Configuration for code generation.
///
/// Uses typed enums instead of booleans to make intent explicit at call sites
/// and prevent invalid combinations.
#[derive(Debug, Clone, Default)]
pub(crate) struct CodegenConfig {
  pub enum_case: EnumCasePolicy,
  pub enum_helpers: EnumHelperPolicy,
  pub enum_deserialize: EnumDeserializePolicy,
  pub odata: ODataPolicy,
  pub customizations: HashMap<String, String>,
}

impl CodegenConfig {
  /// Returns whether enum variant collisions should preserve original names with suffixes.
  #[must_use]
  pub fn preserve_case_variants(&self) -> bool {
    self.enum_case == EnumCasePolicy::Preserve
  }

  /// Returns whether enums should use case-insensitive deserialization.
  #[must_use]
  pub fn case_insensitive_enums(&self) -> bool {
    self.enum_deserialize == EnumDeserializePolicy::CaseInsensitive
  }

  /// Returns whether helper constructors should be disabled.
  #[must_use]
  pub fn no_helpers(&self) -> bool {
    self.enum_helpers == EnumHelperPolicy::Disable
  }

  /// Returns whether OData support is enabled.
  #[must_use]
  pub fn odata_support(&self) -> bool {
    self.odata == ODataPolicy::Enabled
  }
}

#[derive(Debug, Clone)]
pub(crate) struct ConverterContext {
  pub(crate) graph: Arc<SchemaRegistry>,
  pub(crate) config: CodegenConfig,
  pub(crate) cache: RefCell<SharedSchemaCache>,
  pub(crate) reachable_schemas: Option<Arc<BTreeSet<String>>>,
}

impl ConverterContext {
  pub(crate) fn new(
    graph: Arc<SchemaRegistry>,
    config: CodegenConfig,
    cache: SharedSchemaCache,
    reachable_schemas: Option<Arc<BTreeSet<String>>>,
  ) -> Self {
    Self {
      graph,
      config,
      cache: RefCell::new(cache),
      reachable_schemas,
    }
  }

  /// Borrows the schema registry graph.
  pub(crate) fn graph(&self) -> &SchemaRegistry {
    &self.graph
  }

  /// Borrows the configuration.
  pub(crate) fn config(&self) -> &CodegenConfig {
    &self.config
  }
}

/// Main entry point for converting OpenAPI schemas into Rust AST.
///
/// Delegates to `TypeResolver` for schema conversion, providing a stable API
/// for the orchestrator while centralizing conversion logic in `TypeResolver`.
#[derive(Debug, Clone)]
pub(crate) struct SchemaConverter {
  context: Rc<ConverterContext>,
  type_resolver: TypeResolver,
}

impl SchemaConverter {
  pub(crate) fn new(context: &Rc<ConverterContext>) -> Self {
    Self {
      context: context.clone(),
      type_resolver: TypeResolver::new(context.clone()),
    }
  }

  pub(crate) fn context(&self) -> &Rc<ConverterContext> {
    &self.context
  }

  /// Converts a schema definition into Rust types.
  ///
  /// Handles `allOf`, `oneOf`, `anyOf`, enums, and objects.
  pub(crate) fn convert_schema(&self, name: &str, schema: &ObjectSchema) -> anyhow::Result<Vec<RustType>> {
    self.type_resolver.convert_schema(name, schema)
  }

  /// Checks if a name corresponds to a known schema in the graph.
  pub(crate) fn contains(&self, name: &str) -> bool {
    self.context.graph().contains(name)
  }
}

#[cfg(test)]
mod tests;

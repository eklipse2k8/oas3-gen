pub(crate) mod cache;
mod common;
pub(crate) mod discriminator;
mod enums;
pub(crate) mod hashing;
pub(crate) mod metadata;
pub(crate) mod operations;
pub(crate) mod path_renderer;
pub(crate) mod responses;
pub(crate) mod structs;
pub(crate) mod type_resolver;
mod type_usage_recorder;

use std::{
  collections::{BTreeSet, HashSet},
  sync::Arc,
};

pub(crate) use common::{ConversionOutput, SchemaExt};
use oas3::spec::ObjectSchema;
pub(crate) use type_usage_recorder::TypeUsageRecorder;

use self::{cache::SharedSchemaCache, enums::EnumConverter, structs::StructConverter, type_resolver::TypeResolver};
use super::{
  ast::{RustType, StructKind, TypeAliasDef, TypeAliasToken, TypeRef},
  schema_registry::SchemaRegistry,
};
use crate::generator::{converter::type_resolver::TypeResolverBuilder, naming::identifiers::to_rust_type_name};

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
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct CodegenConfig {
  pub enum_case: EnumCasePolicy,
  pub enum_helpers: EnumHelperPolicy,
  pub enum_deserialize: EnumDeserializePolicy,
  pub odata: ODataPolicy,
}

impl CodegenConfig {
  /// Returns whether enum variant collisions should preserve original names with suffixes.
  #[must_use]
  pub fn preserve_case_variants(self) -> bool {
    self.enum_case == EnumCasePolicy::Preserve
  }

  /// Returns whether enums should use case-insensitive deserialization.
  #[must_use]
  pub fn case_insensitive_enums(self) -> bool {
    self.enum_deserialize == EnumDeserializePolicy::CaseInsensitive
  }

  /// Returns whether helper constructors should be disabled.
  #[must_use]
  pub fn no_helpers(self) -> bool {
    self.enum_helpers == EnumHelperPolicy::Disable
  }

  /// Returns whether OData support is enabled.
  #[must_use]
  pub fn odata_support(self) -> bool {
    self.odata == ODataPolicy::Enabled
  }
}

/// Main entry point for converting OpenAPI schemas into Rust AST.
///
/// Coordinates `StructConverter`, `EnumConverter`, and `TypeResolver` to transform
/// `ObjectSchema` definitions into `RustType` definitions (structs, enums, aliases).
pub(crate) struct SchemaConverter {
  type_resolver: TypeResolver,
  struct_converter: StructConverter,
  enum_converter: EnumConverter,
  cached_schema_names: HashSet<String>,
}

impl SchemaConverter {
  pub(crate) fn new(graph: &Arc<SchemaRegistry>, config: CodegenConfig) -> Self {
    let type_resolver = TypeResolverBuilder::default()
      .graph(graph.clone())
      .config(config)
      .build()
      .expect("TypeResolver");
    let cached_schema_names = Self::build_schema_name_cache(graph);
    Self {
      type_resolver: type_resolver.clone(),
      struct_converter: StructConverter::new(graph, config, None),
      enum_converter: EnumConverter::new(graph, type_resolver, config),
      cached_schema_names,
    }
  }

  pub(crate) fn new_with_filter(
    graph: &Arc<SchemaRegistry>,
    reachable_schemas: BTreeSet<String>,
    config: CodegenConfig,
  ) -> Self {
    let type_resolver = TypeResolverBuilder::default()
      .graph(graph.clone())
      .config(config)
      .build()
      .expect("TypeResolver");
    let cached_schema_names = Self::build_schema_name_cache(graph);
    Self {
      type_resolver: type_resolver.clone(),
      struct_converter: StructConverter::new(graph, config, Some(Arc::new(reachable_schemas))),
      enum_converter: EnumConverter::new(graph, type_resolver, config),
      cached_schema_names,
    }
  }

  /// Converts a schema definition into Rust types.
  ///
  /// Handles `allOf`, `oneOf`, `anyOf`, enums, and objects.
  pub(crate) fn convert_schema(
    &self,
    name: &str,
    schema: &ObjectSchema,
    mut cache: Option<&mut SharedSchemaCache>,
  ) -> anyhow::Result<Vec<RustType>> {
    if !schema.all_of.is_empty() {
      let cache_reborrow = cache.as_deref_mut();
      return self
        .struct_converter
        .convert_all_of_schema(name, schema, cache_reborrow);
    }

    if !schema.one_of.is_empty() {
      let cache_reborrow = cache.as_deref_mut();
      return self
        .enum_converter
        .convert_union(name, schema, enums::UnionKind::OneOf, cache_reborrow);
    }

    if !schema.any_of.is_empty() {
      let cache_reborrow = cache.as_deref_mut();
      return self
        .enum_converter
        .convert_union(name, schema, enums::UnionKind::AnyOf, cache_reborrow);
    }

    if !schema.enum_values.is_empty() {
      let cache_reborrow = cache.as_deref_mut();
      return Ok(
        self
          .enum_converter
          .convert_value_enum(name, schema, cache_reborrow)
          .into_iter()
          .collect(),
      );
    }

    if !schema.properties.is_empty() || schema.additional_properties.is_some() {
      let cache_reborrow = cache;
      let result = self
        .struct_converter
        .convert_struct(name, schema, None, cache_reborrow)?;
      return self
        .struct_converter
        .finalize_struct_types(name, schema, result.result, result.inline_types);
    }

    if let Some(output) = self.try_convert_array_type_alias_with_union_items(name, schema, cache)? {
      let alias = RustType::TypeAlias(TypeAliasDef {
        name: TypeAliasToken::from_raw(name),
        docs: metadata::extract_docs(schema.description.as_ref()),
        target: output.result,
      });
      let mut result = vec![alias];
      result.extend(output.inline_types);
      return Ok(result);
    }

    let type_ref = self.type_resolver.resolve_type(schema)?;
    let alias = RustType::TypeAlias(TypeAliasDef {
      name: TypeAliasToken::from_raw(name),
      docs: metadata::extract_docs(schema.description.as_ref()),
      target: type_ref,
    });

    Ok(vec![alias])
  }

  /// Helper to convert a struct schema specifically.
  pub(crate) fn convert_struct(
    &self,
    name: &str,
    schema: &ObjectSchema,
    kind: Option<StructKind>,
    cache: Option<&mut SharedSchemaCache>,
  ) -> anyhow::Result<ConversionOutput<RustType>> {
    self.struct_converter.convert_struct(name, schema, kind, cache)
  }

  /// Resolves a schema to a Rust type reference (e.g. `String`, `Vec<i32>`, `MyStruct`).
  pub(crate) fn resolve_type(&self, schema: &ObjectSchema) -> anyhow::Result<TypeRef> {
    self.type_resolver.resolve_type(schema)
  }

  fn build_schema_name_cache(graph: &SchemaRegistry) -> HashSet<String> {
    graph
      .schema_names()
      .into_iter()
      .flat_map(|schema_name| {
        let rust_name = to_rust_type_name(schema_name);
        [schema_name.clone(), rust_name]
      })
      .collect()
  }

  /// Checks if a name corresponds to a known schema in the graph.
  pub(crate) fn is_schema_name(&self, name: &str) -> bool {
    self.cached_schema_names.contains(name)
  }

  fn try_convert_array_type_alias_with_union_items(
    &self,
    name: &str,
    schema: &ObjectSchema,
    cache: Option<&mut SharedSchemaCache>,
  ) -> anyhow::Result<Option<ConversionOutput<TypeRef>>> {
    if !schema.is_array() && !schema.is_nullable_array() {
      return Ok(None);
    }

    if let Some(output) = self
      .type_resolver
      .resolve_array_with_inline_items(name, name, schema, cache)?
    {
      let type_ref = if schema.is_nullable_array() {
        output.result.with_option()
      } else {
        output.result
      };
      return Ok(Some(ConversionOutput::with_inline_types(type_ref, output.inline_types)));
    }

    Ok(None)
  }
}

#[cfg(test)]
mod tests;

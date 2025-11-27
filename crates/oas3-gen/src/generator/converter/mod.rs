pub(crate) mod cache;
mod common;
mod enums;
mod field_optionality;
pub(crate) mod hashing;
pub(crate) mod metadata;
pub(crate) mod operations;
pub(crate) mod path_renderer;
pub(crate) mod responses;
mod string_enum_optimizer;
pub(crate) mod structs;
pub(crate) mod type_resolver;
mod type_usage_recorder;

use std::{
  collections::{BTreeSet, HashSet},
  sync::Arc,
};

pub(crate) use common::{ConversionOutput, SchemaExt};
pub(crate) use field_optionality::FieldOptionalityPolicy;
use oas3::spec::ObjectSchema;
pub(crate) use type_usage_recorder::TypeUsageRecorder;

use self::{cache::SharedSchemaCache, enums::EnumConverter, structs::StructConverter, type_resolver::TypeResolver};
use super::{
  ast::{RustType, StructKind, TypeAliasDef, TypeRef},
  schema_registry::SchemaRegistry,
};
use crate::generator::naming::identifiers::to_rust_type_name;

#[derive(Debug, Clone, Copy)]
pub(crate) struct CodegenConfig {
  pub preserve_case_variants: bool,
  pub case_insensitive_enums: bool,
  pub no_helpers: bool,
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
  /// Creates a new `SchemaConverter` with standard configuration.
  pub(crate) fn new(
    graph: &Arc<SchemaRegistry>,
    optionality_policy: FieldOptionalityPolicy,
    config: CodegenConfig,
  ) -> Self {
    let type_resolver = TypeResolver::new(graph, config);
    let cached_schema_names = Self::build_schema_name_cache(graph);
    Self {
      type_resolver: type_resolver.clone(),
      struct_converter: StructConverter::new(graph.clone(), config, None, optionality_policy),
      enum_converter: EnumConverter::new(graph, type_resolver, config),
      cached_schema_names,
    }
  }

  /// Creates a `SchemaConverter` that filters generated types to a reachable set.
  ///
  /// Useful for generating only a subset of the API surface (e.g., specific tags).
  pub(crate) fn new_with_filter(
    graph: &Arc<SchemaRegistry>,
    reachable_schemas: BTreeSet<String>,
    optionality_policy: FieldOptionalityPolicy,
    config: CodegenConfig,
  ) -> Self {
    let type_resolver = TypeResolver::new(graph, config);
    let cached_schema_names = Self::build_schema_name_cache(graph);
    Self {
      type_resolver: type_resolver.clone(),
      struct_converter: StructConverter::new(
        graph.clone(),
        config,
        Some(Arc::new(reachable_schemas)),
        optionality_policy,
      ),
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
        .convert_union_enum(name, schema, enums::UnionKind::OneOf, cache_reborrow);
    }

    if !schema.any_of.is_empty() {
      let cache_reborrow = cache.as_deref_mut();
      return self
        .enum_converter
        .convert_union_enum(name, schema, enums::UnionKind::AnyOf, cache_reborrow);
    }

    if !schema.enum_values.is_empty() {
      let cache_reborrow = cache.as_deref_mut();
      return Ok(
        self
          .enum_converter
          .convert_simple_enum(name, schema, cache_reborrow)
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

    let type_ref = self.type_resolver.schema_to_type_ref(schema)?;
    let alias = RustType::TypeAlias(TypeAliasDef {
      name: to_rust_type_name(name),
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
  pub(crate) fn schema_to_type_ref(&self, schema: &ObjectSchema) -> anyhow::Result<TypeRef> {
    self.type_resolver.schema_to_type_ref(schema)
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
}

#[cfg(test)]
mod tests;

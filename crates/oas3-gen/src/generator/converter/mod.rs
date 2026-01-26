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

use anyhow::Result;
pub(crate) use common::ConversionOutput;
use oas3::spec::ObjectSchema;
pub(crate) use operations::{OperationsProcessor, build_server_trait};
pub(crate) use type_resolver::TypeResolver;
pub(crate) use type_usage_recorder::TypeUsageRecorder;

use super::ast::RustType;
use crate::{
  generator::{
    ast::{Documentation, TypeAliasDef, TypeAliasToken, TypeRef},
    converter::{
      cache::SharedSchemaCache,
      discriminator::DiscriminatorConverter,
      structs::StructConverter,
      union_types::UnionKind,
      unions::{EnumConverter, UnionConverter},
    },
    naming::{constants::DISCRIMINATED_BASE_SUFFIX, identifiers::to_rust_type_name},
    schema_registry::SchemaRegistry,
  },
  utils::SchemaExt,
};

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

/// Target for code generation (client vs server).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum GenerationTarget {
  /// Generate HTTP client code.
  #[default]
  Client,
  /// Generate HTTP server code (axum).
  Server,
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
  pub target: GenerationTarget,
  pub customizations: HashMap<String, String>,
}

impl CodegenConfig {
  /// Returns `true` if enum variant name collisions should be resolved by appending
  /// numeric suffixes (e.g., `Value`, `Value1`) rather than merging with serde aliases.
  #[must_use]
  pub fn preserve_case_variants(&self) -> bool {
    self.enum_case == EnumCasePolicy::Preserve
  }

  /// Returns `true` if enums should generate custom deserializers that accept
  /// any casing of variant names (e.g., "FOO", "foo", "Foo" all deserialize to `Foo`).
  #[must_use]
  pub fn case_insensitive_enums(&self) -> bool {
    self.enum_deserialize == EnumDeserializePolicy::CaseInsensitive
  }

  /// Returns `true` if enum helper constructor methods should be omitted from generated code.
  #[must_use]
  pub fn no_helpers(&self) -> bool {
    self.enum_helpers == EnumHelperPolicy::Disable
  }

  /// Returns `true` if OData-specific handling is enabled, which makes `@odata.*` fields
  /// optional even when the schema marks them as required.
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
  pub(crate) type_usage: RefCell<TypeUsageRecorder>,
  pub(crate) reachable_schemas: Option<Arc<BTreeSet<String>>>,
}

impl ConverterContext {
  /// Creates a new converter context with the given schema registry, configuration,
  /// type cache, and optional schema filter.
  ///
  /// The `reachable_schemas` parameter, when `Some`, restricts conversion to only
  /// schemas reachable from the filtered operations, enabling dead-code elimination.
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
      type_usage: RefCell::new(TypeUsageRecorder::new()),
      reachable_schemas,
    }
  }

  /// Returns a reference to the schema registry containing all resolved schemas
  /// and their dependency relationships.
  pub(crate) fn graph(&self) -> &SchemaRegistry {
    &self.graph
  }

  /// Returns a reference to the code generation configuration.
  pub(crate) fn config(&self) -> &CodegenConfig {
    &self.config
  }

  /// Extracts the accumulated type usage data, replacing it with an empty recorder.
  ///
  /// Call this after conversion completes to retrieve which types are used in
  /// requests vs responses for serde mode optimization.
  pub(crate) fn take_type_usage(&self) -> TypeUsageRecorder {
    self.type_usage.take()
  }

  /// Records that the named type appears in an HTTP request context (request body or parameter).
  ///
  /// Types used only in requests may derive `Serialize` without `Deserialize`.
  pub(crate) fn mark_request(&self, type_name: impl Into<super::ast::EnumToken>) {
    self.type_usage.borrow_mut().mark_request(type_name);
  }

  /// Records that the named type appears in an HTTP response context.
  ///
  /// Types used only in responses may derive `Deserialize` without `Serialize`.
  pub(crate) fn mark_response(&self, type_name: impl Into<super::ast::EnumToken>) {
    self.type_usage.borrow_mut().mark_response(type_name);
  }

  /// Records that multiple types appear in HTTP request contexts.
  ///
  /// More efficient than calling [`mark_request`](Self::mark_request) in a loop.
  pub(crate) fn mark_request_iter<I, T>(&self, types: I)
  where
    I: IntoIterator<Item = T>,
    T: Into<super::ast::EnumToken>,
  {
    self.type_usage.borrow_mut().mark_request_iter(types);
  }

  /// Extracts custom type names from a [`TypeRef`] and records them as response types.
  ///
  /// Handles nested types like `Box<CustomType>` by extracting the inner type name.
  pub(crate) fn mark_response_type_ref(&self, type_ref: &TypeRef) {
    self.type_usage.borrow_mut().mark_response_type_ref(type_ref);
  }

  /// Combines type usage data from another recorder into this context's recorder.
  ///
  /// Used when sub-converters track usage independently and results must be aggregated.
  pub(crate) fn merge_usage(&self, other: TypeUsageRecorder) {
    self.type_usage.borrow_mut().merge(other);
  }

  /// Increments the count of generated HTTP client or server methods.
  ///
  /// Used for generation statistics and progress reporting.
  pub(crate) fn record_method(&self) {
    self.type_usage.borrow_mut().record_method();
  }

  /// Records an HTTP header name for generation statistics.
  ///
  /// Header names are normalized to lowercase for deduplication.
  pub(crate) fn record_header(&self, header_name: &str) {
    self.type_usage.borrow_mut().record_header(header_name);
  }
}

/// Main entry point for converting OpenAPI schemas into Rust AST.
///
/// Orchestrates the one-way conversion pipeline from OpenAPI schemas to Rust types.
/// Uses `TypeResolver` for read-only type mapping and navigation, while managing
/// the conversion flow through specialized converters (`StructConverter`, `EnumConverter`, etc.).
#[derive(Debug, Clone)]
pub(crate) struct SchemaConverter {
  context: Rc<ConverterContext>,
  type_resolver: TypeResolver,
  struct_converter: StructConverter,
  enum_converter: EnumConverter,
  union_converter: UnionConverter,
  discriminator_converter: DiscriminatorConverter,
}

impl SchemaConverter {
  /// Creates a new schema converter with all specialized sub-converters.
  ///
  /// Each sub-converter receives a clone of the shared context `Rc`, enabling
  /// coordinated type caching and usage tracking across conversion operations.
  pub(crate) fn new(context: &Rc<ConverterContext>) -> Self {
    Self {
      type_resolver: TypeResolver::new(context.clone()),
      struct_converter: StructConverter::new(context.clone()),
      enum_converter: EnumConverter::new(context.clone()),
      union_converter: UnionConverter::new(context.clone()),
      discriminator_converter: DiscriminatorConverter::new(context.clone()),
      context: context.clone(),
    }
  }

  /// Returns a reference to the shared converter context.
  pub(crate) fn context(&self) -> &Rc<ConverterContext> {
    &self.context
  }

  /// Returns `true` if the schema registry contains a schema with the given name.
  pub(crate) fn contains(&self, name: &str) -> bool {
    self.context.graph().contains(name)
  }

  /// Converts a named OpenAPI schema into one or more Rust type definitions.
  ///
  /// Routes the schema to the appropriate specialized converter based on its structure:
  /// - `allOf` schemas become merged structs
  /// - `oneOf`/`anyOf` schemas become tagged or untagged enums
  /// - Schemas with `enum` values become string enums
  /// - Schemas with `properties` become structs
  /// - Array schemas become `Vec<T>` type aliases
  /// - Primitive schemas become type aliases
  ///
  /// Returns multiple types when inline definitions are extracted (e.g., nested
  /// anonymous objects become separate struct definitions).
  pub(crate) fn convert_schema(&self, name: &str, schema: &ObjectSchema) -> Result<Vec<RustType>> {
    if schema.has_intersection() {
      return self.struct_converter.convert_all_of_schema(name);
    }

    if let Some((_, kind)) = schema.union_variants_with_kind() {
      if schema.discriminator.is_none() && self.type_resolver.is_wrapper_union(schema)? {
        return Ok(vec![]);
      }

      if let Some(flattened) = self.type_resolver.try_flatten_nested_union(schema)? {
        return self
          .union_converter
          .convert_union(name, &flattened, UnionKind::from_schema(&flattened))
          .map(ConversionOutput::into_vec);
      }

      return self
        .union_converter
        .convert_union(name, schema, kind)
        .map(ConversionOutput::into_vec);
    }

    if !schema.enum_values.is_empty() {
      return Ok(vec![self.enum_converter.convert_value_enum(name, schema)]);
    }

    if !schema.properties.is_empty() || schema.additional_properties.is_some() {
      let result = self.struct_converter.convert_struct(name, schema, None)?;
      return self.finalize_struct_types(name, schema, result.result, result.inline_types);
    }

    if let Some(output) = self.try_array_alias(name, schema)? {
      let alias = RustType::TypeAlias(TypeAliasDef {
        name: TypeAliasToken::from_raw(name),
        docs: Documentation::from_optional(schema.description.as_ref()),
        target: output.result,
      });
      let mut result = vec![alias];
      result.extend(output.inline_types);
      return Ok(result);
    }

    let type_ref = self.type_resolver.resolve_type(schema)?;
    Ok(vec![RustType::TypeAlias(TypeAliasDef {
      name: TypeAliasToken::from_raw(name),
      docs: Documentation::from_optional(schema.description.as_ref()),
      target: type_ref,
    })])
  }

  /// Builds a discriminated union enum for a base schema that defines discriminator mappings.
  ///
  /// The `fallback_type` specifies the struct type to use for the catch-all variant
  /// when the discriminator value doesn't match any known mapping.
  fn discriminated_enum(&self, name: &str, schema: &ObjectSchema, fallback_type: &str) -> Result<RustType> {
    self
      .discriminator_converter
      .build_base_discriminated_enum(name, schema, fallback_type)
  }

  /// Assembles the final type collection for a struct conversion, optionally prepending
  /// a discriminated enum wrapper.
  ///
  /// When the schema defines a discriminator with mappings, this creates a tagged union
  /// enum that wraps the base struct and its subtypes, enabling polymorphic deserialization.
  pub(crate) fn finalize_struct_types(
    &self,
    name: &str,
    schema: &ObjectSchema,
    main_type: RustType,
    inline_types: Vec<RustType>,
  ) -> Result<Vec<RustType>> {
    let discriminated_enum = schema
      .is_discriminated_base_type()
      .then(|| {
        let base_struct_name = match &main_type {
          RustType::Struct(def) => def.name.as_str().to_string(),
          _ => format!("{}{DISCRIMINATED_BASE_SUFFIX}", to_rust_type_name(name)),
        };
        self.discriminated_enum(name, schema, &base_struct_name)
      })
      .transpose()?;

    Ok(
      discriminated_enum
        .into_iter()
        .chain(std::iter::once(main_type))
        .chain(inline_types)
        .collect(),
    )
  }

  /// Attempts to create a `Vec<T>` type reference for an array schema with inline items.
  ///
  /// Returns `None` if the schema is not an array type or has no inline item definition.
  /// Returns `Some` with the `Vec` type reference and any inline types extracted from
  /// the array items (e.g., anonymous structs or enums).
  fn try_array_alias(&self, name: &str, schema: &ObjectSchema) -> Result<Option<ConversionOutput<TypeRef>>> {
    if !schema.is_array() && !schema.is_nullable_array() {
      return Ok(None);
    }

    if let Some(output) = self.type_resolver.try_inline_array(name, name, schema)? {
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

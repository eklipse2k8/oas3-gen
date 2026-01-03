pub(crate) mod cache;
pub(crate) mod common;
pub(crate) mod discriminator;
pub(crate) mod fields;
pub(crate) mod hashing;
pub(crate) mod methods;
pub(crate) mod operations;
pub(crate) mod parameters;
pub(crate) mod relaxed_enum;
pub(crate) mod requests;
pub(crate) mod responses;
pub(crate) mod struct_summaries;
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
use oas3::spec::{ObjectOrReference, ObjectSchema};
pub(crate) use type_usage_recorder::TypeUsageRecorder;

use self::{
  structs::StructConverter,
  type_resolver::TypeResolver,
  union_types::UnionKind,
  unions::{EnumConverter, UnionConverter},
};
use super::ast::{Documentation, RustType, TypeAliasDef, TypeAliasToken, TypeRef};
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

/// Output from converting an inline schema.
pub(crate) struct InlineSchemaOutput {
  pub type_name: String,
  pub generated_types: Vec<RustType>,
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
/// Coordinates `StructConverter`, `EnumConverter`, and `TypeResolver` to transform
/// `ObjectSchema` definitions into `RustType` definitions (structs, enums, aliases).
#[derive(Debug, Clone)]
pub(crate) struct SchemaConverter {
  context: Rc<ConverterContext>,
  type_resolver: TypeResolver,
  struct_converter: StructConverter,
  enum_converter: EnumConverter,
  union_converter: UnionConverter,
}

impl SchemaConverter {
  pub(crate) fn new(context: &Rc<ConverterContext>) -> Self {
    Self {
      context: context.clone(),
      type_resolver: TypeResolver::new(context.clone()),
      struct_converter: StructConverter::new(context.clone()),
      enum_converter: EnumConverter::new(context.clone()),
      union_converter: UnionConverter::new(context.clone()),
    }
  }

  pub(crate) fn context(&self) -> &Rc<ConverterContext> {
    &self.context
  }

  /// Converts a schema definition into Rust types.
  ///
  /// Handles `allOf`, `oneOf`, `anyOf`, enums, and objects.
  pub(crate) fn convert_schema(&self, name: &str, schema: &ObjectSchema) -> anyhow::Result<Vec<RustType>> {
    if schema.has_intersection() {
      return self.struct_converter.convert_all_of_schema(name);
    }

    if !schema.one_of.is_empty() {
      return self
        .union_converter
        .convert_union(name, schema, UnionKind::OneOf)
        .map(ConversionOutput::into_vec);
    }

    if !schema.any_of.is_empty() {
      return self
        .union_converter
        .convert_union(name, schema, UnionKind::AnyOf)
        .map(ConversionOutput::into_vec);
    }

    if !schema.enum_values.is_empty() {
      return Ok(vec![self.enum_converter.convert_value_enum(name, schema)]);
    }

    if !schema.properties.is_empty() || schema.additional_properties.is_some() {
      let result = self.struct_converter.convert_struct(name, schema, None)?;
      return self
        .struct_converter
        .finalize_struct_types(name, schema, result.result, result.inline_types);
    }

    if let Some(output) = self.try_convert_array_type_alias_with_union_items(name, schema)? {
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
    let alias = RustType::TypeAlias(TypeAliasDef {
      name: TypeAliasToken::from_raw(name),
      docs: Documentation::from_optional(schema.description.as_ref()),
      target: type_ref,
    });

    Ok(vec![alias])
  }

  /// Resolves a schema to a Rust type reference (e.g. `String`, `Vec<i32>`, `MyStruct`).
  pub(crate) fn resolve_type(&self, schema: &ObjectSchema) -> anyhow::Result<TypeRef> {
    self.type_resolver.resolve_type(schema)
  }

  /// Resolves a property-like schema (struct field or parameter) to a Rust type.
  ///
  /// Unlike `resolve_type`, this method handles inline enums, structs, and unions,
  /// generating new types as needed and returning them along with the type reference.
  pub(crate) fn resolve_property_type(
    &self,
    parent_type_name: &str,
    property_name: &str,
    property_schema: &ObjectSchema,
    property_schema_ref: &ObjectOrReference<ObjectSchema>,
  ) -> anyhow::Result<ConversionOutput<TypeRef>> {
    self
      .type_resolver
      .resolve_property_type(parent_type_name, property_name, property_schema, property_schema_ref)
  }

  /// Converts an inline schema with caching and deduplication.
  ///
  /// This handles the common pattern for inline schemas in request bodies and responses:
  /// 1. Check if schema was already converted (cache lookup by hash)
  /// 2. If not, convert the schema and register in cache
  ///
  /// Returns the type name and any generated types, or None if the schema is empty.
  pub(crate) fn convert_inline_schema(
    &self,
    schema: &ObjectSchema,
    base_name: &str,
  ) -> anyhow::Result<Option<InlineSchemaOutput>> {
    if schema.is_empty_object() {
      return Ok(None);
    }

    {
      let cache = self.context.cache.borrow();
      if let Some(cached_name) = cache.get_type_name(schema)? {
        return Ok(Some(InlineSchemaOutput {
          type_name: cached_name,
          generated_types: vec![],
        }));
      }
    }

    let effective_schema = if schema.all_of.is_empty() {
      schema.clone()
    } else {
      self.context.graph().merge_all_of(schema)
    };

    // We must drop the borrow before calling convert_schema which might borrow mutable
    let unique_name = self.context.cache.borrow_mut().make_unique_name(base_name);
    let generated = self.convert_schema(&unique_name, &effective_schema)?;

    let Some(main_type) = generated.last().cloned() else {
      return Ok(None);
    };

    let final_name = self
      .context
      .cache
      .borrow_mut()
      .register_type(schema, &unique_name, vec![], main_type)?;

    Ok(Some(InlineSchemaOutput {
      type_name: final_name,
      generated_types: generated,
    }))
  }

  /// Checks if a name corresponds to a known schema in the graph.
  pub(crate) fn contains(&self, name: &str) -> bool {
    self.context.graph().contains(name)
  }

  pub(crate) fn merge_all_of(&self, schema: &ObjectSchema) -> ObjectSchema {
    self.context.graph().merge_all_of(schema)
  }

  fn try_convert_array_type_alias_with_union_items(
    &self,
    name: &str,
    schema: &ObjectSchema,
  ) -> anyhow::Result<Option<ConversionOutput<TypeRef>>> {
    if !schema.is_array() && !schema.is_nullable_array() {
      return Ok(None);
    }

    if let Some(output) = self.type_resolver.resolve_array_with_inline_items(name, name, schema)? {
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

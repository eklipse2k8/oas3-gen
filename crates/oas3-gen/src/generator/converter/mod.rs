pub(crate) mod cache;
mod constants;
mod enums;
mod field_optionality;
mod metadata;
pub(crate) mod operations;
mod structs;
mod type_resolver;
mod type_usage_recorder;
mod utils;

use std::collections::BTreeSet;

pub(crate) use field_optionality::FieldOptionalityPolicy;
use oas3::spec::ObjectSchema;
pub(crate) use type_usage_recorder::TypeUsageRecorder;

use self::{enums::EnumConverter, structs::StructConverter, type_resolver::TypeResolver};
use super::{
  ast::{RustType, StructKind, TypeAliasDef, TypeRef},
  schema_graph::SchemaGraph,
};
use crate::reserved::to_rust_type_name;

pub(crate) const REQUEST_SUFFIX: &str = "Request";
pub(crate) const REQUEST_BODY_SUFFIX: &str = "RequestBody";
pub(crate) const RESPONSE_SUFFIX: &str = "Response";
pub(crate) const BODY_FIELD_NAME: &str = "body";
pub(crate) const SUCCESS_RESPONSE_PREFIX: char = '2';

pub(crate) const REQUEST_PARAMS_SUFFIX: &str = "Params";
pub(crate) const RESPONSE_ENUM_SUFFIX: &str = "Enum";
pub(crate) const DISCRIMINATED_BASE_SUFFIX: &str = "Base";
pub(crate) const MERGED_SCHEMA_CACHE_SUFFIX: &str = "_merged";
pub(crate) const RESPONSE_PREFIX: &str = "Response";

pub(crate) const DEFAULT_RESPONSE_VARIANT: &str = "Unknown";
pub(crate) const DEFAULT_RESPONSE_DESCRIPTION: &str = "Unknown response";

pub(crate) const STATUS_OK: &str = "Ok";
pub(crate) const STATUS_CREATED: &str = "Created";
pub(crate) const STATUS_ACCEPTED: &str = "Accepted";
pub(crate) const STATUS_NO_CONTENT: &str = "NoContent";
pub(crate) const STATUS_MOVED_PERMANENTLY: &str = "MovedPermanently";
pub(crate) const STATUS_FOUND: &str = "Found";
pub(crate) const STATUS_NOT_MODIFIED: &str = "NotModified";
pub(crate) const STATUS_BAD_REQUEST: &str = "BadRequest";
pub(crate) const STATUS_UNAUTHORIZED: &str = "Unauthorized";
pub(crate) const STATUS_FORBIDDEN: &str = "Forbidden";
pub(crate) const STATUS_NOT_FOUND: &str = "NotFound";
pub(crate) const STATUS_METHOD_NOT_ALLOWED: &str = "MethodNotAllowed";
pub(crate) const STATUS_NOT_ACCEPTABLE: &str = "NotAcceptable";
pub(crate) const STATUS_REQUEST_TIMEOUT: &str = "RequestTimeout";
pub(crate) const STATUS_CONFLICT: &str = "Conflict";
pub(crate) const STATUS_GONE: &str = "Gone";
pub(crate) const STATUS_UNPROCESSABLE_ENTITY: &str = "UnprocessableEntity";
pub(crate) const STATUS_TOO_MANY_REQUESTS: &str = "TooManyRequests";
pub(crate) const STATUS_INTERNAL_SERVER_ERROR: &str = "InternalServerError";
pub(crate) const STATUS_NOT_IMPLEMENTED: &str = "NotImplemented";
pub(crate) const STATUS_BAD_GATEWAY: &str = "BadGateway";
pub(crate) const STATUS_SERVICE_UNAVAILABLE: &str = "ServiceUnavailable";
pub(crate) const STATUS_GATEWAY_TIMEOUT: &str = "GatewayTimeout";

pub(crate) const STATUS_INFORMATIONAL: &str = "Informational";
pub(crate) const STATUS_SUCCESS: &str = "Success";
pub(crate) const STATUS_REDIRECTION: &str = "Redirection";
pub(crate) const STATUS_CLIENT_ERROR: &str = "ClientError";
pub(crate) const STATUS_SERVER_ERROR: &str = "ServerError";
pub(crate) const STATUS_PREFIX: &str = "Status";

pub(crate) type ConversionResult<T> = anyhow::Result<T>;

pub(crate) struct SchemaConverter<'a> {
  graph: &'a SchemaGraph,
  type_resolver: TypeResolver<'a>,
  struct_converter: StructConverter<'a>,
  enum_converter: EnumConverter<'a>,
}

impl<'a> SchemaConverter<'a> {
  pub(crate) fn new(graph: &'a SchemaGraph, optionality_policy: FieldOptionalityPolicy) -> Self {
    let type_resolver = TypeResolver::new(graph);
    Self {
      graph,
      type_resolver: type_resolver.clone(),
      struct_converter: StructConverter::new(graph, type_resolver.clone(), None, optionality_policy),
      enum_converter: EnumConverter::new(graph, type_resolver),
    }
  }

  pub(crate) fn new_with_filter(
    graph: &'a SchemaGraph,
    reachable_schemas: BTreeSet<String>,
    optionality_policy: FieldOptionalityPolicy,
  ) -> Self {
    let type_resolver = TypeResolver::new(graph);
    Self {
      graph,
      type_resolver: type_resolver.clone(),
      struct_converter: StructConverter::new(
        graph,
        type_resolver.clone(),
        Some(reachable_schemas),
        optionality_policy,
      ),
      enum_converter: EnumConverter::new(graph, type_resolver),
    }
  }

  pub(crate) fn convert_schema(&self, name: &str, schema: &ObjectSchema) -> ConversionResult<Vec<RustType>> {
    if !schema.all_of.is_empty() {
      return self.struct_converter.convert_all_of_schema(name, schema);
    }

    if !schema.one_of.is_empty() {
      return self
        .enum_converter
        .convert_union_enum(name, schema, enums::UnionKind::OneOf);
    }

    if !schema.any_of.is_empty() {
      return self
        .enum_converter
        .convert_union_enum(name, schema, enums::UnionKind::AnyOf);
    }

    if !schema.enum_values.is_empty() {
      return Ok(vec![self.enum_converter.convert_simple_enum(name, schema)]);
    }

    if !schema.properties.is_empty() || schema.additional_properties.is_some() {
      let (main_type, inline_types) = self.struct_converter.convert_struct(name, schema, None)?;
      return self
        .struct_converter
        .finalize_struct_types(name, schema, main_type, inline_types);
    }

    let type_ref = self.type_resolver.schema_to_type_ref(schema)?;
    let alias = RustType::TypeAlias(TypeAliasDef {
      name: to_rust_type_name(name),
      docs: metadata::extract_docs(schema.description.as_ref()),
      target: type_ref,
    });

    Ok(vec![alias])
  }

  pub(crate) fn convert_struct(
    &self,
    name: &str,
    schema: &ObjectSchema,
    kind: Option<StructKind>,
  ) -> ConversionResult<(RustType, Vec<RustType>)> {
    self.struct_converter.convert_struct(name, schema, kind)
  }

  pub(crate) fn schema_to_type_ref(&self, schema: &ObjectSchema) -> anyhow::Result<TypeRef> {
    self.type_resolver.schema_to_type_ref(schema)
  }

  pub(crate) fn extract_validation_attrs(_prop_name: &str, is_required: bool, schema: &ObjectSchema) -> Vec<String> {
    metadata::extract_validation_attrs(is_required, schema)
  }

  pub(crate) fn extract_validation_pattern<'s>(prop_name: &str, schema: &'s ObjectSchema) -> Option<&'s String> {
    metadata::extract_validation_pattern(prop_name, schema)
  }

  pub(crate) fn extract_default_value(schema: &ObjectSchema) -> Option<serde_json::Value> {
    metadata::extract_default_value(schema)
  }

  pub(crate) fn is_schema_name(&self, name: &str) -> bool {
    self.graph.schema_names().iter().any(|schema_name| {
      let rust_schema_name = to_rust_type_name(schema_name);
      rust_schema_name == name || **schema_name == name
    })
  }
}

#[cfg(test)]
mod tests;

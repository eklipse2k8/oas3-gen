mod constants;
mod enums;
mod error;
mod metadata;
pub(crate) mod operations;
mod structs;
mod type_resolver;
mod utils;

use oas3::spec::ObjectSchema;

use self::{enums::EnumConverter, error::ConversionResult, structs::StructConverter, type_resolver::TypeResolver};
use super::{
  ast::{RustType, StructKind, TypeAliasDef, TypeRef},
  schema_graph::SchemaGraph,
};
use crate::reserved::to_rust_type_name;

pub(crate) struct SchemaConverter<'a> {
  type_resolver: TypeResolver<'a>,
  struct_converter: StructConverter<'a>,
  enum_converter: EnumConverter<'a>,
}

impl<'a> SchemaConverter<'a> {
  pub(crate) fn new(graph: &'a SchemaGraph) -> Self {
    let type_resolver = TypeResolver::new(graph);
    Self {
      type_resolver: type_resolver.clone(),
      struct_converter: StructConverter::new(graph, type_resolver.clone()),
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

    let alias = RustType::TypeAlias(TypeAliasDef {
      name: to_rust_type_name(name),
      docs: metadata::extract_docs(schema.description.as_ref()),
      target: TypeRef::new("serde_json::Value"),
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
}

#[cfg(test)]
mod tests;

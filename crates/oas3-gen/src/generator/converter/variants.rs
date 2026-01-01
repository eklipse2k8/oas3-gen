use std::sync::Arc;

use oas3::spec::ObjectSchema;

use super::{
  CodegenConfig, ConversionOutput, SchemaExt, cache::SharedSchemaCache, common::handle_inline_creation,
  structs::StructConverter, type_resolver::TypeResolver, union_types::UnionVariantSpec,
};
use crate::generator::{
  ast::{Documentation, EnumVariantToken, SerdeAttribute, TypeRef, VariantContent, VariantDef},
  converter::type_resolver::TypeResolverBuilder,
  naming::{identifiers::to_rust_type_name, inference::NormalizedVariant},
  schema_registry::SchemaRegistry,
};

#[derive(Clone, Debug)]
pub(crate) struct VariantBuilder {
  graph: Arc<SchemaRegistry>,
  type_resolver: TypeResolver,
  struct_converter: StructConverter,
}

impl VariantBuilder {
  pub(crate) fn new(graph: &Arc<SchemaRegistry>, config: &CodegenConfig) -> Self {
    let type_resolver = TypeResolverBuilder::default()
      .graph(graph.clone())
      .config(config.clone())
      .build()
      .expect("TypeResolver");
    let struct_converter = StructConverter::new(graph, config, None);
    Self {
      graph: graph.clone(),
      type_resolver,
      struct_converter,
    }
  }

  pub(crate) fn build_variant(
    &self,
    enum_name: &str,
    spec: &UnionVariantSpec,
    cache: Option<&mut SharedSchemaCache>,
  ) -> anyhow::Result<ConversionOutput<VariantDef>> {
    if let Some(ref schema_name) = spec.ref_name {
      Ok(self.build_ref_variant(schema_name, spec))
    } else {
      self.build_inline_variant(&spec.resolved_schema, enum_name, &spec.variant_name, cache)
    }
  }

  fn build_ref_variant(&self, schema_name: &str, spec: &UnionVariantSpec) -> ConversionOutput<VariantDef> {
    let rust_type_name = to_rust_type_name(schema_name);

    let type_ref = if self.graph.is_cyclic(schema_name) {
      TypeRef::new(&rust_type_name).unwrap_option().with_boxed()
    } else {
      TypeRef::new(&rust_type_name).unwrap_option()
    };

    ConversionOutput::new(
      VariantDef::builder()
        .name(spec.variant_name.clone())
        .docs(Documentation::from_optional(spec.resolved_schema.description.as_ref()))
        .content(VariantContent::Tuple(vec![type_ref]))
        .deprecated(spec.resolved_schema.deprecated.unwrap_or(false))
        .build(),
    )
  }

  fn build_inline_variant(
    &self,
    resolved_schema: &ObjectSchema,
    enum_name: &str,
    variant_name: &EnumVariantToken,
    mut cache: Option<&mut SharedSchemaCache>,
  ) -> anyhow::Result<ConversionOutput<VariantDef>> {
    let resolved_schema = if resolved_schema.has_intersection() {
      self.graph.merge_inline(resolved_schema)?
    } else {
      resolved_schema.clone()
    };

    if let Some(output) = Self::build_const_content(&resolved_schema, variant_name)? {
      return Ok(output);
    }

    let variant_label = variant_name.to_string();

    let content_output = if resolved_schema.properties.is_empty() {
      if let Some(output) =
        self.build_array_content(enum_name, &variant_label, &resolved_schema, cache.as_deref_mut())?
      {
        output
      } else if let Some(output) =
        self.build_nested_union_content(enum_name, &variant_label, &resolved_schema, cache.as_deref_mut())?
      {
        output
      } else {
        self.build_primitive_content(&resolved_schema)?
      }
    } else {
      self.build_struct_content(enum_name, &variant_label, &resolved_schema, cache)?
    };

    Ok(ConversionOutput::with_inline_types(
      VariantDef::builder()
        .name(variant_name.clone())
        .content(content_output.result)
        .docs(Documentation::from_optional(resolved_schema.description.as_ref()))
        .deprecated(resolved_schema.deprecated.unwrap_or(false))
        .build(),
      content_output.inline_types,
    ))
  }

  fn build_const_content(
    resolved_schema: &ObjectSchema,
    variant_name: &EnumVariantToken,
  ) -> anyhow::Result<Option<ConversionOutput<VariantDef>>> {
    let Some(const_value) = &resolved_schema.const_value else {
      return Ok(None);
    };

    let normalized = NormalizedVariant::try_from(const_value)
      .map_err(|_| anyhow::anyhow!("Unsupported const value type: {const_value}"))?;

    let variant = VariantDef::builder()
      .name(variant_name.clone())
      .docs(Documentation::from_optional(resolved_schema.description.as_ref()))
      .content(VariantContent::Unit)
      .serde_attrs(vec![SerdeAttribute::Rename(normalized.rename_value)])
      .deprecated(resolved_schema.deprecated.unwrap_or(false))
      .build();

    Ok(Some(ConversionOutput::new(variant)))
  }

  fn build_array_content(
    &self,
    enum_name: &str,
    variant_label: &str,
    resolved_schema: &ObjectSchema,
    cache: Option<&mut SharedSchemaCache>,
  ) -> anyhow::Result<Option<ConversionOutput<VariantContent>>> {
    if !resolved_schema.is_array() {
      return Ok(None);
    }

    let conversion =
      self
        .type_resolver
        .resolve_array_with_inline_items(enum_name, variant_label, resolved_schema, cache)?;

    Ok(conversion.map(|c| ConversionOutput::with_inline_types(VariantContent::Tuple(vec![c.result]), c.inline_types)))
  }

  fn build_nested_union_content(
    &self,
    enum_name: &str,
    variant_label: &str,
    resolved_schema: &ObjectSchema,
    cache: Option<&mut SharedSchemaCache>,
  ) -> anyhow::Result<Option<ConversionOutput<VariantContent>>> {
    if !resolved_schema.has_union() {
      return Ok(None);
    }

    let uses_one_of = !resolved_schema.one_of.is_empty();
    let result =
      self
        .type_resolver
        .resolve_inline_union_type(enum_name, variant_label, resolved_schema, uses_one_of, cache)?;

    Ok(Some(ConversionOutput::with_inline_types(
      VariantContent::Tuple(vec![result.result]),
      result.inline_types,
    )))
  }

  fn build_struct_content(
    &self,
    enum_name: &str,
    variant_label: &str,
    resolved_schema: &ObjectSchema,
    cache: Option<&mut SharedSchemaCache>,
  ) -> anyhow::Result<ConversionOutput<VariantContent>> {
    let enum_name_converted = to_rust_type_name(enum_name);
    let struct_name_prefix = format!("{enum_name_converted}{variant_label}");

    let result = handle_inline_creation(
      resolved_schema,
      &struct_name_prefix,
      None,
      cache,
      |_| None,
      |name, cache| self.struct_converter.convert_struct(name, resolved_schema, None, cache),
    )?;

    Ok(ConversionOutput::with_inline_types(
      VariantContent::Tuple(vec![result.result]),
      result.inline_types,
    ))
  }

  fn build_primitive_content(
    &self,
    resolved_schema: &ObjectSchema,
  ) -> anyhow::Result<ConversionOutput<VariantContent>> {
    let type_ref = self.type_resolver.resolve_type(resolved_schema)?.unwrap_option();
    Ok(ConversionOutput::new(VariantContent::Tuple(vec![type_ref])))
  }
}

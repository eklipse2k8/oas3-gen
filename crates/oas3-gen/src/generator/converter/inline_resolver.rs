use std::{collections::BTreeSet, rc::Rc};

use anyhow::Result;
use inflections::Inflect;
use oas3::spec::ObjectSchema;

use super::{
  ConversionOutput, SchemaExt,
  structs::StructConverter,
  union_types::UnionKind,
  unions::{EnumConverter, UnionConverter},
};
use crate::generator::{
  ast::{RustType, TypeRef},
  converter::ConverterContext,
  naming::identifiers::{strip_parent_prefix, to_rust_type_name},
};

#[derive(Debug, Clone)]
pub(crate) struct InlineTypeResolver {
  context: Rc<ConverterContext>,
}

impl InlineTypeResolver {
  pub(crate) fn new(context: Rc<ConverterContext>) -> Self {
    Self { context }
  }

  pub(crate) fn resolve_inline_struct(
    &self,
    parent_name: &str,
    property_name: &str,
    schema: &ObjectSchema,
  ) -> Result<ConversionOutput<TypeRef>> {
    let prop_pascal = property_name.to_pascal_case();
    let base_name = format!("{parent_name}{}", strip_parent_prefix(parent_name, &prop_pascal));

    self.resolve_with_cache(
      schema,
      &base_name,
      None,
      |_| None,
      |name| StructConverter::new(self.context.clone()).convert_struct(name, schema, None),
    )
  }

  pub(crate) fn resolve_inline_struct_with_name(
    &self,
    schema: &ObjectSchema,
    base_name: &str,
  ) -> Result<ConversionOutput<TypeRef>> {
    self.resolve_with_cache(
      schema,
      base_name,
      None,
      |_| None,
      |name| StructConverter::new(self.context.clone()).convert_struct(name, schema, None),
    )
  }

  pub(crate) fn resolve_inline_enum(
    &self,
    parent_name: &str,
    property_name: &str,
    schema: &ObjectSchema,
    enum_values: &[String],
  ) -> Result<ConversionOutput<TypeRef>> {
    let base_name = format!("{parent_name}{}", property_name.to_pascal_case());
    let forced_name = self.context.cache.borrow().get_enum_name(enum_values);

    self.resolve_with_cache(
      schema,
      &base_name,
      forced_name,
      |cache| {
        cache
          .get_enum_name(enum_values)
          .filter(|_| cache.is_enum_generated(enum_values))
      },
      |name| {
        let converter = EnumConverter::new(self.context.clone());
        Ok(ConversionOutput::new(converter.convert_value_enum(name, schema)))
      },
    )
  }

  pub(crate) fn resolve_inline_union(
    &self,
    schema: &ObjectSchema,
    refs: &BTreeSet<String>,
    base_name: &str,
    kind: UnionKind,
  ) -> Result<ConversionOutput<TypeRef>> {
    if let Some(name) = self.find_union_by_refs(refs) {
      return Ok(ConversionOutput::new(self.type_ref(&name)));
    }

    let discriminator = schema.discriminator.as_ref().map(|d| d.property_name.as_str());

    {
      let cache = self.context.cache.borrow();
      if refs.len() >= 2
        && let Some(name) = cache.get_union_name(refs, discriminator)
      {
        return Ok(ConversionOutput::new(TypeRef::new(name)));
      }
    }

    let result = self.resolve_with_cache(
      schema,
      base_name,
      None,
      |cache| cache.lookup_enum_name(schema),
      |name| UnionConverter::new(self.context.clone()).convert_union(name, schema, kind),
    )?;

    if refs.len() >= 2 {
      self.context.cache.borrow_mut().register_union(
        refs.clone(),
        schema.discriminator.as_ref().map(|d| d.property_name.clone()),
        result.result.base_type.to_string(),
      );
    }

    Ok(result)
  }

  pub(crate) fn resolve_inline_schema<F>(
    &self,
    schema: &ObjectSchema,
    base_name: &str,
    convert_fn: F,
  ) -> Result<Option<ConversionOutput<String>>>
  where
    F: FnOnce(&str, &ObjectSchema) -> Result<Vec<RustType>>,
  {
    if schema.is_empty_object() {
      return Ok(None);
    }

    {
      let cache = self.context.cache.borrow();
      if let Some(cached) = cache.get_type_name(schema)? {
        return Ok(Some(ConversionOutput::new(cached)));
      }
    }

    let effective = if schema.all_of.is_empty() {
      schema.clone()
    } else {
      self.context.graph().merge_all_of(schema)
    };

    let unique_name = self.context.cache.borrow_mut().make_unique_name(base_name);
    let generated = convert_fn(&unique_name, &effective)?;

    if generated.is_empty() {
      return Ok(None);
    }

    let main_type = generated.last().cloned().unwrap();
    let final_name = self
      .context
      .cache
      .borrow_mut()
      .register_type(schema, &unique_name, vec![], main_type)?;

    Ok(Some(ConversionOutput::with_inline_types(final_name, generated)))
  }

  fn resolve_with_cache<F, C>(
    &self,
    schema: &ObjectSchema,
    base_name: &str,
    forced_name: Option<String>,
    cached_name_check: C,
    generator: F,
  ) -> Result<ConversionOutput<TypeRef>>
  where
    F: FnOnce(&str) -> Result<ConversionOutput<RustType>>,
    C: FnOnce(&super::cache::SharedSchemaCache) -> Option<String>,
  {
    {
      let cache = self.context.cache.borrow();
      if let Some(existing_name) = cache.get_type_name(schema)? {
        return Ok(ConversionOutput::new(TypeRef::new(existing_name)));
      }
      if let Some(name) = cached_name_check(&cache) {
        return Ok(ConversionOutput::new(TypeRef::new(name)));
      }
    }

    let name = if let Some(forced) = forced_name {
      forced
    } else {
      self.context.cache.borrow().get_preferred_name(schema, base_name)?
    };

    let result = generator(&name)?;

    let type_name =
      self
        .context
        .cache
        .borrow_mut()
        .register_type(schema, &name, result.inline_types, result.result.clone())?;

    Ok(ConversionOutput::new(TypeRef::new(type_name)))
  }

  fn type_ref(&self, schema_name: &str) -> TypeRef {
    let mut type_ref = TypeRef::new(to_rust_type_name(schema_name));
    if self.context.graph().is_cyclic(schema_name) {
      type_ref = type_ref.with_boxed();
    }
    type_ref
  }

  fn find_union_by_refs(&self, refs: &BTreeSet<String>) -> Option<String> {
    if refs.len() < 2 {
      return None;
    }
    self.context.graph().find_union(refs).cloned()
  }
}

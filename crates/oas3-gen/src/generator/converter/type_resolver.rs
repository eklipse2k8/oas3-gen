use std::{collections::BTreeSet, rc::Rc};

use anyhow::{Context, Result};
use inflections::Inflect;
use oas3::spec::{ObjectOrReference, ObjectSchema, Schema, SchemaType, Spec};

use super::{
  ConversionOutput,
  common::extract_variant_references,
  inline_resolver::InlineTypeResolver,
  union_types::{UnionKind, entries_to_cache_key},
};
use crate::{
  generator::{
    ast::{RustPrimitive, TypeRef},
    converter::ConverterContext,
    naming::{
      constants::VARIANT_KIND_SUFFIX,
      identifiers::{strip_parent_prefix, to_rust_type_name},
      inference::CommonVariantName,
    },
    schema_registry::{RefCollector, SchemaRegistry},
  },
  utils::SchemaExt,
};

/// Resolves OpenAPI schemas into Rust type references.
///
/// This is a read-only component that maps OpenAPI schemas to Rust `TypeRef`
/// and provides navigation through the schema graph. It does not produce
/// `RustType` definitions - that is handled by `SchemaConverter`.
#[derive(Clone, Debug)]
pub(crate) struct TypeResolver {
  context: Rc<ConverterContext>,
  inline_resolver: InlineTypeResolver,
}

impl TypeResolver {
  pub(crate) fn new(context: Rc<ConverterContext>) -> Self {
    let inline_resolver = InlineTypeResolver::new(context.clone());
    Self {
      context,
      inline_resolver,
    }
  }

  fn spec(&self) -> &Spec {
    self.context.graph().spec()
  }

  /// Resolves a schema reference to its underlying schema.
  pub(crate) fn resolve(&self, schema_ref: &ObjectOrReference<ObjectSchema>) -> Result<ObjectSchema> {
    schema_ref.resolve(self.spec()).context("Schema resolution failed")
  }

  /// Creates a type reference for a named schema, applying boxing if cyclic.
  pub(crate) fn type_ref(&self, schema_name: &str) -> TypeRef {
    let mut type_ref = TypeRef::new(to_rust_type_name(schema_name));
    if self.context.graph().is_cyclic(schema_name) {
      type_ref = type_ref.with_boxed();
    }
    type_ref
  }

  /// Resolves a schema to its Rust type reference.
  pub(crate) fn resolve_type(&self, schema: &ObjectSchema) -> Result<TypeRef> {
    let cached = self.context.cache.borrow().get_type_ref(schema)?;
    if let Some(type_ref) = cached {
      return Ok(type_ref);
    }

    let type_ref = self.resolve_type_uncached(schema)?;

    if schema.is_primitive() {
      let _ = self
        .context
        .cache
        .borrow_mut()
        .register_type_ref(schema, type_ref.clone());
    }

    Ok(type_ref)
  }

  fn resolve_type_uncached(&self, schema: &ObjectSchema) -> Result<TypeRef> {
    if let Some(type_ref) = self.try_type_ref_by_title(schema) {
      return Ok(type_ref);
    }

    let union_variants = if !schema.one_of.is_empty() {
      Some(&schema.one_of)
    } else if schema.has_intersection() {
      Some(&schema.any_of)
    } else {
      None
    };

    if let Some(variants) = union_variants
      && let Some(type_ref) = self.try_union(variants)?
    {
      return Ok(type_ref);
    }

    if let Some(typ) = schema.single_type() {
      return self.primitive(typ, schema);
    }

    if let Some(non_null) = schema.non_null_type() {
      return Ok(self.primitive(non_null, schema)?.with_option());
    }

    if let Some(ref const_value) = schema.const_value {
      return Ok(const_value.into());
    }

    Ok(TypeRef::new(RustPrimitive::Value))
  }

  /// Resolves a property schema to its Rust type reference with inline type tracking.
  pub(crate) fn resolve_property(
    &self,
    parent_name: &str,
    property_name: &str,
    schema: &ObjectSchema,
    schema_ref: &ObjectOrReference<ObjectSchema>,
  ) -> Result<ConversionOutput<TypeRef>> {
    if let ObjectOrReference::Ref { ref_path, .. } = schema_ref {
      return self.resolve_ref(ref_path, schema);
    }

    if schema.all_of.len() == 1
      && let Some(type_ref) = self.try_union(&schema.all_of)?
    {
      return Ok(ConversionOutput::new(type_ref));
    }

    if schema.is_inline_object() {
      return self
        .inline_resolver
        .resolve_inline_struct(parent_name, property_name, schema);
    }

    if schema.has_enum_values() {
      return self.inline_enum(parent_name, property_name, schema);
    }

    if schema.has_union() {
      return self.inline_union(parent_name, property_name, schema);
    }

    if schema.is_array()
      && let Some(result) = self.try_inline_array(parent_name, property_name, schema)?
    {
      return Ok(result);
    }

    Ok(ConversionOutput::new(self.resolve_type(schema)?))
  }

  fn resolve_ref(&self, ref_path: &str, schema: &ObjectSchema) -> Result<ConversionOutput<TypeRef>> {
    let ref_name =
      SchemaRegistry::parse_ref(ref_path).ok_or_else(|| anyhow::anyhow!("Invalid reference path: {ref_path}"))?;

    if schema.is_primitive() && !schema.has_inline_union_array_items(self.spec()) {
      return Ok(ConversionOutput::new(self.resolve_type(schema)?));
    }

    let Some((variants, _)) = schema.union_variants_with_kind() else {
      return Ok(ConversionOutput::new(self.type_ref(&ref_name)));
    };

    if self.is_wrapper_union(schema)?
      && let Some(type_ref) = self.try_union(variants)?
    {
      return Ok(ConversionOutput::new(type_ref));
    }

    Ok(ConversionOutput::new(self.type_ref(&ref_name)))
  }

  fn inline_enum(
    &self,
    parent_name: &str,
    property_name: &str,
    schema: &ObjectSchema,
  ) -> Result<ConversionOutput<TypeRef>> {
    if schema.enum_values.len() == 1 {
      return Ok(ConversionOutput::new(self.resolve_type(schema)?));
    }

    let enum_values = entries_to_cache_key(&schema.extract_enum_entries(self.spec()));
    self
      .inline_resolver
      .resolve_inline_enum(parent_name, property_name, schema, &enum_values)
  }

  pub(crate) fn inline_struct_from_schema(
    &self,
    schema: &ObjectSchema,
    base_name: &str,
  ) -> Result<ConversionOutput<TypeRef>> {
    self.inline_resolver.resolve_inline_struct_with_name(schema, base_name)
  }

  pub(crate) fn inline_union(
    &self,
    parent_name: &str,
    property_name: &str,
    schema: &ObjectSchema,
  ) -> Result<ConversionOutput<TypeRef>> {
    let (variants, _) = schema.union_variants_with_kind().unwrap();

    if let Some(type_ref) = self.try_nullable_union(schema)? {
      return Ok(ConversionOutput::new(type_ref));
    }

    if let Some(result) = self.try_inline_array(parent_name, property_name, schema)? {
      return Ok(result);
    }

    let property_pascal = property_name.to_pascal_case();
    let suffix = format!("{property_pascal}{VARIANT_KIND_SUFFIX}");
    let base_name = CommonVariantName::union_name_or(variants, &suffix, || format!("{parent_name}{property_pascal}"));

    self.union_type(schema, variants, &base_name)
  }

  fn union_type(
    &self,
    schema: &ObjectSchema,
    variants: &[ObjectOrReference<ObjectSchema>],
    base_name: &str,
  ) -> Result<ConversionOutput<TypeRef>> {
    let refs = extract_variant_references(variants);
    let kind = if schema.one_of.is_empty() {
      UnionKind::AnyOf
    } else {
      UnionKind::OneOf
    };
    self
      .inline_resolver
      .resolve_inline_union(schema, &refs, base_name, kind)
  }

  pub(crate) fn try_inline_array(
    &self,
    parent_name: &str,
    property_name: &str,
    schema: &ObjectSchema,
  ) -> Result<Option<ConversionOutput<TypeRef>>> {
    let Some(items) = schema.inline_array_items(self.spec()) else {
      return Ok(None);
    };

    let unique = schema.unique_items.unwrap_or(false);
    let singular = cruet::to_singular(property_name).to_pascal_case();

    let result = if items.is_inline_object() {
      let base = format!("{parent_name}{}", strip_parent_prefix(parent_name, &singular));
      self.inline_struct_from_schema(&items, &base)?
    } else if items.has_union() {
      self.inline_union_array_item(&items, &singular)?
    } else if items.has_enum_values() {
      self.inline_enum(parent_name, &singular, &items)?
    } else {
      return Ok(None);
    };

    let vec_type = TypeRef {
      boxed: false,
      ..result.result
    }
    .with_vec()
    .with_unique_items(unique);

    Ok(Some(ConversionOutput::with_inline_types(vec_type, result.inline_types)))
  }

  fn inline_union_array_item(&self, items: &ObjectSchema, singular: &str) -> Result<ConversionOutput<TypeRef>> {
    let (variants, _) = items.union_variants_with_kind().unwrap();
    let kind_name = format!("{singular}{VARIANT_KIND_SUFFIX}");
    let name = CommonVariantName::union_name_or(variants, &kind_name, || kind_name.clone());

    let final_name = {
      let cache = self.context.cache.borrow();
      if cache.name_conflicts_with_different_schema(&name, items)? {
        cache.make_unique_name(&name)
      } else {
        name
      }
    };

    self.union_type(items, variants, &final_name)
  }

  fn try_type_ref_by_title(&self, schema: &ObjectSchema) -> Option<TypeRef> {
    let title = schema.title.as_ref()?;
    if schema.schema_type.is_some() {
      return None;
    }
    self.context.graph().get(title)?;
    Some(self.type_ref(title))
  }

  fn find_union_by_refs(&self, refs: &BTreeSet<String>) -> Option<String> {
    if refs.len() < 2 {
      return None;
    }
    self.context.graph().find_union(refs).cloned()
  }

  pub(crate) fn try_nullable_union(&self, schema: &ObjectSchema) -> Result<Option<TypeRef>> {
    let Some((variants, _)) = schema.union_variants_with_kind() else {
      return Ok(None);
    };

    if variants.len() != 2 {
      return Ok(None);
    }

    let spec = self.spec();
    if !schema.has_null_variant(spec) {
      return Ok(None);
    }

    let Some(variant) = schema.find_non_null_variant(spec) else {
      return Ok(None);
    };

    if let Some(ref_name) = RefCollector::parse_schema_ref(variant) {
      return Ok(Some(self.type_ref(&ref_name).with_option()));
    }

    let resolved = self.resolve(variant)?;

    if resolved.is_array() && self.has_union_items(&resolved) {
      return Ok(None);
    }

    Ok(Some(self.resolve_type(&resolved)?.with_option()))
  }

  fn has_union_items(&self, schema: &ObjectSchema) -> bool {
    let Some(Schema::Object(o)) = schema.items.as_ref().map(std::convert::AsRef::as_ref) else {
      return false;
    };
    self.resolve(o).is_ok_and(|items| items.has_union())
  }

  pub(crate) fn try_union(&self, variants: &[ObjectOrReference<ObjectSchema>]) -> Result<Option<TypeRef>> {
    let refs = extract_variant_references(variants);

    if let Some(name) = self.find_union_by_refs(&refs) {
      return Ok(Some(self.type_ref(&name)));
    }

    let temp_schema = ObjectSchema {
      one_of: variants.to_vec(),
      ..Default::default()
    };
    if let Some(nullable) = self.try_nullable_union(&temp_schema)? {
      return Ok(Some(nullable));
    }

    self.union_fallback(variants)
  }

  fn union_fallback(&self, variants: &[ObjectOrReference<ObjectSchema>]) -> Result<Option<TypeRef>> {
    let mut first_ref: Option<String> = None;

    for variant in variants {
      if let Some(name) = RefCollector::parse_schema_ref(variant) {
        if first_ref.is_some() {
          return Ok(None);
        }
        first_ref = Some(name);
        continue;
      }

      let Ok(resolved) = self.resolve(variant) else {
        continue;
      };

      if resolved.is_null() {
        continue;
      }

      if let Some(type_ref) = self.try_simple_fallback_type(&resolved)? {
        return Ok(Some(type_ref));
      }
    }

    Ok(first_ref.map(|name| self.type_ref(&name)))
  }

  fn try_simple_fallback_type(&self, schema: &ObjectSchema) -> Result<Option<TypeRef>> {
    if schema.is_array() {
      let item = self.array_item_type(schema)?;
      let unique = schema.unique_items.unwrap_or(false);
      return Ok(Some(
        TypeRef::new(item.to_rust_type()).with_vec().with_unique_items(unique),
      ));
    }

    if schema.is_string() {
      return Ok(Some(TypeRef::new(RustPrimitive::String)));
    }

    if let [single_variant] = schema.one_of.as_slice()
      && let Some(name) = RefCollector::parse_schema_ref(single_variant)
    {
      return Ok(Some(self.type_ref(&name)));
    }

    if let Some(ref title) = schema.title
      && self.context.graph().get(title).is_some()
    {
      return Ok(Some(self.type_ref(title)));
    }

    Ok(None)
  }

  fn primitive(&self, typ: SchemaType, schema: &ObjectSchema) -> Result<TypeRef> {
    match typ {
      SchemaType::String | SchemaType::Number | SchemaType::Integer => {
        Ok(TypeRef::new(Self::format_or_default(typ, schema)))
      }
      SchemaType::Boolean => Ok(TypeRef::new(RustPrimitive::Bool)),
      SchemaType::Object => {
        if let Some(map) = self.try_map_type(schema)? {
          return Ok(map);
        }
        Ok(TypeRef::new(RustPrimitive::Value))
      }
      SchemaType::Null => Ok(TypeRef::new(RustPrimitive::Unit).with_option()),
      SchemaType::Array => {
        let item = self.array_item_type(schema)?;
        let unique = schema.unique_items.unwrap_or(false);
        Ok(TypeRef::new(item.to_rust_type()).with_vec().with_unique_items(unique))
      }
    }
  }

  fn format_or_default(typ: SchemaType, schema: &ObjectSchema) -> RustPrimitive {
    let default = match typ {
      SchemaType::String => RustPrimitive::String,
      SchemaType::Number => RustPrimitive::F64,
      SchemaType::Integer => RustPrimitive::I64,
      _ => return RustPrimitive::Value,
    };
    schema
      .format
      .as_ref()
      .and_then(|f| RustPrimitive::from_format(f))
      .unwrap_or(default)
  }

  fn try_map_type(&self, schema: &ObjectSchema) -> Result<Option<TypeRef>> {
    let Some(ref additional) = schema.additional_properties else {
      return Ok(None);
    };

    if matches!(additional, Schema::Boolean(b) if !b.0) {
      return Ok(None);
    }

    if !schema.properties.is_empty() {
      return Ok(None);
    }

    let value = self.additional_properties_type(additional)?;
    Ok(Some(TypeRef::new(format!(
      "std::collections::HashMap<String, {}>",
      value.to_rust_type()
    ))))
  }

  pub(crate) fn additional_properties_type(&self, additional: &Schema) -> Result<TypeRef> {
    match additional {
      Schema::Boolean(_) => Ok(TypeRef::new(RustPrimitive::Value)),
      Schema::Object(schema_ref) => {
        if let ObjectOrReference::Ref { ref_path, .. } = &**schema_ref {
          let name =
            SchemaRegistry::parse_ref(ref_path).ok_or_else(|| anyhow::anyhow!("Invalid reference: {ref_path}"))?;
          return Ok(self.type_ref(&name));
        }

        let resolved = self.resolve(schema_ref)?;

        if resolved.is_empty_object() {
          return Ok(TypeRef::new(RustPrimitive::Value));
        }

        self.resolve_type(&resolved)
      }
    }
  }

  fn array_item_type(&self, schema: &ObjectSchema) -> Result<TypeRef> {
    let Some(Schema::Object(items_ref)) = schema.items.as_ref().map(std::convert::AsRef::as_ref) else {
      return Ok(TypeRef::new(RustPrimitive::Value));
    };

    let items = self.resolve(items_ref)?;

    let type_ref = match &**items_ref {
      ObjectOrReference::Ref { ref_path, .. } => self.resolve_ref(ref_path, &items)?.result,
      ObjectOrReference::Object(_) => self.resolve_type(&items)?,
    };

    Ok(TypeRef {
      boxed: false,
      ..type_ref
    })
  }

  pub(crate) fn is_wrapper_union(&self, schema: &ObjectSchema) -> Result<bool> {
    let spec = self.spec();

    let Some(variant) = schema.single_non_null_variant(spec) else {
      return Ok(false);
    };

    if RefCollector::parse_schema_ref(variant).is_some() {
      return Ok(true);
    }

    let resolved = self.resolve(variant)?;

    if resolved.has_union() || resolved.additional_properties.is_some() {
      return Ok(false);
    }

    Ok(resolved.is_primitive())
  }

  pub(crate) fn try_flatten_nested_union(&self, outer: &ObjectSchema) -> Result<Option<ObjectSchema>> {
    let spec = self.spec();

    if !outer.has_inline_single_variant(spec) {
      return Ok(None);
    }

    let variant = outer.single_non_null_variant(spec).unwrap();
    let inner = self.resolve(variant)?;

    if !inner.has_union() {
      return Ok(None);
    }

    let (inner_variants, _) = inner.union_variants_with_kind().unwrap();
    let variants_vec = inner_variants.to_vec();
    let is_one_of = !inner.one_of.is_empty();

    Ok(Some(ObjectSchema {
      description: outer.description.clone().or_else(|| inner.description.clone()),
      discriminator: inner.discriminator.clone().or_else(|| outer.discriminator.clone()),
      one_of: if is_one_of { variants_vec.clone() } else { vec![] },
      any_of: if is_one_of { vec![] } else { variants_vec },
      ..Default::default()
    }))
  }
}

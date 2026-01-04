use std::{collections::BTreeSet, rc::Rc};

use anyhow::{Context, Result};
use inflections::Inflect;
use oas3::spec::{ObjectOrReference, ObjectSchema, Schema, SchemaType, Spec};

use super::{
  ConversionOutput, SchemaExt,
  common::{InlineSchemaOutput, extract_variant_references},
  discriminator::DiscriminatorConverter,
  structs::StructConverter,
  union_types::UnionKind,
  unions::{EnumConverter, UnionConverter},
};
use crate::generator::{
  ast::{Documentation, RustPrimitive, RustType, TypeAliasDef, TypeAliasToken, TypeRef},
  converter::{ConverterContext, common::handle_inline_creation},
  naming::{
    constants::VARIANT_KIND_SUFFIX,
    identifiers::{strip_parent_prefix, to_rust_type_name},
    inference::{CommonVariantName, InferenceExt},
  },
  schema_registry::{RefCollector, SchemaRegistry},
};

/// Resolves OpenAPI schemas into Rust type references.
#[derive(Clone, Debug)]
pub(crate) struct TypeResolver {
  context: Rc<ConverterContext>,
}

impl TypeResolver {
  pub(crate) fn new(context: Rc<ConverterContext>) -> Self {
    Self { context }
  }

  fn spec(&self) -> &Spec {
    self.context.graph().spec()
  }

  fn resolve(&self, schema_ref: &ObjectOrReference<ObjectSchema>) -> Result<ObjectSchema> {
    schema_ref.resolve(self.spec()).context("Schema resolution failed")
  }

  fn type_ref(&self, schema_name: &str) -> TypeRef {
    let mut type_ref = TypeRef::new(to_rust_type_name(schema_name));
    if self.context.graph().is_cyclic(schema_name) {
      type_ref = type_ref.with_boxed();
    }
    type_ref
  }

  fn union_variants(schema: &ObjectSchema) -> Option<(&[ObjectOrReference<ObjectSchema>], UnionKind)> {
    let variants = if !schema.one_of.is_empty() {
      &schema.one_of
    } else if !schema.any_of.is_empty() {
      &schema.any_of
    } else {
      return None;
    };
    Some((variants, UnionKind::from_schema(schema)))
  }

  fn count_non_null(&self, variants: &[ObjectOrReference<ObjectSchema>]) -> usize {
    variants
      .iter()
      .filter(|v| {
        self
          .resolve(v)
          .map(|s| !s.is_null() && !s.is_nullable_object())
          .unwrap_or(true)
      })
      .count()
  }

  fn find_non_null_variant<'a>(
    &self,
    variants: &'a [ObjectOrReference<ObjectSchema>],
  ) -> Result<Option<&'a ObjectOrReference<ObjectSchema>>> {
    variants
      .iter()
      .find(|v| self.resolve(v).is_ok_and(|s| !s.is_nullable_object()))
      .map_or(Ok(None), |v| Ok(Some(v)))
  }

  fn has_null_variant(&self, variants: &[ObjectOrReference<ObjectSchema>]) -> Result<bool> {
    for variant in variants {
      if self.resolve(variant)?.is_nullable_object() {
        return Ok(true);
      }
    }
    Ok(false)
  }

  pub(crate) fn resolve_type(&self, schema: &ObjectSchema) -> Result<TypeRef> {
    if let Some(type_ref) = self.try_type_ref_by_title(schema) {
      return Ok(type_ref);
    }

    if !schema.one_of.is_empty() {
      if let Some(type_ref) = self.try_union(&schema.one_of)? {
        return Ok(type_ref);
      }
    } else if schema.has_intersection()
      && let Some(type_ref) = self.try_union(&schema.any_of)?
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
      return self.inline_struct(parent_name, property_name, schema);
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

    let Some((variants, _)) = Self::union_variants(schema) else {
      return Ok(ConversionOutput::new(self.type_ref(&ref_name)));
    };

    if self.is_wrapper_union(variants)?
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

    let enum_values: Vec<String> = schema.extract_enum_values().unwrap_or_default();
    let base_name = format!("{parent_name}{}", property_name.to_pascal_case());
    let forced_name = self.context.cache.borrow().get_enum_name(&enum_values);

    handle_inline_creation(
      schema,
      &base_name,
      forced_name,
      &self.context,
      |cache| {
        cache
          .get_enum_name(&enum_values)
          .filter(|_| cache.is_enum_generated(&enum_values))
      },
      |name| {
        let converter = EnumConverter::new(self.context.clone());
        Ok(ConversionOutput::new(converter.convert_value_enum(name, schema)))
      },
    )
  }

  fn inline_struct(
    &self,
    parent_name: &str,
    property_name: &str,
    schema: &ObjectSchema,
  ) -> Result<ConversionOutput<TypeRef>> {
    let prop_pascal = property_name.to_pascal_case();
    let base_name = format!("{parent_name}{}", strip_parent_prefix(parent_name, &prop_pascal));
    self.inline_struct_from_schema(schema, &base_name)
  }

  fn inline_struct_from_schema(&self, schema: &ObjectSchema, base_name: &str) -> Result<ConversionOutput<TypeRef>> {
    handle_inline_creation(
      schema,
      base_name,
      None,
      &self.context,
      |_| None,
      |name| StructConverter::new(self.context.clone()).convert_struct(name, schema, None),
    )
  }

  pub(crate) fn inline_union(
    &self,
    parent_name: &str,
    property_name: &str,
    schema: &ObjectSchema,
  ) -> Result<ConversionOutput<TypeRef>> {
    let (variants, _) = Self::union_variants(schema).unwrap();

    if let Some(type_ref) = self.try_nullable_union(variants)? {
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

    if let Some(name) = self.find_union_by_refs(&refs) {
      return Ok(ConversionOutput::new(self.type_ref(&name)));
    }

    let discriminator = schema.discriminator.as_ref().map(|d| d.property_name.as_str());

    {
      let cache = self.context.cache.borrow();
      if refs.len() >= 2
        && let Some(name) = cache.get_union_name(&refs, discriminator)
      {
        return Ok(ConversionOutput::new(TypeRef::new(name)));
      }
    }

    let kind = if schema.one_of.is_empty() {
      UnionKind::AnyOf
    } else {
      UnionKind::OneOf
    };

    let result = handle_inline_creation(
      schema,
      base_name,
      None,
      &self.context,
      |cache| cache.lookup_enum_name(schema),
      |name| UnionConverter::new(self.context.clone()).convert_union(name, schema, kind),
    )?;

    if refs.len() >= 2 {
      self.context.cache.borrow_mut().register_union(
        refs,
        schema.discriminator.as_ref().map(|d| d.property_name.clone()),
        result.result.base_type.to_string(),
      );
    }

    Ok(result)
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
      let (variants, _) = Self::union_variants(&items).unwrap();
      let kind_name = format!("{singular}{VARIANT_KIND_SUFFIX}");
      let name = CommonVariantName::union_name_or(variants, &kind_name, || kind_name.clone());

      let final_name = {
        let cache = self.context.cache.borrow();
        if cache.name_conflicts_with_different_schema(&name, &items)? {
          cache.make_unique_name(&name)
        } else {
          name
        }
      };

      self.union_type(&items, variants, &final_name)?
    } else {
      return Ok(None);
    };

    let mut type_ref = result.result;
    type_ref.boxed = false;
    let vec_type = type_ref.with_vec().with_unique_items(unique);

    Ok(Some(ConversionOutput::with_inline_types(vec_type, result.inline_types)))
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

  fn try_nullable_union(&self, variants: &[ObjectOrReference<ObjectSchema>]) -> Result<Option<TypeRef>> {
    if variants.len() != 2 {
      return Ok(None);
    }

    let non_null = self.find_non_null_variant(variants)?;
    let has_null = self.has_null_variant(variants)?;

    if !has_null {
      return Ok(None);
    }

    let Some(variant) = non_null else {
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
    schema
      .items
      .as_ref()
      .and_then(|b| match b.as_ref() {
        Schema::Object(o) => self.resolve(o).ok(),
        Schema::Boolean(_) => None,
      })
      .is_some_and(|items| items.has_union())
  }

  pub(crate) fn try_union(&self, variants: &[ObjectOrReference<ObjectSchema>]) -> Result<Option<TypeRef>> {
    let refs = extract_variant_references(variants);

    if let Some(name) = self.find_union_by_refs(&refs) {
      return Ok(Some(self.type_ref(&name)));
    }

    if let Some(nullable) = self.try_nullable_union(variants)? {
      return Ok(Some(nullable));
    }

    self.union_fallback(variants)
  }

  fn union_fallback(&self, variants: &[ObjectOrReference<ObjectSchema>]) -> Result<Option<TypeRef>> {
    let mut ref_count = 0;
    let mut first_ref: Option<String> = None;

    for variant in variants {
      if let Some(name) = RefCollector::parse_schema_ref(variant) {
        ref_count += 1;
        if ref_count >= 2 {
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

      if resolved.is_array() {
        let item = self.array_item_type(&resolved)?;
        let unique = resolved.unique_items.unwrap_or(false);
        return Ok(Some(
          TypeRef::new(item.to_rust_type()).with_vec().with_unique_items(unique),
        ));
      }

      if resolved.is_string() {
        return Ok(Some(TypeRef::new(RustPrimitive::String)));
      }

      if resolved.one_of.len() == 1
        && let Some(name) = RefCollector::parse_schema_ref(&resolved.one_of[0])
      {
        return Ok(Some(self.type_ref(&name)));
      }

      if let Some(ref title) = resolved.title
        && self.context.graph().get(title).is_some()
      {
        return Ok(Some(self.type_ref(title)));
      }
    }

    Ok(first_ref.map(|name| self.type_ref(&name)))
  }

  fn primitive(&self, typ: SchemaType, schema: &ObjectSchema) -> Result<TypeRef> {
    let prim = match typ {
      SchemaType::String | SchemaType::Number | SchemaType::Integer => {
        let default = match typ {
          SchemaType::String => RustPrimitive::String,
          SchemaType::Number => RustPrimitive::F64,
          SchemaType::Integer => RustPrimitive::I64,
          _ => unreachable!(),
        };
        schema
          .format
          .as_ref()
          .and_then(|f| RustPrimitive::from_format(f))
          .unwrap_or(default)
      }
      SchemaType::Boolean => RustPrimitive::Bool,
      SchemaType::Object => {
        if let Some(map) = self.try_map_type(schema)? {
          return Ok(map);
        }
        RustPrimitive::Value
      }
      SchemaType::Null => RustPrimitive::Unit,
      SchemaType::Array => {
        let item = self.array_item_type(schema)?;
        let unique = schema.unique_items.unwrap_or(false);
        return Ok(TypeRef::new(item.to_rust_type()).with_vec().with_unique_items(unique));
      }
    };

    let mut type_ref = TypeRef::new(prim);
    if typ == SchemaType::Null {
      type_ref = type_ref.with_option();
    }
    Ok(type_ref)
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
    let Some(items_ref) = schema.items.as_ref().and_then(|b| match b.as_ref() {
      Schema::Object(o) => Some(o),
      Schema::Boolean(_) => None,
    }) else {
      return Ok(TypeRef::new(RustPrimitive::Value));
    };

    let items = self.resolve(items_ref)?;

    if let ObjectOrReference::Ref { ref_path, .. } = &**items_ref {
      let mut type_ref = self.resolve_ref(ref_path, &items)?.result;
      type_ref.boxed = false;
      return Ok(type_ref);
    }

    let mut type_ref = self.resolve_type(&items)?;
    type_ref.boxed = false;
    Ok(type_ref)
  }

  pub(crate) fn discriminated_enum(&self, name: &str, schema: &ObjectSchema, fallback_type: &str) -> Result<RustType> {
    DiscriminatorConverter::new(self.context.clone()).build_enum(name, schema, fallback_type)
  }

  pub(crate) fn try_inline_schema(&self, schema: &ObjectSchema, base_name: &str) -> Result<Option<InlineSchemaOutput>> {
    if schema.is_empty_object() {
      return Ok(None);
    }

    {
      let cache = self.context.cache.borrow();
      if let Some(cached) = cache.get_type_name(schema)? {
        return Ok(Some(InlineSchemaOutput {
          type_name: cached,
          generated_types: vec![],
        }));
      }
    }

    let effective = if schema.all_of.is_empty() {
      schema.clone()
    } else {
      self.context.graph().merge_all_of(schema)
    };

    let unique_name = self.context.cache.borrow_mut().make_unique_name(base_name);
    let generated = self.convert_schema(&unique_name, &effective)?;

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

  pub(crate) fn convert_schema(&self, name: &str, schema: &ObjectSchema) -> Result<Vec<RustType>> {
    let struct_conv = StructConverter::new(self.context.clone());
    let enum_conv = EnumConverter::new(self.context.clone());
    let union_conv = UnionConverter::new(self.context.clone());

    if schema.has_intersection() {
      return struct_conv.convert_all_of_schema(name);
    }

    if let Some((variants, kind)) = Self::union_variants(schema) {
      if schema.discriminator.is_none() && self.is_wrapper_union(variants)? {
        return Ok(vec![]);
      }

      if let Some(flattened) = self.try_flatten_nested_union(schema, variants)? {
        return union_conv
          .convert_union(name, &flattened, UnionKind::from_schema(&flattened))
          .map(ConversionOutput::into_vec);
      }

      return union_conv
        .convert_union(name, schema, kind)
        .map(ConversionOutput::into_vec);
    }

    if !schema.enum_values.is_empty() {
      return Ok(vec![enum_conv.convert_value_enum(name, schema)]);
    }

    if !schema.properties.is_empty() || schema.additional_properties.is_some() {
      let result = struct_conv.convert_struct(name, schema, None)?;
      return struct_conv.finalize_struct_types(name, schema, result.result, result.inline_types);
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

    let type_ref = self.resolve_type(schema)?;
    Ok(vec![RustType::TypeAlias(TypeAliasDef {
      name: TypeAliasToken::from_raw(name),
      docs: Documentation::from_optional(schema.description.as_ref()),
      target: type_ref,
    })])
  }

  fn try_array_alias(&self, name: &str, schema: &ObjectSchema) -> Result<Option<ConversionOutput<TypeRef>>> {
    if !schema.is_array() && !schema.is_nullable_array() {
      return Ok(None);
    }

    if let Some(output) = self.try_inline_array(name, name, schema)? {
      let type_ref = if schema.is_nullable_array() {
        output.result.with_option()
      } else {
        output.result
      };
      return Ok(Some(ConversionOutput::with_inline_types(type_ref, output.inline_types)));
    }

    Ok(None)
  }

  fn is_wrapper_union(&self, variants: &[ObjectOrReference<ObjectSchema>]) -> Result<bool> {
    if self.count_non_null(variants) != 1 {
      return Ok(false);
    }

    let Some(variant) = self.find_non_null_variant(variants)? else {
      return Ok(false);
    };

    let resolved = self.resolve(variant)?;

    if resolved.has_union() {
      return Ok(false);
    }

    if RefCollector::parse_schema_ref(variant).is_some() {
      return Ok(true);
    }

    if resolved.additional_properties.is_some() {
      return Ok(false);
    }

    Ok(resolved.is_primitive())
  }

  fn try_flatten_nested_union(
    &self,
    outer: &ObjectSchema,
    variants: &[ObjectOrReference<ObjectSchema>],
  ) -> Result<Option<ObjectSchema>> {
    if self.count_non_null(variants) != 1 {
      return Ok(None);
    }

    let Some(variant) = self.find_non_null_variant(variants)? else {
      return Ok(None);
    };

    if RefCollector::parse_schema_ref(variant).is_some() {
      return Ok(None);
    }

    let inner = self.resolve(variant)?;

    if !inner.has_union() {
      return Ok(None);
    }

    let (inner_variants, _) = Self::union_variants(&inner).unwrap();

    Ok(Some(ObjectSchema {
      description: outer.description.clone().or_else(|| inner.description.clone()),
      discriminator: inner.discriminator.clone().or_else(|| outer.discriminator.clone()),
      one_of: if inner.one_of.is_empty() {
        vec![]
      } else {
        inner_variants.to_vec()
      },
      any_of: if inner.one_of.is_empty() {
        inner_variants.to_vec()
      } else {
        vec![]
      },
      ..Default::default()
    }))
  }
}

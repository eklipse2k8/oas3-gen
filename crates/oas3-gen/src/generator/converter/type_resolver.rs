use std::{collections::BTreeSet, rc::Rc};

use anyhow::{Context, Result};
use inflections::Inflect;
use oas3::spec::{ObjectOrReference, ObjectSchema, Schema, SchemaType};

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
    identifiers::{strip_parent_prefix, to_rust_type_name},
    inference::{CommonVariantName, InferenceExt},
  },
  schema_registry::{RefCollector, SchemaRegistry},
};

/// Resolves OpenAPI schemas into Rust Type References (`TypeRef`).
#[derive(Clone, Debug)]
pub(crate) struct TypeResolver {
  context: Rc<ConverterContext>,
}

impl TypeResolver {
  pub(crate) fn new(context: Rc<ConverterContext>) -> Self {
    Self { context }
  }

  fn create_type_reference(&self, schema_name: &str) -> TypeRef {
    let mut type_reference = TypeRef::new(to_rust_type_name(schema_name));
    if self.context.graph().is_cyclic(schema_name) {
      type_reference = type_reference.with_boxed();
    }
    type_reference
  }

  /// Resolves a schema to a `TypeRef`, potentially wrapping it in `Option` or `Vec`.
  pub(crate) fn resolve_type(&self, schema: &ObjectSchema) -> anyhow::Result<TypeRef> {
    if let Some(type_ref) = self.resolve_by_title(schema) {
      return Ok(type_ref);
    }

    if !schema.one_of.is_empty() {
      if let Some(type_ref) = self.resolve_union(&schema.one_of)? {
        return Ok(type_ref);
      }
    } else if schema.has_intersection()
      && let Some(type_ref) = self.resolve_union(&schema.any_of)?
    {
      return Ok(type_ref);
    }

    if let Some(typ) = schema.single_type() {
      return self.resolve_primitive(typ, schema);
    }
    if let Some(non_null) = schema.non_null_type() {
      return Ok(self.resolve_primitive(non_null, schema)?.with_option());
    }
    if let Some(ref const_value) = schema.const_value {
      return Ok(const_value.into());
    }

    Ok(TypeRef::new(RustPrimitive::Value))
  }

  /// Resolves a property type, handling inline structs/enums/unions by generating them.
  pub(crate) fn resolve_property_type(
    &self,
    parent_type_name: &str,
    property_name: &str,
    property_schema: &ObjectSchema,
    property_schema_ref: &ObjectOrReference<ObjectSchema>,
  ) -> anyhow::Result<ConversionOutput<TypeRef>> {
    if let ObjectOrReference::Ref { ref_path, .. } = property_schema_ref {
      return self.resolve_reference(ref_path, property_schema);
    }

    if property_schema.all_of.len() == 1
      && let Some(type_ref) = self.resolve_union(&property_schema.all_of)?
    {
      return Ok(ConversionOutput::new(type_ref));
    }

    if property_schema.is_inline_object() {
      return self.resolve_inline_struct(parent_type_name, property_name, property_schema);
    }

    if property_schema.has_enum_values() {
      return self.resolve_inline_enum(parent_type_name, property_name, property_schema);
    }

    if property_schema.has_union() {
      let has_one_of = !property_schema.one_of.is_empty();
      return self.resolve_inline_union_type(parent_type_name, property_name, property_schema, has_one_of);
    }

    if property_schema.is_array()
      && let Some(result) = self.resolve_array_with_inline_items(parent_type_name, property_name, property_schema)?
    {
      return Ok(result);
    }

    Ok(ConversionOutput::new(self.resolve_type(property_schema)?))
  }

  /// Resolves a schema reference to its corresponding Rust type.
  ///
  /// If the reference points to a primitive type, returns the primitive directly.
  /// Otherwise, returns the named type (with Box wrapper if cyclic).
  fn resolve_reference(
    &self,
    reference_path: &str,
    resolved_schema: &ObjectSchema,
  ) -> anyhow::Result<ConversionOutput<TypeRef>> {
    let reference_name = SchemaRegistry::parse_ref(reference_path)
      .ok_or_else(|| anyhow::anyhow!("Invalid reference path: {reference_path}"))?;

    let is_complex_array = resolved_schema.has_inline_union_array_items(self.context.graph().spec());

    if resolved_schema.is_primitive() && !is_complex_array {
      Ok(ConversionOutput::new(self.resolve_type(resolved_schema)?))
    } else {
      Ok(ConversionOutput::new(self.create_type_reference(&reference_name)))
    }
  }

  fn resolve_inline_enum(
    &self,
    parent_name: &str,
    property_name: &str,
    property_schema: &ObjectSchema,
  ) -> anyhow::Result<ConversionOutput<TypeRef>> {
    if property_schema.enum_values.len() == 1 {
      return Ok(ConversionOutput::new(self.resolve_type(property_schema)?));
    }

    let enum_values: Vec<String> = property_schema.extract_enum_values().unwrap_or_default();
    let base_name = format!("{parent_name}{}", property_name.to_pascal_case());

    let forced_name = self.context.cache.borrow().get_enum_name(&enum_values);

    handle_inline_creation(
      property_schema,
      &base_name,
      forced_name,
      &self.context,
      |cache| {
        if let Some(name) = cache.get_enum_name(&enum_values)
          && cache.is_enum_generated(&enum_values)
        {
          Some(name)
        } else {
          None
        }
      },
      |name| {
        let converter = EnumConverter::new(self.context.clone());
        let inline_enum = converter.convert_value_enum(name, property_schema);

        Ok(ConversionOutput::new(inline_enum))
      },
    )
  }

  fn resolve_inline_struct(
    &self,
    parent_name: &str,
    property_name: &str,
    property_schema: &ObjectSchema,
  ) -> anyhow::Result<ConversionOutput<TypeRef>> {
    let prop_pascal = property_name.to_pascal_case();
    let base_name = format!("{parent_name}{}", strip_parent_prefix(parent_name, &prop_pascal));
    self.resolve_inline_struct_schema(property_schema, &base_name)
  }

  fn resolve_inline_struct_schema(
    &self,
    schema: &ObjectSchema,
    base_name: &str,
  ) -> anyhow::Result<ConversionOutput<TypeRef>> {
    handle_inline_creation(
      schema,
      base_name,
      None,
      &self.context,
      |_| None,
      |name| {
        let converter = StructConverter::new(self.context.clone());
        converter.convert_struct(name, schema, None)
      },
    )
  }

  /// Consolidates common logic for creating union types (oneOf/anyOf).
  fn create_union_type(
    &self,
    schema: &ObjectSchema,
    variants: &[ObjectOrReference<ObjectSchema>],
    base_name: &str,
  ) -> anyhow::Result<ConversionOutput<TypeRef>> {
    let variant_references = extract_variant_references(variants);

    if let Some(name) = self.lookup_matching_union_schema(&variant_references) {
      return Ok(ConversionOutput::new(self.create_type_reference(&name)));
    }

    let discriminator = schema.discriminator.as_ref().map(|d| d.property_name.as_str());

    {
      let cache = self.context.cache.borrow();
      if variant_references.len() >= 2
        && let Some(name) = cache.get_union_name(&variant_references, discriminator)
      {
        return Ok(ConversionOutput::new(TypeRef::new(name)));
      }
    }

    let uses_one_of = !schema.one_of.is_empty();

    let result = handle_inline_creation(
      schema,
      base_name,
      None,
      &self.context,
      |cache| cache.lookup_enum_name(schema),
      |name| {
        let union_converter = UnionConverter::new(self.context.clone());
        union_converter.convert_union(
          name,
          schema,
          if uses_one_of {
            UnionKind::OneOf
          } else {
            UnionKind::AnyOf
          },
        )
      },
    )?;

    if variant_references.len() >= 2 {
      self.context.cache.borrow_mut().register_union(
        variant_references,
        schema.discriminator.as_ref().map(|d| d.property_name.clone()),
        result.result.base_type.to_string(),
      );
    }

    Ok(result)
  }

  pub(crate) fn resolve_inline_union_type(
    &self,
    parent_name: &str,
    property_name: &str,
    property_schema: &ObjectSchema,
    uses_one_of: bool,
  ) -> anyhow::Result<ConversionOutput<TypeRef>> {
    let variants = if uses_one_of {
      &property_schema.one_of
    } else {
      &property_schema.any_of
    };

    if let Some(type_reference) = self.resolve_nullable_union(variants)? {
      return Ok(ConversionOutput::new(type_reference));
    }

    if let Some(result) = self.resolve_array_with_inline_items(parent_name, property_name, property_schema)? {
      return Ok(result);
    }

    let property_pascal = property_name.to_pascal_case();
    let suffix_part = format!("{property_pascal}Kind");
    let base_name = CommonVariantName::union_name(variants, &suffix_part)
      .unwrap_or_else(|| format!("{parent_name}{property_pascal}"));

    self.create_union_type(property_schema, variants, &base_name)
  }

  pub(crate) fn resolve_array_with_inline_items(
    &self,
    parent_name: &str,
    property_name: &str,
    array_schema: &ObjectSchema,
  ) -> anyhow::Result<Option<ConversionOutput<TypeRef>>> {
    let Some(items_schema) = array_schema.inline_array_items(self.context.graph().spec()) else {
      return Ok(None);
    };

    let unique_items = array_schema.unique_items.unwrap_or(false);
    let singular_name = cruet::to_singular(property_name);
    let singular_pascal = singular_name.to_pascal_case();

    let result = if items_schema.is_inline_object() {
      let base_name = format!("{parent_name}{}", strip_parent_prefix(parent_name, &singular_pascal));
      self.resolve_inline_struct_schema(&items_schema, &base_name)?
    } else if items_schema.has_union() {
      let has_one_of = !items_schema.one_of.is_empty();
      let variants = if has_one_of {
        &items_schema.one_of
      } else {
        &items_schema.any_of
      };

      let base_kind_name = format!("{singular_pascal}Kind");
      let name_to_use =
        CommonVariantName::union_name(variants, &base_kind_name).unwrap_or_else(|| base_kind_name.clone());

      let final_name = {
        let cache = self.context.cache.borrow();
        if cache.name_conflicts_with_different_schema(&name_to_use, &items_schema)? {
          cache.make_unique_name(&name_to_use)
        } else {
          name_to_use
        }
      };

      self.create_union_type(&items_schema, variants, &final_name)?
    } else {
      return Ok(None);
    };

    let mut type_reference = result.result;
    type_reference.boxed = false;
    let vec_type_reference = type_reference.with_vec().with_unique_items(unique_items);

    Ok(Some(ConversionOutput::with_inline_types(
      vec_type_reference,
      result.inline_types,
    )))
  }

  fn resolve_by_title(&self, schema: &ObjectSchema) -> Option<TypeRef> {
    let title = schema.title.as_ref()?;
    if schema.schema_type.is_some() {
      return None;
    }
    self.context.graph().get(title)?;
    Some(self.create_type_reference(title))
  }

  /// Looks up a matching union schema by fingerprint in O(1) time.
  fn lookup_matching_union_schema(&self, variant_references: &BTreeSet<String>) -> Option<String> {
    if variant_references.len() < 2 {
      return None;
    }
    self.context.graph().find_union(variant_references).cloned()
  }

  /// Partitions union variants into nullable and non-nullable types.
  fn partition_nullable_variants<'b>(
    &self,
    variants: &'b [ObjectOrReference<ObjectSchema>],
  ) -> Result<(Option<&'b ObjectOrReference<ObjectSchema>>, bool)> {
    let mut non_null_variant = None;
    let mut contains_null = false;

    for variant in variants {
      let resolved = variant
        .resolve(self.context.graph().spec())
        .context("Resolving variant for null check")?;

      if resolved.is_nullable_object() {
        contains_null = true;
      } else {
        non_null_variant = Some(variant);
      }
    }
    Ok((non_null_variant, contains_null))
  }

  fn resolve_nullable_union(&self, variants: &[ObjectOrReference<ObjectSchema>]) -> anyhow::Result<Option<TypeRef>> {
    if variants.len() != 2 {
      return Ok(None);
    }

    let (non_null_variant, contains_null) = self.partition_nullable_variants(variants)?;

    if !contains_null {
      return Ok(None);
    }

    if let Some(non_null) = non_null_variant {
      if let Some(reference_name) = RefCollector::parse_schema_ref(non_null) {
        return Ok(Some(self.create_type_reference(&reference_name).with_option()));
      }

      let resolved = non_null
        .resolve(self.context.graph().spec())
        .context("Resolving non-null union variant")?;

      if resolved.is_array() && self.contains_union_items(&resolved) {
        return Ok(None);
      }

      return Ok(Some(self.resolve_type(&resolved)?.with_option()));
    }

    Ok(None)
  }

  fn contains_union_items(&self, schema: &ObjectSchema) -> bool {
    schema
      .items
      .as_ref()
      .and_then(|b| match b.as_ref() {
        Schema::Object(o) => o.resolve(self.context.graph().spec()).ok(),
        Schema::Boolean(_) => None,
      })
      .is_some_and(|items| items.has_union())
  }

  /// Tries to convert a union schema into a single `TypeRef` (e.g. `Option<T>`, `Vec<T>`).
  pub(crate) fn resolve_union(&self, variants: &[ObjectOrReference<ObjectSchema>]) -> anyhow::Result<Option<TypeRef>> {
    let variant_references = extract_variant_references(variants);

    if let Some(name) = self.lookup_matching_union_schema(&variant_references) {
      return Ok(Some(self.create_type_reference(&name)));
    }

    if let Some(nullable_type) = self.resolve_nullable_union(variants)? {
      return Ok(Some(nullable_type));
    }

    self.resolve_union_fallback(variants)
  }

  /// Resolves union variants that don't match simple patterns.
  fn resolve_union_fallback(&self, variants: &[ObjectOrReference<ObjectSchema>]) -> anyhow::Result<Option<TypeRef>> {
    let mut reference_count = 0;
    let mut first_reference_name: Option<String> = None;

    for variant in variants {
      if let Some(reference_name) = RefCollector::parse_schema_ref(variant) {
        reference_count += 1;
        if reference_count >= 2 {
          return Ok(None);
        }
        first_reference_name = Some(reference_name);
        continue;
      }

      let Ok(resolved) = variant.resolve(self.context.graph().spec()) else {
        continue;
      };

      if resolved.is_null() {
        continue;
      }

      if resolved.is_array() {
        let item_type = self.resolve_array_items(&resolved)?;
        let unique_items = resolved.unique_items.unwrap_or(false);
        return Ok(Some(
          TypeRef::new(item_type.to_rust_type())
            .with_vec()
            .with_unique_items(unique_items),
        ));
      }

      if resolved.is_string() {
        return Ok(Some(TypeRef::new(RustPrimitive::String)));
      }

      if resolved.one_of.len() == 1
        && let Some(reference_name) = RefCollector::parse_schema_ref(&resolved.one_of[0])
      {
        return Ok(Some(self.create_type_reference(&reference_name)));
      }

      if let Some(ref variant_title) = resolved.title
        && self.context.graph().get(variant_title).is_some()
      {
        return Ok(Some(self.create_type_reference(variant_title)));
      }
    }

    if let Some(reference_name) = first_reference_name {
      return Ok(Some(self.create_type_reference(&reference_name)));
    }

    Ok(None)
  }

  fn resolve_primitive(&self, schema_type: SchemaType, schema: &ObjectSchema) -> anyhow::Result<TypeRef> {
    let primitive = match schema_type {
      SchemaType::String | SchemaType::Number | SchemaType::Integer => {
        let default = match schema_type {
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
        if let Some(map_type) = self.try_resolve_map_type(schema)? {
          return Ok(map_type);
        }
        RustPrimitive::Value
      }
      SchemaType::Null => RustPrimitive::Unit,
      SchemaType::Array => {
        let item_type = self.resolve_array_items(schema)?;
        let unique_items = schema.unique_items.unwrap_or(false);
        return Ok(
          TypeRef::new(item_type.to_rust_type())
            .with_vec()
            .with_unique_items(unique_items),
        );
      }
    };

    let mut type_ref = TypeRef::new(primitive);
    if schema_type == SchemaType::Null {
      type_ref = type_ref.with_option();
    }
    Ok(type_ref)
  }

  fn try_resolve_map_type(&self, schema: &ObjectSchema) -> anyhow::Result<Option<TypeRef>> {
    let Some(ref additional) = schema.additional_properties else {
      return Ok(None);
    };

    if matches!(additional, Schema::Boolean(b) if !b.0) {
      return Ok(None);
    }

    if !schema.properties.is_empty() {
      return Ok(None);
    }

    let value_type = self.resolve_additional_properties_type(additional)?;
    let map_type = TypeRef::new(format!(
      "std::collections::HashMap<String, {}>",
      value_type.to_rust_type()
    ));
    Ok(Some(map_type))
  }

  pub(crate) fn resolve_additional_properties_type(&self, additional: &Schema) -> anyhow::Result<TypeRef> {
    match additional {
      Schema::Boolean(_) => Ok(TypeRef::new(RustPrimitive::Value)),
      Schema::Object(schema_ref) => {
        if let ObjectOrReference::Ref { ref_path, .. } = &**schema_ref {
          let reference_name =
            SchemaRegistry::parse_ref(ref_path).ok_or_else(|| anyhow::anyhow!("Invalid reference path: {ref_path}"))?;
          return Ok(self.create_type_reference(&reference_name));
        }

        let additional_schema = schema_ref
          .resolve(self.context.graph().spec())
          .context("Schema resolution failed for additionalProperties")?;

        if additional_schema.is_empty_object() {
          return Ok(TypeRef::new(RustPrimitive::Value));
        }

        self.resolve_type(&additional_schema)
      }
    }
  }

  fn resolve_array_items(&self, schema: &ObjectSchema) -> anyhow::Result<TypeRef> {
    let Some(items_reference) = schema.items.as_ref().and_then(|b| match b.as_ref() {
      Schema::Object(o) => Some(o),
      Schema::Boolean(_) => None,
    }) else {
      return Ok(TypeRef::new(RustPrimitive::Value));
    };

    if let Some(reference_name) = RefCollector::parse_schema_ref(items_reference) {
      let mut type_reference = self.create_type_reference(&reference_name);
      type_reference.boxed = false;
      return Ok(type_reference);
    }

    let items_schema = items_reference
      .resolve(self.context.graph().spec())
      .context("Resolving array items")?;

    let mut type_reference = self.resolve_type(&items_schema)?;
    type_reference.boxed = false;
    Ok(type_reference)
  }

  pub(crate) fn build_discriminated_enum(
    &self,
    name: &str,
    schema: &ObjectSchema,
    fallback_type: &str,
  ) -> anyhow::Result<RustType> {
    let handler = DiscriminatorConverter::new(self.context.clone());
    handler.build_enum(name, schema, fallback_type)
  }

  /// Converts an inline schema with caching and deduplication.
  ///
  /// Handles the common pattern for inline schemas in request bodies and responses:
  /// 1. Check if schema was already converted (cache lookup by hash)
  /// 2. If not, convert the schema and register in cache
  ///
  /// Returns the type name and any generated types, or None if the schema is empty.
  pub(crate) fn resolve_inline_schema(
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

  /// Converts a schema definition into Rust types.
  ///
  /// Handles `allOf`, `oneOf`, `anyOf`, enums, and objects.
  pub(crate) fn convert_schema(&self, name: &str, schema: &ObjectSchema) -> anyhow::Result<Vec<RustType>> {
    let struct_converter = StructConverter::new(self.context.clone());
    let enum_converter = EnumConverter::new(self.context.clone());
    let union_converter = UnionConverter::new(self.context.clone());

    if schema.has_intersection() {
      return struct_converter.convert_all_of_schema(name);
    }

    if !schema.one_of.is_empty() {
      return union_converter
        .convert_union(name, schema, UnionKind::OneOf)
        .map(ConversionOutput::into_vec);
    }

    if !schema.any_of.is_empty() {
      return union_converter
        .convert_union(name, schema, UnionKind::AnyOf)
        .map(ConversionOutput::into_vec);
    }

    if !schema.enum_values.is_empty() {
      return Ok(vec![enum_converter.convert_value_enum(name, schema)]);
    }

    if !schema.properties.is_empty() || schema.additional_properties.is_some() {
      let result = struct_converter.convert_struct(name, schema, None)?;
      return struct_converter.finalize_struct_types(name, schema, result.result, result.inline_types);
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

    let type_ref = self.resolve_type(schema)?;
    let alias = RustType::TypeAlias(TypeAliasDef {
      name: TypeAliasToken::from_raw(name),
      docs: Documentation::from_optional(schema.description.as_ref()),
      target: type_ref,
    });

    Ok(vec![alias])
  }

  fn try_convert_array_type_alias_with_union_items(
    &self,
    name: &str,
    schema: &ObjectSchema,
  ) -> anyhow::Result<Option<ConversionOutput<TypeRef>>> {
    if !schema.is_array() && !schema.is_nullable_array() {
      return Ok(None);
    }

    if let Some(output) = self.resolve_array_with_inline_items(name, name, schema)? {
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

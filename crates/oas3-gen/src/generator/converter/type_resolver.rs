use std::collections::BTreeSet;

use inflections::Inflect;
use oas3::spec::{ObjectOrReference, ObjectSchema, SchemaType, SchemaTypeSet};

use super::{
  enums::{self, EnumConverter},
  error::ConversionResult,
};
use crate::{
  generator::{
    ast::{RustPrimitive, RustType, TypeRef},
    schema_graph::SchemaGraph,
  },
  reserved::to_rust_type_name,
};

#[derive(Clone)]
pub(crate) struct TypeResolver<'a> {
  graph: &'a SchemaGraph,
}

impl<'a> TypeResolver<'a> {
  pub(crate) fn new(graph: &'a SchemaGraph) -> Self {
    Self { graph }
  }

  pub(crate) fn schema_to_type_ref(&self, schema: &ObjectSchema) -> ConversionResult<TypeRef> {
    if let Some(type_ref) = self.try_resolve_by_title(schema) {
      return Ok(type_ref);
    }

    if !schema.one_of.is_empty()
      && let Some(type_ref) = self.try_convert_union_to_type_ref(&schema.one_of)?
    {
      return Ok(type_ref);
    }
    if !schema.any_of.is_empty()
      && let Some(type_ref) = self.try_convert_union_to_type_ref(&schema.any_of)?
    {
      return Ok(type_ref);
    }

    if let Some(ref schema_type) = schema.schema_type {
      return match schema_type {
        SchemaTypeSet::Single(typ) => self.map_single_primitive_type(*typ, schema),
        SchemaTypeSet::Multiple(types) => self.convert_nullable_primitive(types, schema),
      };
    }

    Ok(TypeRef::new("serde_json::Value"))
  }

  pub(crate) fn resolve_property_type_with_inlines(
    &self,
    parent_name: &str,
    prop_name: &str,
    prop_schema: &ObjectSchema,
    prop_schema_ref: &ObjectOrReference<ObjectSchema>,
  ) -> ConversionResult<(TypeRef, Vec<RustType>)> {
    if let ObjectOrReference::Ref { ref_path, .. } = prop_schema_ref
      && let Some(ref_name) = SchemaGraph::extract_ref_name(ref_path)
    {
      let mut type_ref = TypeRef::new(to_rust_type_name(&ref_name));
      if self.graph.is_cyclic(&ref_name) {
        type_ref = type_ref.with_boxed();
      }
      return Ok((type_ref, vec![]));
    }

    if !prop_schema.enum_values.is_empty() {
      return self.handle_inline_enum(parent_name, prop_name, prop_schema);
    }

    let has_one_of = !prop_schema.one_of.is_empty();
    if has_one_of || !prop_schema.any_of.is_empty() {
      return self.convert_inline_union_type(parent_name, prop_name, prop_schema, has_one_of);
    }

    Ok((self.schema_to_type_ref(prop_schema)?, vec![]))
  }

  fn handle_inline_enum(
    &self,
    parent_name: &str,
    prop_name: &str,
    prop_schema: &ObjectSchema,
  ) -> ConversionResult<(TypeRef, Vec<RustType>)> {
    if prop_schema.enum_values.len() == 1 {
      return Ok((self.schema_to_type_ref(prop_schema)?, vec![]));
    }

    if let Some(name) = self.find_matching_enum_schema(&prop_schema.enum_values) {
      return Ok((TypeRef::new(to_rust_type_name(&name)), vec![]));
    }

    let enum_name = format!("{}{}", parent_name, prop_name.to_pascal_case());
    let enum_converter = EnumConverter::new(self.graph, self.clone());
    let inline_enum = enum_converter.convert_simple_enum(&enum_name, prop_schema);

    Ok((TypeRef::new(RustPrimitive::Custom(enum_name)), vec![inline_enum]))
  }

  fn convert_inline_union_type(
    &self,
    parent_name: &str,
    prop_name: &str,
    prop_schema: &ObjectSchema,
    uses_one_of: bool,
  ) -> ConversionResult<(TypeRef, Vec<RustType>)> {
    let variants = if uses_one_of {
      &prop_schema.one_of
    } else {
      &prop_schema.any_of
    };

    if let Some(type_ref) = self.try_build_nullable_union(variants)? {
      return Ok((type_ref, vec![]));
    }

    if let Some(name) = self.find_matching_union_schema(variants) {
      let mut type_ref = TypeRef::new(to_rust_type_name(&name));
      if self.graph.is_cyclic(&name) {
        type_ref = type_ref.with_boxed();
      }
      return Ok((type_ref, vec![]));
    }

    let enum_name = format!("{parent_name}{}", prop_name.to_pascal_case());
    let enum_converter = EnumConverter::new(self.graph, self.clone());
    let kind = if uses_one_of {
      enums::UnionKind::OneOf
    } else {
      enums::UnionKind::AnyOf
    };
    let enum_types = enum_converter.convert_union_enum(&enum_name, prop_schema, kind)?;

    let type_name = enum_types
      .iter()
      .find_map(|t| match t {
        RustType::Enum(e) if e.name == to_rust_type_name(&enum_name) => Some(e.name.clone()),
        _ => None,
      })
      .unwrap_or_else(|| to_rust_type_name(&enum_name));

    Ok((TypeRef::new(type_name), enum_types))
  }

  fn try_build_nullable_union(
    &self,
    variants: &[ObjectOrReference<ObjectSchema>],
  ) -> ConversionResult<Option<TypeRef>> {
    if variants.len() != 2 {
      return Ok(None);
    }

    let mut non_null_variant = None;
    let mut has_null = false;

    for variant_ref in variants {
      let resolved = variant_ref
        .resolve(self.graph.spec())
        .map_err(|e| anyhow::anyhow!("Schema resolution failed for nullable union variant: {e}"))?;
      if is_null_or_nullable_object(&resolved) {
        has_null = true;
      } else {
        non_null_variant = Some(variant_ref);
      }
    }

    if !has_null || non_null_variant.is_none() {
      return Ok(None);
    }
    let non_null_variant = non_null_variant.unwrap();

    if let Some(ref_name) = SchemaGraph::extract_ref_name_from_ref(non_null_variant) {
      let mut type_ref = TypeRef::new(to_rust_type_name(&ref_name));
      if self.graph.is_cyclic(&ref_name) {
        type_ref = type_ref.with_boxed();
      }
      return Ok(Some(type_ref.with_option()));
    }

    let resolved = non_null_variant
      .resolve(self.graph.spec())
      .map_err(|e| anyhow::anyhow!("Schema resolution failed for non-null union variant: {e}"))?;
    Ok(Some(self.schema_to_type_ref(&resolved)?.with_option()))
  }

  pub(crate) fn try_convert_union_to_type_ref(
    &self,
    variants: &[ObjectOrReference<ObjectSchema>],
  ) -> ConversionResult<Option<TypeRef>> {
    if let Some(name) = self.find_matching_union_schema(variants) {
      let mut type_ref = TypeRef::new(to_rust_type_name(&name));
      if self.graph.is_cyclic(&name) {
        type_ref = type_ref.with_boxed();
      }
      return Ok(Some(type_ref));
    }

    if let Some(non_null_variant) = self.find_non_null_variant(variants) {
      if let Some(ref_name) = SchemaGraph::extract_ref_name_from_ref(non_null_variant) {
        return Ok(Some(TypeRef::new(to_rust_type_name(&ref_name)).with_option()));
      }
      let resolved = non_null_variant
        .resolve(self.graph.spec())
        .map_err(|e| anyhow::anyhow!("Schema resolution failed for non-null variant: {e}"))?;
      return Ok(Some(self.schema_to_type_ref(&resolved)?.with_option()));
    }

    let mut fallback_type: Option<TypeRef> = None;

    for variant_ref in variants {
      if let Some(ref_name) = SchemaGraph::extract_ref_name_from_ref(variant_ref) {
        return Ok(Some(TypeRef::new(to_rust_type_name(&ref_name))));
      }

      let Ok(resolved) = variant_ref.resolve(self.graph.spec()) else {
        continue;
      };

      if is_null_schema(&resolved) {
        continue;
      }

      if resolved.schema_type == Some(SchemaTypeSet::Single(SchemaType::Array)) {
        let item_type = self.convert_array_items(&resolved)?;
        let unique_items = resolved.unique_items.unwrap_or(false);
        return Ok(Some(
          TypeRef::new(item_type.to_rust_type())
            .with_vec()
            .with_unique_items(unique_items),
        ));
      }

      if resolved.schema_type == Some(SchemaTypeSet::Single(SchemaType::String)) && fallback_type.is_none() {
        fallback_type = Some(TypeRef::new(RustPrimitive::String));
        continue;
      }

      if !resolved.one_of.is_empty()
        && resolved.one_of.len() == 1
        && let Some(ref_name) = SchemaGraph::extract_ref_name_from_ref(&resolved.one_of[0])
      {
        return Ok(Some(TypeRef::new(to_rust_type_name(&ref_name))));
      }

      if let Some(ref variant_title) = resolved.title
        && self.graph.get_schema(variant_title).is_some()
      {
        return Ok(Some(TypeRef::new(to_rust_type_name(variant_title))));
      }
    }

    Ok(fallback_type)
  }

  fn map_single_primitive_type(&self, schema_type: SchemaType, schema: &ObjectSchema) -> ConversionResult<TypeRef> {
    Ok(match schema_type {
      SchemaType::String => TypeRef::new(format_to_primitive(schema.format.as_ref()).unwrap_or(RustPrimitive::String)),
      SchemaType::Number => TypeRef::new(format_to_primitive(schema.format.as_ref()).unwrap_or(RustPrimitive::F64)),
      SchemaType::Integer => TypeRef::new(format_to_primitive(schema.format.as_ref()).unwrap_or(RustPrimitive::I64)),
      SchemaType::Boolean => TypeRef::new(RustPrimitive::Bool),
      SchemaType::Array => {
        let item_type = self.convert_array_items(schema)?;
        let unique_items = schema.unique_items.unwrap_or(false);
        TypeRef::new(item_type.to_rust_type())
          .with_vec()
          .with_unique_items(unique_items)
      }
      SchemaType::Object => TypeRef::new(RustPrimitive::Value),
      SchemaType::Null => TypeRef::new(RustPrimitive::Unit).with_option(),
    })
  }

  fn convert_nullable_primitive(&self, types: &[SchemaType], schema: &ObjectSchema) -> ConversionResult<TypeRef> {
    if types.len() == 2
      && types.contains(&SchemaType::Null)
      && let Some(non_null_type) = types.iter().find(|t| **t != SchemaType::Null)
    {
      let type_ref = self.map_single_primitive_type(*non_null_type, schema)?;
      return Ok(type_ref.with_option());
    }
    Ok(TypeRef::new("serde_json::Value"))
  }

  fn convert_array_items(&self, schema: &ObjectSchema) -> ConversionResult<TypeRef> {
    let items_ref = schema
      .items
      .as_ref()
      .and_then(|b| match b.as_ref() {
        oas3::spec::Schema::Object(o) => Some(o),
        oas3::spec::Schema::Boolean(_) => None,
      })
      .ok_or_else(|| anyhow::anyhow!("Array schema missing items"))?;

    if let Some(ref_name) = SchemaGraph::extract_ref_name_from_ref(items_ref) {
      return Ok(TypeRef::new(to_rust_type_name(&ref_name)));
    }

    let items_schema = items_ref
      .resolve(self.graph.spec())
      .map_err(|e| anyhow::anyhow!("Schema resolution failed for array items: {e}"))?;
    self.schema_to_type_ref(&items_schema)
  }

  fn try_resolve_by_title(&self, schema: &ObjectSchema) -> Option<TypeRef> {
    let title = schema.title.as_ref()?;
    if schema.schema_type.is_some() {
      return None;
    }
    self.graph.get_schema(title)?;
    let mut type_ref = TypeRef::new(to_rust_type_name(title));
    if self.graph.is_cyclic(title) {
      type_ref = type_ref.with_boxed();
    }
    Some(type_ref)
  }

  fn find_matching_union_schema(&self, variants: &[ObjectOrReference<ObjectSchema>]) -> Option<String> {
    let variant_refs = extract_all_variant_refs(variants);
    if variant_refs.len() < 2 {
      return None;
    }
    self
      .graph
      .schema_names()
      .iter()
      .find(|&&name| {
        self.graph.get_schema(name).is_some_and(|s| {
          (!s.one_of.is_empty() && extract_all_variant_refs(&s.one_of) == variant_refs)
            || (!s.any_of.is_empty() && extract_all_variant_refs(&s.any_of) == variant_refs)
        })
      })
      .map(|s| (*s).clone())
  }

  fn find_matching_enum_schema(&self, enum_values: &[serde_json::Value]) -> Option<String> {
    let enum_set: BTreeSet<_> = enum_values.iter().filter_map(|v| v.as_str()).collect();
    if enum_set.is_empty() {
      return None;
    }
    self
      .graph
      .schema_names()
      .iter()
      .find(|&&name| {
        self.graph.get_schema(name).is_some_and(|s| {
          let schema_enum_set: BTreeSet<_> = s.enum_values.iter().filter_map(|v| v.as_str()).collect();
          schema_enum_set == enum_set
        })
      })
      .map(|s| (*s).clone())
  }

  fn find_non_null_variant<'b>(
    &self,
    variants: &'b [ObjectOrReference<ObjectSchema>],
  ) -> Option<&'b ObjectOrReference<ObjectSchema>> {
    if variants.len() != 2 {
      return None;
    }
    let has_null = variants
      .iter()
      .any(|v| v.resolve(self.graph.spec()).ok().is_some_and(|s| is_null_schema(&s)));
    if !has_null {
      return None;
    }
    variants
      .iter()
      .find(|v| v.resolve(self.graph.spec()).ok().is_some_and(|s| !is_null_schema(&s)))
  }
}

pub(crate) fn format_to_primitive(format: Option<&String>) -> Option<RustPrimitive> {
  match format?.as_str() {
    "int8" => Some(RustPrimitive::I8),
    "int16" => Some(RustPrimitive::I16),
    "int32" => Some(RustPrimitive::I32),
    "int64" => Some(RustPrimitive::I64),
    "uint8" => Some(RustPrimitive::U8),
    "uint16" => Some(RustPrimitive::U16),
    "uint32" => Some(RustPrimitive::U32),
    "uint64" => Some(RustPrimitive::U64),
    "float" => Some(RustPrimitive::F32),
    "double" => Some(RustPrimitive::F64),
    "date" => Some(RustPrimitive::Date),
    "date-time" => Some(RustPrimitive::DateTime),
    "time" => Some(RustPrimitive::Time),
    "duration" => Some(RustPrimitive::Duration),
    "byte" | "binary" => Some(RustPrimitive::Bytes),
    "uuid" => Some(RustPrimitive::Uuid),
    _ => None,
  }
}

fn is_null_schema(schema: &ObjectSchema) -> bool {
  schema.schema_type == Some(SchemaTypeSet::Single(SchemaType::Null))
}

fn is_null_or_nullable_object(schema: &ObjectSchema) -> bool {
  if is_null_schema(schema) {
    return true;
  }
  if let Some(SchemaTypeSet::Multiple(types)) = &schema.schema_type {
    types.contains(&SchemaType::Null)
      && types.contains(&SchemaType::Object)
      && schema.properties.is_empty()
      && schema.additional_properties.is_none()
  } else {
    false
  }
}

fn extract_all_variant_refs(variants: &[ObjectOrReference<ObjectSchema>]) -> BTreeSet<String> {
  variants
    .iter()
    .filter_map(SchemaGraph::extract_ref_name_from_ref)
    .collect()
}

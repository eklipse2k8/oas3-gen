use std::collections::{BTreeMap, BTreeSet, HashSet};

use oas3::spec::{ObjectOrReference, ObjectSchema, SchemaType, SchemaTypeSet};

use super::{error::ConversionResult, metadata, structs::StructConverter, type_resolver::TypeResolver, utils};
use crate::{
  generator::{
    ast::{EnumDef, RustType, TypeRef, VariantContent, VariantDef},
    schema_graph::SchemaGraph,
  },
  reserved::to_rust_type_name,
};

#[derive(Copy, Clone, PartialEq, Eq)]
pub(crate) enum UnionKind {
  OneOf,
  AnyOf,
}

struct VariantContext<'a> {
  parent_name: &'a str,
  index: usize,
  variant_ref: &'a ObjectOrReference<ObjectSchema>,
  resolved_schema: &'a ObjectSchema,
  has_discriminator: bool,
  discriminator_map: &'a BTreeMap<String, String>,
}

#[derive(Clone)]
pub(crate) struct EnumConverter<'a> {
  graph: &'a SchemaGraph,
  type_resolver: TypeResolver<'a>,
  struct_converter: StructConverter<'a>,
}

impl<'a> EnumConverter<'a> {
  pub(crate) fn new(graph: &'a SchemaGraph, type_resolver: TypeResolver<'a>) -> Self {
    let struct_converter = StructConverter::new(graph, type_resolver.clone());
    Self {
      graph,
      type_resolver,
      struct_converter,
    }
  }

  #[allow(clippy::unused_self)]
  pub(crate) fn convert_simple_enum(&self, name: &str, schema: &ObjectSchema) -> RustType {
    let mut variants = Vec::new();
    let mut seen_names = BTreeSet::new();

    for (i, value) in schema.enum_values.iter().enumerate() {
      if let Some(str_val) = value.as_str() {
        let mut variant_name = to_rust_type_name(str_val);
        if !seen_names.insert(variant_name.clone()) {
          variant_name = format!("{variant_name}{i}");
        }
        variants.push(VariantDef {
          name: variant_name,
          docs: vec![],
          content: VariantContent::Unit,
          serde_attrs: vec![format!(r#"rename = "{}""#, str_val)],
          deprecated: false,
        });
      }
    }

    RustType::Enum(EnumDef {
      name: to_rust_type_name(name),
      docs: metadata::extract_docs(schema.description.as_ref()),
      variants,
      discriminator: None,
      derives: utils::derives_for_enum(true),
      serde_attrs: vec![],
      outer_attrs: vec![],
    })
  }

  pub(crate) fn convert_union_enum(
    &self,
    name: &str,
    schema: &ObjectSchema,
    kind: UnionKind,
  ) -> ConversionResult<Vec<RustType>> {
    if kind == UnionKind::AnyOf
      && let Some(result) = self.try_convert_string_catch_all(name, schema)?
    {
      return Ok(result);
    }

    let variants_src = match kind {
      UnionKind::OneOf => &schema.one_of,
      UnionKind::AnyOf => &schema.any_of,
    };

    let discriminator_map = if kind == UnionKind::OneOf {
      schema
        .discriminator
        .as_ref()
        .and_then(|d| d.mapping.as_ref())
        .map(|mapping| {
          mapping
            .iter()
            .filter_map(|(val, ref_path)| SchemaGraph::extract_ref_name(ref_path).map(|name| (name, val.clone())))
            .collect()
        })
        .unwrap_or_default()
    } else {
      BTreeMap::new()
    };

    let has_discriminator = schema.discriminator.is_some();
    let mut inline_types = Vec::new();
    let mut variants = Vec::new();
    let mut seen_names = BTreeSet::new();

    for (i, variant_ref) in variants_src.iter().enumerate() {
      let resolved = variant_ref
        .resolve(self.graph.spec())
        .map_err(|e| anyhow::anyhow!("Schema resolution failed for union variant {i}: {e}"))?;

      if resolved.schema_type == Some(SchemaTypeSet::Single(SchemaType::Null)) {
        continue;
      }

      let ctx = VariantContext {
        parent_name: name,
        index: i,
        variant_ref,
        resolved_schema: &resolved,
        has_discriminator,
        discriminator_map: &discriminator_map,
      };
      let (variant, mut generated_types) = self.process_union_variant(&ctx, &mut seen_names)?;
      variants.push(variant);
      inline_types.append(&mut generated_types);
    }

    utils::strip_common_affixes(&mut variants);

    let (serde_attrs, derives) = if kind == UnionKind::AnyOf {
      (vec!["untagged".into()], utils::derives_for_enum(false))
    } else {
      (vec![], utils::derives_for_enum(false))
    };

    let main_enum = RustType::Enum(EnumDef {
      name: to_rust_type_name(name),
      docs: metadata::extract_docs(schema.description.as_ref()),
      variants,
      discriminator: schema.discriminator.as_ref().map(|d| d.property_name.clone()),
      derives,
      serde_attrs,
      outer_attrs: vec![],
    });

    inline_types.push(main_enum);
    Ok(inline_types)
  }

  fn process_union_variant(
    &self,
    ctx: &VariantContext<'_>,
    seen_names: &mut BTreeSet<String>,
  ) -> ConversionResult<(VariantDef, Vec<RustType>)> {
    let ref_schema_name = SchemaGraph::extract_ref_name_from_ref(ctx.variant_ref);

    if let Some(schema_name) = ref_schema_name {
      let rust_type_name = to_rust_type_name(&schema_name);
      let mut type_ref = TypeRef::new(&rust_type_name);
      if self.graph.is_cyclic(&schema_name) {
        type_ref = type_ref.with_boxed();
      }
      let variant_name = utils::unique_variant_name(&rust_type_name, ctx.index, seen_names);
      let mut serde_attrs = Vec::new();
      if ctx.has_discriminator
        && let Some(disc_value) = ctx.discriminator_map.get(&schema_name)
      {
        serde_attrs.push(format!(r#"rename = "{disc_value}""#));
      }
      let variant = VariantDef {
        name: variant_name,
        docs: metadata::extract_docs(ctx.resolved_schema.description.as_ref()),
        content: VariantContent::Tuple(vec![type_ref]),
        serde_attrs,
        deprecated: ctx.resolved_schema.deprecated.unwrap_or(false),
      };
      return Ok((variant, vec![]));
    }

    let base_name = ctx.resolved_schema.title.as_ref().map_or_else(
      || utils::infer_variant_name(ctx.resolved_schema, ctx.index),
      |t| to_rust_type_name(t),
    );
    let variant_name = utils::unique_variant_name(&base_name, ctx.index, seen_names);

    let (content, generated_types) = if ctx.resolved_schema.properties.is_empty() {
      let type_ref = self.type_resolver.schema_to_type_ref(ctx.resolved_schema)?;
      (VariantContent::Tuple(vec![type_ref]), vec![])
    } else {
      let (struct_def, mut inline_types) = self.struct_converter.convert_struct(
        &format!("{}{variant_name}", ctx.parent_name),
        ctx.resolved_schema,
        None,
      )?;
      let struct_name = match &struct_def {
        RustType::Struct(s) => s.name.clone(),
        _ => unreachable!(),
      };
      inline_types.push(struct_def);
      (VariantContent::Tuple(vec![TypeRef::new(struct_name)]), inline_types)
    };

    let variant = VariantDef {
      name: variant_name,
      docs: metadata::extract_docs(ctx.resolved_schema.description.as_ref()),
      content,
      serde_attrs: vec![],
      deprecated: ctx.resolved_schema.deprecated.unwrap_or(false),
    };

    Ok((variant, generated_types))
  }

  #[allow(clippy::unnecessary_wraps)]
  fn try_convert_string_catch_all(&self, name: &str, schema: &ObjectSchema) -> ConversionResult<Option<Vec<RustType>>> {
    let has_freeform_string = schema.any_of.iter().any(|s| {
      s.resolve(self.graph.spec()).ok().is_some_and(|resolved| {
        resolved.const_value.is_none()
          && resolved.enum_values.is_empty()
          && resolved.schema_type == Some(SchemaTypeSet::Single(SchemaType::String))
      })
    });

    if !has_freeform_string {
      return Ok(None);
    }

    let mut seen_values = HashSet::new();
    let mut known_values = Vec::new();

    for variant in &schema.any_of {
      let Ok(resolved) = variant.resolve(self.graph.spec()) else {
        continue;
      };

      if let Some(const_val) = resolved.const_value.as_ref().and_then(|v| v.as_str())
        && seen_values.insert(const_val.to_string())
      {
        known_values.push((
          const_val.to_string(),
          resolved.description.clone(),
          resolved.deprecated.unwrap_or(false),
        ));
        continue;
      }

      if resolved.schema_type == Some(SchemaTypeSet::Single(SchemaType::String)) && !resolved.enum_values.is_empty() {
        for enum_value in &resolved.enum_values {
          if let Some(str_val) = enum_value.as_str()
            && seen_values.insert(str_val.to_string())
          {
            known_values.push((
              str_val.to_string(),
              resolved.description.clone(),
              resolved.deprecated.unwrap_or(false),
            ));
          }
        }
      }
    }

    if known_values.is_empty() {
      return Ok(None);
    }

    Ok(Some(Self::convert_string_enum_with_catch_all(
      name,
      schema,
      &known_values,
    )))
  }

  fn convert_string_enum_with_catch_all(
    name: &str,
    schema: &ObjectSchema,
    const_values: &[(String, Option<String>, bool)],
  ) -> Vec<RustType> {
    let base_name = to_rust_type_name(name);
    let known_name = format!("{base_name}Known");
    let mut seen_names = BTreeSet::new();

    let known_variants = const_values
      .iter()
      .enumerate()
      .map(|(i, (value, description, deprecated))| {
        let variant_name = utils::unique_variant_name(&to_rust_type_name(value), i, &mut seen_names);
        VariantDef {
          name: variant_name,
          docs: metadata::extract_docs(description.as_ref()),
          content: VariantContent::Unit,
          serde_attrs: vec![format!(r#"rename = "{}""#, value)],
          deprecated: *deprecated,
        }
      })
      .collect();

    let inner_enum = RustType::Enum(EnumDef {
      name: known_name.clone(),
      docs: vec!["/// Known values for the string enum.".to_string()],
      variants: known_variants,
      discriminator: None,
      derives: utils::derives_for_enum(true),
      serde_attrs: vec![],
      outer_attrs: vec![],
    });

    let outer_variants = vec![
      VariantDef {
        name: "Known".to_string(),
        docs: vec!["/// A known value.".to_string()],
        content: VariantContent::Tuple(vec![TypeRef::new(&known_name)]),
        serde_attrs: vec![],
        deprecated: false,
      },
      VariantDef {
        name: "Other".to_string(),
        docs: vec!["/// An unknown value.".to_string()],
        content: VariantContent::Tuple(vec![TypeRef::new("String")]),
        serde_attrs: vec![],
        deprecated: false,
      },
    ];

    let outer_enum = RustType::Enum(EnumDef {
      name: base_name,
      docs: metadata::extract_docs(schema.description.as_ref()),
      variants: outer_variants,
      discriminator: None,
      derives: utils::derives_for_enum(false),
      serde_attrs: vec!["untagged".into()],
      outer_attrs: vec![],
    });

    vec![inner_enum, outer_enum]
  }
}

use std::collections::{BTreeMap, BTreeSet, HashSet};

use oas3::spec::{ObjectOrReference, ObjectSchema, SchemaType, SchemaTypeSet};

use super::{
  ConversionResult, cache::SharedSchemaCache, field_optionality::FieldOptionalityPolicy, metadata,
  structs::StructConverter, type_resolver::TypeResolver, utils,
};
use crate::{
  generator::{
    ast::{EnumDef, RustType, TypeRef, VariantContent, VariantDef, default_enum_derives},
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
  preserve_case_variants: bool,
  case_insensitive_enums: bool,
}

impl<'a> EnumConverter<'a> {
  pub(crate) fn new(
    graph: &'a SchemaGraph,
    type_resolver: TypeResolver<'a>,
    preserve_case_variants: bool,
    case_insensitive_enums: bool,
  ) -> Self {
    let struct_converter = StructConverter::new(graph, type_resolver.clone(), None, FieldOptionalityPolicy::standard());
    Self {
      graph,
      type_resolver,
      struct_converter,
      preserve_case_variants,
      case_insensitive_enums,
    }
  }

  pub(crate) fn convert_simple_enum(&self, name: &str, schema: &ObjectSchema) -> RustType {
    if self.preserve_case_variants {
      self.convert_simple_enum_with_case_preservation(name, schema)
    } else {
      self.convert_simple_enum_with_case_deduplication(name, schema)
    }
  }

  fn convert_simple_enum_with_case_preservation(&self, name: &str, schema: &ObjectSchema) -> RustType {
    let mut variants = Vec::new();
    let mut seen_names = BTreeSet::new();

    for (i, value) in schema.enum_values.iter().enumerate() {
      let (variant_name_base, rename_value) = if let Some(str_val) = value.as_str() {
        (to_rust_type_name(str_val), str_val.to_string())
      } else if let Some(num_val) = value.as_i64() {
        (format!("Value{num_val}"), num_val.to_string())
      } else if let Some(num_val) = value.as_f64() {
        let normalized = format!("{num_val}");
        (format!("Value{}", normalized.replace(['.', '-'], "_")), normalized)
      } else if value.is_boolean() {
        let bool_val = value.as_bool().unwrap();
        (
          if bool_val {
            "True".to_string()
          } else {
            "False".to_string()
          },
          bool_val.to_string(),
        )
      } else {
        continue;
      };

      let mut variant_name = variant_name_base;
      if !seen_names.insert(variant_name.clone()) {
        variant_name = format!("{variant_name}{i}");
      }

      variants.push(VariantDef {
        name: variant_name,
        docs: vec![],
        content: VariantContent::Unit,
        serde_attrs: vec![format!(r#"rename = "{}""#, rename_value)],
        deprecated: false,
      });
    }

    RustType::Enum(EnumDef {
      name: to_rust_type_name(name),
      docs: metadata::extract_docs(schema.description.as_ref()),
      variants,
      discriminator: None,
      derives: default_enum_derives(true),
      serde_attrs: vec![],
      outer_attrs: vec![],
      case_insensitive: self.case_insensitive_enums,
    })
  }

  fn convert_simple_enum_with_case_deduplication(&self, name: &str, schema: &ObjectSchema) -> RustType {
    let mut variants: Vec<VariantDef> = Vec::new();
    let mut seen_names: BTreeMap<String, usize> = BTreeMap::new();

    for value in &schema.enum_values {
      let (variant_name_base, rename_value) = if let Some(str_val) = value.as_str() {
        (to_rust_type_name(str_val), str_val.to_string())
      } else if let Some(num_val) = value.as_i64() {
        (format!("Value{num_val}"), num_val.to_string())
      } else if let Some(num_val) = value.as_f64() {
        let normalized = format!("{num_val}");
        (format!("Value{}", normalized.replace(['.', '-'], "_")), normalized)
      } else if value.is_boolean() {
        let bool_val = value.as_bool().unwrap();
        (
          if bool_val {
            "True".to_string()
          } else {
            "False".to_string()
          },
          bool_val.to_string(),
        )
      } else {
        continue;
      };

      if let Some(&idx) = seen_names.get(&variant_name_base) {
        variants[idx].serde_attrs.push(format!(r#"alias = "{rename_value}""#));
      } else {
        let idx = variants.len();
        seen_names.insert(variant_name_base.clone(), idx);
        variants.push(VariantDef {
          name: variant_name_base,
          docs: vec![],
          content: VariantContent::Unit,
          serde_attrs: vec![format!(r#"rename = "{}""#, rename_value)],
          deprecated: false,
        });
      }
    }

    RustType::Enum(EnumDef {
      name: to_rust_type_name(name),
      docs: metadata::extract_docs(schema.description.as_ref()),
      variants,
      discriminator: None,
      derives: default_enum_derives(true),
      serde_attrs: vec![],
      outer_attrs: vec![],
      case_insensitive: self.case_insensitive_enums,
    })
  }

  pub(crate) fn convert_union_enum(
    &self,
    name: &str,
    schema: &ObjectSchema,
    kind: UnionKind,
    mut cache: Option<&mut SharedSchemaCache>,
  ) -> ConversionResult<Vec<RustType>> {
    if kind == UnionKind::AnyOf
      && let Some(result) = self.try_convert_string_catch_all(name, schema, cache.as_deref_mut())?
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
      let (variant, mut generated_types) = self.process_union_variant(&ctx, &mut seen_names, cache.as_deref_mut())?;
      variants.push(variant);
      inline_types.append(&mut generated_types);
    }

    utils::strip_common_affixes(&mut variants);

    let has_discriminator = schema.discriminator.is_some();
    let (serde_attrs, derives) = if kind == UnionKind::AnyOf && !has_discriminator {
      (vec!["untagged".into()], default_enum_derives(false))
    } else {
      (vec![], default_enum_derives(false))
    };

    let main_enum = RustType::Enum(EnumDef {
      name: to_rust_type_name(name),
      docs: metadata::extract_docs(schema.description.as_ref()),
      variants,
      discriminator: schema.discriminator.as_ref().map(|d| d.property_name.clone()),
      derives,
      serde_attrs,
      outer_attrs: vec![],
      case_insensitive: false,
    });

    inline_types.push(main_enum);
    Ok(inline_types)
  }

  fn process_union_variant(
    &self,
    ctx: &VariantContext<'_>,
    seen_names: &mut BTreeSet<String>,
    cache: Option<&mut SharedSchemaCache>,
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
        cache,
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
  fn try_convert_string_catch_all(
    &self,
    name: &str,
    schema: &ObjectSchema,
    mut cache: Option<&mut SharedSchemaCache>,
  ) -> ConversionResult<Option<Vec<RustType>>> {
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

    let base_name = to_rust_type_name(name);
    let mut cache_key_values: Vec<String> = known_values.iter().map(|(v, _, _)| v.clone()).collect();
    cache_key_values.sort();

    let known_name;
    let inner_enum_type;

    if let Some(ref mut c) = cache {
      if let Some(existing) = c.get_enum_name(&cache_key_values) {
        known_name = existing;
        if c.is_enum_generated(&cache_key_values) {
          inner_enum_type = None;
        } else {
          inner_enum_type = Some(self.build_known_enum(&known_name, &known_values));
          c.register_enum(cache_key_values, known_name.clone());
          c.mark_name_used(known_name.clone());
        }
      } else {
        known_name = format!("{base_name}Known");
        inner_enum_type = Some(self.build_known_enum(&known_name, &known_values));
        c.register_enum(cache_key_values, known_name.clone());
        c.mark_name_used(known_name.clone());
      }
    } else {
      known_name = format!("{base_name}Known");
      inner_enum_type = Some(self.build_known_enum(&known_name, &known_values));
    }

    let outer_variants = vec![
      VariantDef {
        name: "Known".to_string(),
        docs: vec!["/// A known value.".to_string()],
        content: VariantContent::Tuple(vec![TypeRef::new(&known_name)]),
        serde_attrs: vec![format!("default")],
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
      derives: default_enum_derives(false),
      serde_attrs: vec!["untagged".into()],
      outer_attrs: vec![],
      case_insensitive: false,
    });

    let mut types = Vec::new();
    if let Some(ie) = inner_enum_type {
      types.push(ie);
    }
    types.push(outer_enum);

    Ok(Some(types))
  }

  fn build_known_enum(&self, name: &str, values: &[(String, Option<String>, bool)]) -> RustType {
    let mut seen_names = BTreeSet::new();
    let variants = values
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

    RustType::Enum(EnumDef {
      name: name.to_string(),
      docs: vec!["/// Known values for the string enum.".to_string()],
      variants,
      discriminator: None,
      derives: default_enum_derives(true),
      serde_attrs: vec![],
      outer_attrs: vec![],
      case_insensitive: self.case_insensitive_enums,
    })
  }
}

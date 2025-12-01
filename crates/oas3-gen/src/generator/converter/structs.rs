use std::{
  cmp::Reverse,
  collections::{BTreeMap, BTreeSet, HashMap, HashSet},
  sync::Arc,
};

use anyhow::Context as _;
use inflections::Inflect;
use oas3::spec::{Discriminator, ObjectOrReference, ObjectSchema, Schema, SchemaTypeSet};
use string_cache::DefaultAtom;

use super::{
  CodegenConfig, ConversionOutput, SchemaExt,
  cache::SharedSchemaCache,
  field_optionality::{FieldContext, FieldOptionalityPolicy},
  metadata::{self, FieldMetadata},
  type_resolver::TypeResolver,
};
use crate::generator::{
  ast::{
    DiscriminatedEnumDef, DiscriminatedVariant, EnumToken, FieldDef, FieldDefBuilder, RustType, SerdeAttribute,
    SerdeMode, StructDef, StructKind, StructToken, TypeRef, default_struct_derives, tokens::FieldNameToken,
  },
  naming::{
    constants::{DISCRIMINATED_BASE_SUFFIX, MERGED_SCHEMA_CACHE_SUFFIX},
    identifiers::{to_rust_field_name, to_rust_type_name},
  },
  schema_registry::{ReferenceExtractor, SchemaRegistry},
};

const HIDDEN: &str = "#[doc(hidden)]";

pub(crate) struct DiscriminatorAttributesResult {
  pub metadata: FieldMetadata,
  pub serde_attrs: Vec<SerdeAttribute>,
  pub extra_attrs: Vec<String>,
}

struct AdditionalPropertiesResult {
  serde_attrs: Vec<SerdeAttribute>,
  additional_field: Option<FieldDef>,
}

struct FieldProcessingContext<'a> {
  prop_name: &'a str,
  schema: &'a ObjectSchema,
}

pub(crate) struct DiscriminatorInfo {
  pub value: Option<DefaultAtom>,
  pub is_base: bool,
  pub has_enum: bool,
}

/// Converter for OpenAPI object schemas into Rust Structs.
#[derive(Clone)]
pub(crate) struct StructConverter {
  graph: Arc<SchemaRegistry>,
  type_resolver: TypeResolver,
  merger: SchemaMerger,
  field_processor: FieldProcessor,
  discriminator_handler: DiscriminatorHandler,
}

impl StructConverter {
  pub(crate) fn new(
    graph: Arc<SchemaRegistry>,
    config: CodegenConfig,
    reachable_schemas: Option<Arc<BTreeSet<String>>>,
    optionality_policy: FieldOptionalityPolicy,
  ) -> Self {
    let type_resolver = TypeResolver::new(&graph, config);
    let merger = SchemaMerger::new(graph.clone());
    let field_processor = FieldProcessor::new(graph.clone(), optionality_policy, type_resolver.clone());
    let discriminator_handler = DiscriminatorHandler::new(graph.clone(), reachable_schemas);
    Self {
      graph,
      type_resolver,
      merger,
      field_processor,
      discriminator_handler,
    }
  }

  fn convert_fields(
    &self,
    parent_name: &str,
    schema: &ObjectSchema,
    exclude_field: Option<&str>,
    schema_name: Option<&str>,
    mut cache: Option<&mut SharedSchemaCache>,
  ) -> anyhow::Result<ConversionOutput<Vec<FieldDef>>> {
    let num_properties = schema.properties.len();
    let required_set: std::collections::HashSet<&String> = schema.required.iter().collect();

    let discriminator_mapping = schema_name.and_then(|name| self.graph.get_discriminator_mapping(name));

    let mut fields = Vec::with_capacity(num_properties);
    let mut inline_types = vec![];

    for (prop_name, prop_schema_ref) in &schema.properties {
      if exclude_field == Some(prop_name.as_str()) {
        continue;
      }

      let is_required = required_set.contains(prop_name);

      let prop_schema = prop_schema_ref
        .resolve(self.graph.spec())
        .with_context(|| format!("Schema resolution failed for property '{prop_name}'"))?;

      let cache_borrow = cache.as_deref_mut();
      let resolved = self.resolve_field_type(parent_name, prop_name, &prop_schema, prop_schema_ref, cache_borrow)?;

      let ctx = FieldProcessingContext { prop_name, schema };

      let field = self.field_processor.process_single_field(
        &ctx,
        &prop_schema,
        resolved.result,
        is_required,
        discriminator_mapping,
      )?;
      fields.push(field);

      let mut generated = resolved.inline_types;
      inline_types.append(&mut generated);
    }

    Self::deduplicate_field_names(&mut fields);
    Ok(ConversionOutput::with_inline_types(fields, inline_types))
  }

  /// Converts a schema composed with `allOf` by merging properties.
  pub(crate) fn convert_all_of_schema(
    &self,
    name: &str,
    schema: &ObjectSchema,
    cache: Option<&mut SharedSchemaCache>,
  ) -> anyhow::Result<Vec<RustType>> {
    let mut merged_schema_cache = HashMap::new();

    if let Some(parent_schema) = self
      .discriminator_handler
      .detect_discriminated_parent(schema, &mut merged_schema_cache)
    {
      return self.convert_discriminated_child(name, schema, &parent_schema, &mut merged_schema_cache, cache);
    }

    let merged_schema = self.merger.merge_all_of_schema(schema)?;
    let result = self.convert_struct(name, &merged_schema, None, cache)?;

    self.finalize_struct_types(name, &merged_schema, result.result, result.inline_types)
  }

  fn resolve_field_type(
    &self,
    parent_name: &str,
    prop_name: &str,
    prop_schema: &ObjectSchema,
    prop_schema_ref: &ObjectOrReference<ObjectSchema>,
    cache: Option<&mut SharedSchemaCache>,
  ) -> anyhow::Result<ConversionOutput<TypeRef>> {
    if prop_schema.is_inline_struct(prop_schema_ref) {
      return self.generate_inline_struct(parent_name, prop_name, prop_schema, cache);
    }

    self
      .type_resolver
      .resolve_property_type_with_inlines(parent_name, prop_name, prop_schema, prop_schema_ref, cache)
  }

  fn generate_inline_struct(
    &self,
    parent_name: &str,
    prop_name: &str,
    schema: &ObjectSchema,
    cache: Option<&mut SharedSchemaCache>,
  ) -> anyhow::Result<ConversionOutput<TypeRef>> {
    let base_name = format!("{}{}", parent_name, prop_name.to_pascal_case());

    super::common::handle_inline_creation(
      schema,
      &base_name,
      None,
      cache,
      |_| None,
      |name, cache| self.convert_struct(name, schema, None, cache),
    )
  }

  /// Converts a standard object schema into a Rust Struct.
  pub(crate) fn convert_struct(
    &self,
    name: &str,
    schema: &ObjectSchema,
    kind: Option<StructKind>,
    cache: Option<&mut SharedSchemaCache>,
  ) -> anyhow::Result<ConversionOutput<RustType>> {
    let is_discriminated = is_discriminated_base_type(schema);
    let struct_name = if is_discriminated {
      StructToken::from(format!("{}{DISCRIMINATED_BASE_SUFFIX}", to_rust_type_name(name)))
    } else {
      StructToken::from_raw(name)
    };

    let field_result = self.convert_fields(struct_name.as_str(), schema, None, Some(name), cache)?;
    let additional_props = self.field_processor.prepare_additional_properties(schema)?;

    let mut fields = field_result.result;
    let mut serde_attrs = additional_props.serde_attrs;

    if let Some(field) = additional_props.additional_field {
      fields.push(field);
    }

    if fields.iter().any(|f| f.default_value.is_some()) {
      serde_attrs.push(SerdeAttribute::Default);
    }

    let struct_type = RustType::Struct(StructDef {
      name: struct_name,
      docs: metadata::extract_docs(schema.description.as_ref()),
      fields,
      derives: default_struct_derives(),
      serde_attrs,
      outer_attrs: vec![],
      methods: vec![],
      kind: kind.unwrap_or(StructKind::Schema),
    });

    Ok(ConversionOutput::with_inline_types(
      struct_type,
      field_result.inline_types,
    ))
  }

  fn convert_discriminated_child(
    &self,
    name: &str,
    schema: &ObjectSchema,
    parent_schema: &ObjectSchema,
    merged_schema_cache: &mut HashMap<String, ObjectSchema>,
    cache: Option<&mut SharedSchemaCache>,
  ) -> anyhow::Result<Vec<RustType>> {
    if parent_schema.discriminator.is_none() {
      anyhow::bail!("Parent schema for discriminated child '{name}' is not a valid discriminator base");
    }

    let struct_name = StructToken::from_raw(name);

    let cache_key = format!("{name}{MERGED_SCHEMA_CACHE_SUFFIX}");
    let merged_schema = if let Some(cached) = merged_schema_cache.get(&cache_key) {
      cached.clone()
    } else {
      let new_merged = self.merger.merge_child_schema_with_parent(schema, parent_schema)?;
      merged_schema_cache.insert(cache_key, new_merged.clone());
      new_merged
    };

    let field_result = self.convert_fields(struct_name.as_str(), &merged_schema, None, Some(name), cache)?;
    let additional_props = self.field_processor.prepare_additional_properties(&merged_schema)?;

    let mut fields = field_result.result;
    let mut serde_attrs = additional_props.serde_attrs;

    if let Some(field) = additional_props.additional_field {
      fields.push(field);
    }

    if fields.iter().any(|f| f.default_value.is_some()) {
      serde_attrs.push(SerdeAttribute::Default);
    }

    let mut all_types = Vec::with_capacity(1 + field_result.inline_types.len());
    all_types.push(RustType::Struct(StructDef {
      name: struct_name,
      docs: metadata::extract_docs(schema.description.as_ref()),
      fields,
      derives: default_struct_derives(),
      serde_attrs,
      outer_attrs: vec![],
      methods: vec![],
      kind: StructKind::Schema,
    }));

    let mut inline_types = field_result.inline_types;
    all_types.append(&mut inline_types);
    Ok(all_types)
  }

  pub(crate) fn finalize_struct_types(
    &self,
    name: &str,
    schema: &ObjectSchema,
    main_type: RustType,
    mut inline_types: Vec<RustType>,
  ) -> anyhow::Result<Vec<RustType>> {
    let is_discriminated = is_discriminated_base_type(schema);
    let capacity = if is_discriminated { 2 } else { 1 } + inline_types.len();
    let mut all_types = Vec::with_capacity(capacity);

    if is_discriminated {
      let base_struct_name = match &main_type {
        RustType::Struct(def) => def.name.clone(),
        _ => StructToken::from(format!("{}{DISCRIMINATED_BASE_SUFFIX}", to_rust_type_name(name))),
      };
      let discriminated_enum =
        self
          .discriminator_handler
          .create_discriminated_enum(name, schema, base_struct_name.as_str())?;
      all_types.push(discriminated_enum);
    }

    all_types.push(main_type);
    all_types.append(&mut inline_types);
    Ok(all_types)
  }

  pub(crate) fn deduplicate_field_names(fields: &mut Vec<FieldDef>) {
    let mut indices_by_name: BTreeMap<String, Vec<usize>> = BTreeMap::new();
    for (i, field) in fields.iter().enumerate() {
      indices_by_name.entry(field.name.to_string()).or_default().push(i);
    }

    let mut indices_to_remove = HashSet::<usize>::new();

    for (name, colliding_indices) in indices_by_name.into_iter().filter(|(_, v)| v.len() > 1) {
      let (deprecated, non_deprecated): (Vec<usize>, Vec<usize>) =
        colliding_indices.iter().copied().partition(|&i| fields[i].deprecated);

      if !deprecated.is_empty() && !non_deprecated.is_empty() {
        indices_to_remove.extend(deprecated);
      } else {
        for (suffix_num, &idx) in colliding_indices.iter().enumerate().skip(1) {
          fields[idx].name = FieldNameToken::new(format!("{name}_{}", suffix_num + 1));
        }
      }
    }

    if !indices_to_remove.is_empty() {
      let mut idx = 0;
      fields.retain(|_| {
        let keep = !indices_to_remove.contains(&idx);
        idx += 1;
        keep
      });
    }
  }
}

#[derive(Clone)]
pub(crate) struct SchemaMerger {
  graph: Arc<SchemaRegistry>,
}

impl SchemaMerger {
  pub(crate) fn new(graph: Arc<SchemaRegistry>) -> Self {
    Self { graph }
  }

  pub(crate) fn merge_child_schema_with_parent(
    &self,
    child_schema: &ObjectSchema,
    parent_schema: &ObjectSchema,
  ) -> anyhow::Result<ObjectSchema> {
    let mut merged_properties = BTreeMap::new();
    let mut merged_required = BTreeSet::new();
    let mut merged_discriminator = parent_schema.discriminator.clone();
    let mut merged_schema_type = parent_schema.schema_type.clone();

    self.collect_all_of_properties(
      child_schema,
      &mut merged_properties,
      &mut merged_required,
      &mut merged_discriminator,
      &mut merged_schema_type,
    )?;

    let mut merged_schema = child_schema.clone();
    merged_schema.properties = merged_properties;
    merged_schema.required = merged_required.into_iter().collect();
    merged_schema.discriminator = merged_discriminator;
    merged_schema.schema_type = merged_schema_type;
    merged_schema.all_of.clear();

    if merged_schema.additional_properties.is_none() {
      merged_schema
        .additional_properties
        .clone_from(&parent_schema.additional_properties);
    }

    Ok(merged_schema)
  }

  pub(crate) fn merge_all_of_schema(&self, schema: &ObjectSchema) -> anyhow::Result<ObjectSchema> {
    let mut merged_properties = BTreeMap::new();
    let mut merged_required = BTreeSet::new();
    let mut merged_discriminator = None;
    let mut merged_schema_type = None;

    self.collect_all_of_properties(
      schema,
      &mut merged_properties,
      &mut merged_required,
      &mut merged_discriminator,
      &mut merged_schema_type,
    )?;

    let mut merged_schema = schema.clone();
    merged_schema.properties = merged_properties;
    merged_schema.required = merged_required.into_iter().collect();
    merged_schema.discriminator = merged_discriminator;
    if merged_schema_type.is_some() {
      merged_schema.schema_type = merged_schema_type;
    }
    merged_schema.all_of.clear();

    Ok(merged_schema)
  }

  fn collect_all_of_properties(
    &self,
    schema: &ObjectSchema,
    properties: &mut BTreeMap<String, ObjectOrReference<ObjectSchema>>,
    required: &mut BTreeSet<String>,
    discriminator: &mut Option<Discriminator>,
    schema_type: &mut Option<SchemaTypeSet>,
  ) -> anyhow::Result<()> {
    for all_of_ref in &schema.all_of {
      let all_of_schema = all_of_ref
        .resolve(self.graph.spec())
        .with_context(|| "Schema resolution failed for allOf item")?;
      self.collect_all_of_properties(&all_of_schema, properties, required, discriminator, schema_type)?;
    }

    for (prop_name, prop_ref) in &schema.properties {
      properties.insert(prop_name.clone(), prop_ref.clone());
    }
    required.extend(schema.required.iter().cloned());

    if schema.discriminator.is_some() {
      discriminator.clone_from(&schema.discriminator);
    }

    if schema.schema_type.is_some() {
      schema_type.clone_from(&schema.schema_type);
    }
    Ok(())
  }

  fn get_merged_schema(
    &self,
    schema_name: &str,
    schema: &ObjectSchema,
    merged_schema_cache: &mut HashMap<String, ObjectSchema>,
  ) -> anyhow::Result<ObjectSchema> {
    if let Some(cached) = merged_schema_cache.get(schema_name) {
      return Ok(cached.clone());
    }

    let merged = self.merge_all_of_schema(schema)?;
    merged_schema_cache.insert(schema_name.to_string(), merged.clone());
    Ok(merged)
  }
}

#[derive(Clone)]
pub(crate) struct DiscriminatorHandler {
  graph: Arc<SchemaRegistry>,
  reachable_schemas: Option<Arc<BTreeSet<String>>>,
  merger: SchemaMerger,
}

impl DiscriminatorHandler {
  pub(crate) fn new(graph: Arc<SchemaRegistry>, reachable_schemas: Option<Arc<BTreeSet<String>>>) -> Self {
    let merger = SchemaMerger::new(graph.clone());
    Self {
      graph,
      reachable_schemas,
      merger,
    }
  }

  pub(crate) fn detect_discriminated_parent(
    &self,
    schema: &ObjectSchema,
    merged_schema_cache: &mut HashMap<String, ObjectSchema>,
  ) -> Option<ObjectSchema> {
    if schema.all_of.is_empty() {
      return None;
    }

    schema.all_of.iter().find_map(|all_of_ref| {
      let ObjectOrReference::Ref { ref_path, .. } = all_of_ref else {
        return None;
      };
      let parent_name = SchemaRegistry::extract_ref_name(ref_path)?;
      let parent_schema = self.graph.get_schema(&parent_name)?;

      parent_schema.discriminator.as_ref()?;

      let merged_parent = self
        .merger
        .get_merged_schema(&parent_name, parent_schema, merged_schema_cache)
        .ok()?;
      if is_discriminated_base_type(&merged_parent) {
        Some(merged_parent)
      } else {
        None
      }
    })
  }

  pub(crate) fn create_discriminated_enum(
    &self,
    base_name: &str,
    schema: &ObjectSchema,
    base_struct_name: &str,
  ) -> anyhow::Result<RustType> {
    let Some(discriminator_field) = schema.discriminator.as_ref().map(|d| &d.property_name) else {
      anyhow::bail!("Failed to find discriminator property for schema '{base_name}'");
    };

    let children = self.extract_discriminator_children(schema);
    let enum_name = to_rust_type_name(base_name);

    let mut variants = vec![];
    for (disc_value, child_schema_name) in children {
      let child_type_name = to_rust_type_name(&child_schema_name);
      let variant_name = child_type_name
        .strip_prefix(&enum_name)
        .filter(|s| !s.is_empty())
        .map(|s| {
          let mut chars = s.chars();
          match chars.next() {
            None => String::new(),
            Some(first) => first.to_uppercase().chain(chars).collect(),
          }
        })
        .unwrap_or(child_type_name.clone());

      variants.push(DiscriminatedVariant {
        discriminator_value: disc_value,
        variant_name,
        type_name: format!("Box<{child_type_name}>"),
      });
    }

    let base_variant_name = to_rust_type_name(base_name.split('.').next_back().unwrap_or(base_name));
    let fallback = Some(DiscriminatedVariant {
      discriminator_value: String::new(),
      variant_name: base_variant_name,
      type_name: format!("Box<{base_struct_name}>"),
    });

    Ok(RustType::DiscriminatedEnum(DiscriminatedEnumDef {
      name: EnumToken::new(enum_name),
      docs: metadata::extract_docs(schema.description.as_ref()),
      discriminator_field: discriminator_field.clone(),
      variants,
      fallback,
      serde_mode: SerdeMode::default(),
    }))
  }

  fn extract_discriminator_children(&self, schema: &ObjectSchema) -> Vec<(String, String)> {
    let Some(mapping) = schema.discriminator.as_ref().and_then(|d| d.mapping.as_ref()) else {
      return vec![];
    };

    let mut children: Vec<_> = mapping
      .iter()
      .filter_map(|(val, ref_path)| SchemaRegistry::extract_ref_name(ref_path).map(|name| (val.clone(), name)))
      .filter(|(_, name)| {
        if let Some(filter) = &self.reachable_schemas {
          filter.contains(name)
        } else {
          true
        }
      })
      .collect();

    let mut depth_memo = HashMap::new();
    children.sort_by_key(|(_, name)| Reverse(compute_inheritance_depth(&self.graph, name, &mut depth_memo)));
    children
  }
}

#[derive(Clone)]
pub(crate) struct FieldProcessor {
  graph: Arc<SchemaRegistry>,
  optionality_policy: FieldOptionalityPolicy,
  type_resolver: TypeResolver,
}

impl FieldProcessor {
  fn new(graph: Arc<SchemaRegistry>, optionality_policy: FieldOptionalityPolicy, type_resolver: TypeResolver) -> Self {
    Self {
      graph,
      optionality_policy,
      type_resolver,
    }
  }

  fn process_single_field(
    &self,
    ctx: &FieldProcessingContext,
    prop_schema: &ObjectSchema,
    resolved_type: TypeRef,
    is_required: bool,
    discriminator_mapping: Option<&(String, String)>,
  ) -> anyhow::Result<FieldDef> {
    let discriminator_info = Self::get_discriminator_info(ctx, discriminator_mapping, prop_schema);

    let should_be_optional = self.optionality_policy.is_optional(
      ctx.prop_name,
      ctx.schema,
      FieldContext {
        is_required,
        has_default: prop_schema.default.is_some(),
        is_discriminator_field: discriminator_info.is_some(),
        discriminator_has_enum: discriminator_info.as_ref().is_some_and(|i| i.has_enum),
      },
    );
    let final_type = if should_be_optional && !resolved_type.nullable {
      resolved_type.with_option()
    } else {
      resolved_type
    };

    let metadata = FieldMetadata::from_schema(ctx.prop_name, is_required, prop_schema, &final_type);
    let rust_field_name = to_rust_field_name(ctx.prop_name);
    let serde_attrs = if rust_field_name == ctx.prop_name {
      vec![]
    } else {
      vec![SerdeAttribute::Rename(ctx.prop_name.to_string())]
    };

    let disc_attrs =
      Self::apply_discriminator_attributes(metadata, serde_attrs, &final_type, discriminator_info.as_ref());

    let field = FieldDefBuilder::default()
      .name(to_rust_field_name(ctx.prop_name))
      .rust_type(final_type)
      .docs(disc_attrs.metadata.docs)
      .serde_attrs(disc_attrs.serde_attrs)
      .extra_attrs(disc_attrs.extra_attrs)
      .validation_attrs(disc_attrs.metadata.validation_attrs)
      .default_value(disc_attrs.metadata.default_value)
      .deprecated(disc_attrs.metadata.deprecated)
      .multiple_of(disc_attrs.metadata.multiple_of)
      .build()?;

    Ok(field)
  }

  fn get_discriminator_info(
    ctx: &FieldProcessingContext,
    discriminator_mapping: Option<&(String, String)>,
    prop_schema: &ObjectSchema,
  ) -> Option<DiscriminatorInfo> {
    let is_child_discriminator = discriminator_mapping
      .as_ref()
      .is_some_and(|(prop, _)| prop == ctx.prop_name);

    let is_base_discriminator = ctx
      .schema
      .discriminator
      .as_ref()
      .is_some_and(|d| d.property_name == ctx.prop_name);

    let has_enum = !prop_schema.enum_values.is_empty();

    if is_child_discriminator {
      let (_, value) = discriminator_mapping?;
      Some(DiscriminatorInfo {
        value: Some(DefaultAtom::from(value.as_str())),
        is_base: false,
        has_enum,
      })
    } else if is_base_discriminator {
      Some(DiscriminatorInfo {
        value: None,
        is_base: true,
        has_enum,
      })
    } else {
      None
    }
  }

  pub(crate) fn apply_discriminator_attributes(
    mut metadata: FieldMetadata,
    mut serde_attrs: Vec<SerdeAttribute>,
    final_type: &TypeRef,
    discriminator_info: Option<&DiscriminatorInfo>,
  ) -> DiscriminatorAttributesResult {
    let should_hide = discriminator_info
      .as_ref()
      .is_some_and(|d| d.value.is_some() || (d.is_base && !d.has_enum));

    if !should_hide {
      return DiscriminatorAttributesResult {
        metadata,
        serde_attrs,
        extra_attrs: vec![],
      };
    }

    let disc_info = discriminator_info.expect("checked above");

    metadata.docs.clear();
    metadata.validation_attrs.clear();
    let extra_attrs = vec![HIDDEN.to_string()];

    if let Some(ref disc_value) = disc_info.value {
      metadata.default_value = Some(serde_json::Value::String(disc_value.to_string()));
      serde_attrs.push(SerdeAttribute::SkipDeserializing);
      serde_attrs.push(SerdeAttribute::Default);
    } else {
      serde_attrs.push(SerdeAttribute::Skip);
      if final_type.is_string_like() {
        metadata.default_value = Some(serde_json::Value::String(String::new()));
      }
    }

    DiscriminatorAttributesResult {
      metadata,
      serde_attrs,
      extra_attrs,
    }
  }

  fn prepare_additional_properties(&self, schema: &ObjectSchema) -> anyhow::Result<AdditionalPropertiesResult> {
    let mut serde_attrs = vec![];
    let mut additional_field = None;

    if let Some(ref additional) = schema.additional_properties {
      match additional {
        Schema::Boolean(b) if !b.0 => {
          serde_attrs.push(SerdeAttribute::DenyUnknownFields);
        }
        Schema::Object(schema_ref) => {
          let additional_schema = schema_ref
            .resolve(self.graph.spec())
            .with_context(|| "Schema resolution failed for additionalProperties")?;

          let value_type = self.type_resolver.schema_to_type_ref(&additional_schema)?;
          let map_type = TypeRef::new(format!(
            "std::collections::HashMap<String, {}>",
            value_type.to_rust_type()
          ));
          additional_field = Some(FieldDef {
            name: FieldNameToken::new("additional_properties"),
            docs: vec!["Additional properties not defined in the schema.".to_string()],
            rust_type: map_type,
            serde_attrs: vec![SerdeAttribute::Flatten],
            ..Default::default()
          });
        }
        Schema::Boolean(_) => {}
      }
    }
    Ok(AdditionalPropertiesResult {
      serde_attrs,
      additional_field,
    })
  }
}

fn is_discriminated_base_type(schema: &ObjectSchema) -> bool {
  schema
    .discriminator
    .as_ref()
    .and_then(|d| d.mapping.as_ref().map(|m| !m.is_empty()))
    .unwrap_or(false)
    && !schema.properties.is_empty()
}

fn compute_inheritance_depth(graph: &SchemaRegistry, schema_name: &str, memo: &mut HashMap<String, usize>) -> usize {
  if let Some(&depth) = memo.get(schema_name) {
    return depth;
  }
  let Some(schema) = graph.get_schema(schema_name) else {
    return 0;
  };

  let depth = if schema.all_of.is_empty() {
    0
  } else {
    schema
      .all_of
      .iter()
      .filter_map(ReferenceExtractor::extract_ref_name_from_obj_ref)
      .map(|parent| compute_inheritance_depth(graph, &parent, memo))
      .max()
      .unwrap_or(0)
      + 1
  };

  memo.insert(schema_name.to_string(), depth);
  depth
}

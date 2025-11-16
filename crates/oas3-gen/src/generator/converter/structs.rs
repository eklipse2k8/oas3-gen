use std::collections::{BTreeMap, BTreeSet, HashMap};

use oas3::spec::{Discriminator, ObjectOrReference, ObjectSchema, Schema};

use super::{
  DISCRIMINATED_BASE_SUFFIX, MERGED_SCHEMA_CACHE_SUFFIX,
  constants::{doc_attrs, serde_attrs},
  error::ConversionResult,
  field_optionality::{FieldOptionalityContext, FieldOptionalityPolicy},
  metadata::{self, FieldMetadata},
  type_resolver::TypeResolver,
  utils,
};
use crate::{
  generator::{
    ast::{DiscriminatedEnumDef, DiscriminatedVariant, FieldDef, RustType, StructDef, StructKind, TypeRef},
    schema_graph::SchemaGraph,
  },
  reserved::to_rust_type_name,
};

struct FieldProcessingContext<'a> {
  parent_name: &'a str,
  prop_name: &'a str,
  schema: &'a ObjectSchema,
  policy: utils::InlinePolicy,
}

struct DiscriminatorInfo {
  value: Option<String>,
  is_base: bool,
  has_enum: bool,
}

#[derive(Clone)]
pub(crate) struct StructConverter<'a> {
  graph: &'a SchemaGraph,
  type_resolver: TypeResolver<'a>,
  reachable_schemas: Option<std::collections::BTreeSet<String>>,
  optionality_policy: FieldOptionalityPolicy,
}

impl<'a> StructConverter<'a> {
  pub(crate) fn new(
    graph: &'a SchemaGraph,
    type_resolver: TypeResolver<'a>,
    reachable_schemas: Option<std::collections::BTreeSet<String>>,
    optionality_policy: FieldOptionalityPolicy,
  ) -> Self {
    Self {
      graph,
      type_resolver,
      reachable_schemas,
      optionality_policy,
    }
  }

  pub(crate) fn convert_all_of_schema(&self, name: &str, schema: &ObjectSchema) -> ConversionResult<Vec<RustType>> {
    let mut merged_schema_cache = HashMap::new();

    if let Some((_, parent_schema)) = self.detect_discriminated_parent(schema, &mut merged_schema_cache) {
      return self.convert_discriminated_child(name, schema, &parent_schema, &mut merged_schema_cache);
    }

    let merged_schema = self.merge_all_of_schema(schema)?;
    let (main_type, inline_types) = self.convert_struct(name, &merged_schema, None)?;

    self.finalize_struct_types(name, &merged_schema, main_type, inline_types)
  }

  pub(crate) fn convert_struct(
    &self,
    name: &str,
    schema: &ObjectSchema,
    kind: Option<StructKind>,
  ) -> ConversionResult<(RustType, Vec<RustType>)> {
    let is_discriminated = utils::is_discriminated_base_type(schema);
    let struct_name = if is_discriminated {
      format!("{}{DISCRIMINATED_BASE_SUFFIX}", to_rust_type_name(name))
    } else {
      to_rust_type_name(name)
    };

    let (mut fields, inline_types) = self.convert_fields_core(
      &struct_name,
      schema,
      utils::InlinePolicy::InlineUnions,
      None,
      Some(name),
    )?;

    let (mut serde_attrs, additional_field) = self.prepare_additional_properties(schema)?;
    if let Some(field) = additional_field {
      fields.push(field);
    }

    if fields.iter().any(|f| f.default_value.is_some()) {
      serde_attrs.push(serde_attrs::DEFAULT.to_string());
    }

    let all_read_only = !fields.is_empty() && fields.iter().all(|f| f.read_only);
    let all_write_only = !fields.is_empty() && fields.iter().all(|f| f.write_only);
    let outer_attrs = utils::container_outer_attrs(&fields);

    let struct_type = RustType::Struct(StructDef {
      name: struct_name,
      docs: metadata::extract_docs(schema.description.as_ref()),
      fields,
      derives: utils::derives_for_struct(all_read_only, all_write_only),
      serde_attrs,
      outer_attrs,
      methods: vec![],
      kind: kind.unwrap_or(StructKind::Schema),
    });

    Ok((struct_type, inline_types))
  }

  fn convert_discriminated_child(
    &self,
    name: &str,
    schema: &ObjectSchema,
    parent_schema: &ObjectSchema,
    merged_schema_cache: &mut HashMap<String, ObjectSchema>,
  ) -> ConversionResult<Vec<RustType>> {
    if parent_schema.discriminator.is_none() {
      anyhow::bail!("Parent schema for discriminated child '{name}' is not a valid discriminator base");
    }

    let struct_name = to_rust_type_name(name);

    let cache_key = format!("{name}{MERGED_SCHEMA_CACHE_SUFFIX}");
    let merged_schema = if let Some(cached) = merged_schema_cache.get(&cache_key) {
      cached.clone()
    } else {
      let merged = self.merge_child_schema_with_parent(schema, parent_schema)?;
      merged_schema_cache.insert(cache_key, merged.clone());
      merged
    };

    let (fields, mut inline_types) = self.convert_fields_core(
      &struct_name,
      &merged_schema,
      utils::InlinePolicy::InlineUnions,
      None,
      Some(name),
    )?;

    let (serde_attrs, additional_field) = self.prepare_additional_properties(&merged_schema)?;
    let mut fields = fields;
    if let Some(field) = additional_field {
      fields.push(field);
    }

    let all_read_only = !fields.is_empty() && fields.iter().all(|f| f.read_only);
    let all_write_only = !fields.is_empty() && fields.iter().all(|f| f.write_only);
    let outer_attrs = utils::container_outer_attrs(&fields);

    let mut all_types = Vec::with_capacity(1 + inline_types.len());
    all_types.push(RustType::Struct(StructDef {
      name: struct_name,
      docs: metadata::extract_docs(schema.description.as_ref()),
      fields,
      derives: utils::derives_for_struct(all_read_only, all_write_only),
      serde_attrs,
      outer_attrs,
      methods: vec![],
      kind: StructKind::Schema,
    }));

    all_types.append(&mut inline_types);
    Ok(all_types)
  }

  pub(crate) fn finalize_struct_types(
    &self,
    name: &str,
    schema: &ObjectSchema,
    main_type: RustType,
    mut inline_types: Vec<RustType>,
  ) -> ConversionResult<Vec<RustType>> {
    let is_discriminated = utils::is_discriminated_base_type(schema);
    let capacity = if is_discriminated { 2 } else { 1 } + inline_types.len();
    let mut all_types = Vec::with_capacity(capacity);

    if is_discriminated {
      let base_struct_name = match &main_type {
        RustType::Struct(def) => def.name.clone(),
        _ => format!("{}{DISCRIMINATED_BASE_SUFFIX}", to_rust_type_name(name)),
      };
      let discriminated_enum = self.create_discriminated_enum(name, schema, &base_struct_name)?;
      all_types.push(discriminated_enum);
    }

    all_types.push(main_type);
    all_types.append(&mut inline_types);
    Ok(all_types)
  }

  fn create_discriminated_enum(
    &self,
    base_name: &str,
    schema: &ObjectSchema,
    base_struct_name: &str,
  ) -> ConversionResult<RustType> {
    let Some(discriminator_field) = schema.discriminator.as_ref().map(|d| &d.property_name) else {
      anyhow::bail!("Failed to find discriminator property for schema '{base_name}'");
    };

    let children = utils::extract_discriminator_children(self.graph, schema, self.reachable_schemas.as_ref());
    let enum_name = to_rust_type_name(base_name);

    let mut variants = Vec::new();
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
      name: enum_name,
      docs: metadata::extract_docs(schema.description.as_ref()),
      discriminator_field: discriminator_field.clone(),
      variants,
      fallback,
    }))
  }

  fn convert_fields_core(
    &self,
    parent_name: &str,
    schema: &ObjectSchema,
    policy: utils::InlinePolicy,
    exclude_field: Option<&str>,
    schema_name: Option<&str>,
  ) -> ConversionResult<(Vec<FieldDef>, Vec<RustType>)> {
    let num_properties = schema.properties.len();
    let required_set: std::collections::HashSet<&String> = schema.required.iter().collect();

    let discriminator_mapping = schema_name.and_then(|name| self.graph.get_discriminator_mapping(name));

    let mut fields = Vec::with_capacity(num_properties);
    let mut inline_types = Vec::new();

    for (prop_name, prop_schema_ref) in &schema.properties {
      if exclude_field == Some(prop_name.as_str()) {
        continue;
      }

      let is_required = required_set.contains(prop_name);

      let ctx = FieldProcessingContext {
        parent_name,
        prop_name,
        schema,
        policy,
      };

      let (field, mut generated_types) =
        self.process_single_field(&ctx, prop_schema_ref, is_required, discriminator_mapping)?;

      fields.push(field);
      inline_types.append(&mut generated_types);
    }

    utils::deduplicate_field_names(&mut fields);
    Ok((fields, inline_types))
  }

  fn process_single_field(
    &self,
    ctx: &FieldProcessingContext,
    prop_schema_ref: &ObjectOrReference<ObjectSchema>,
    is_required: bool,
    discriminator_mapping: Option<&(String, String)>,
  ) -> ConversionResult<(FieldDef, Vec<RustType>)> {
    let prop_schema = prop_schema_ref
      .resolve(self.graph.spec())
      .map_err(|e| anyhow::anyhow!("Schema resolution failed for property '{}': {e}", ctx.prop_name))?;

    let (base_type, generated_types) = self.resolve_field_type(
      ctx.parent_name,
      ctx.prop_name,
      &prop_schema,
      prop_schema_ref,
      ctx.policy,
    )?;

    let discriminator_info = Self::get_discriminator_info(ctx, discriminator_mapping, &prop_schema);

    let should_be_optional = self.compute_field_optionality(
      ctx.prop_name,
      ctx.schema,
      &prop_schema,
      is_required,
      discriminator_info.as_ref(),
    );
    let final_type = utils::apply_optionality(base_type, should_be_optional);

    let metadata = FieldMetadata::from_schema(ctx.prop_name, is_required, &prop_schema);
    let serde_attrs = utils::serde_renamed_if_needed(ctx.prop_name);

    let (metadata, serde_attrs, extra_attrs, regex_validation) =
      Self::apply_discriminator_attributes(metadata, serde_attrs, &final_type, discriminator_info.as_ref());

    let regex_validation =
      regex_validation.or_else(|| metadata::filter_regex_validation(&final_type, metadata.regex_validation.clone()));

    let field = utils::build_field_def(
      ctx.prop_name,
      final_type,
      serde_attrs,
      metadata,
      regex_validation,
      extra_attrs,
    );

    Ok((field, generated_types))
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
        value: Some(value.clone()),
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

  fn compute_field_optionality(
    &self,
    prop_name: &str,
    parent_schema: &ObjectSchema,
    prop_schema: &ObjectSchema,
    is_required: bool,
    discriminator_info: Option<&DiscriminatorInfo>,
  ) -> bool {
    let ctx = FieldOptionalityContext {
      prop_name,
      parent_schema,
      is_required,
      has_default: prop_schema.default.is_some(),
      is_discriminator_field: discriminator_info.is_some(),
      discriminator_has_enum: discriminator_info.as_ref().is_some_and(|info| info.has_enum),
    };

    self.optionality_policy.compute_optionality(&ctx)
  }

  fn resolve_field_type(
    &self,
    parent_name: &str,
    prop_name: &str,
    prop_schema: &ObjectSchema,
    prop_schema_ref: &ObjectOrReference<ObjectSchema>,
    policy: utils::InlinePolicy,
  ) -> ConversionResult<(TypeRef, Vec<RustType>)> {
    match policy {
      utils::InlinePolicy::InlineUnions => {
        self
          .type_resolver
          .resolve_property_type_with_inlines(parent_name, prop_name, prop_schema, prop_schema_ref)
      }
    }
  }

  fn apply_discriminator_attributes(
    mut metadata: FieldMetadata,
    mut serde_attrs: Vec<String>,
    final_type: &TypeRef,
    discriminator_info: Option<&DiscriminatorInfo>,
  ) -> (FieldMetadata, Vec<String>, Vec<String>, Option<String>) {
    let Some(disc_info) = discriminator_info else {
      let regex = metadata.regex_validation.clone();
      return (metadata, serde_attrs, Vec::new(), regex);
    };

    if let Some(ref disc_value) = disc_info.value {
      metadata.docs.clear();
      metadata.validation_attrs.clear();
      let extra_attrs = vec![doc_attrs::HIDDEN.to_string()];
      metadata.default_value = Some(serde_json::Value::String(disc_value.clone()));
      serde_attrs.push(serde_attrs::SKIP_DESERIALIZING.to_string());
      serde_attrs.push(serde_attrs::DEFAULT.to_string());
      (metadata, serde_attrs, extra_attrs, None)
    } else if disc_info.is_base && !disc_info.has_enum {
      metadata.docs.clear();
      metadata.validation_attrs.clear();
      let extra_attrs = vec![doc_attrs::HIDDEN.to_string()];
      serde_attrs.push(serde_attrs::SKIP.to_string());
      if final_type.is_string_like() {
        metadata.default_value = Some(serde_json::Value::String(String::new()));
      }
      (metadata, serde_attrs, extra_attrs, None)
    } else {
      let regex = metadata.regex_validation.clone();
      (metadata, serde_attrs, Vec::new(), regex)
    }
  }

  fn prepare_additional_properties(&self, schema: &ObjectSchema) -> ConversionResult<(Vec<String>, Option<FieldDef>)> {
    let mut serde_attrs = Vec::new();
    let mut additional_field = None;

    if let Some(ref additional) = schema.additional_properties {
      match additional {
        Schema::Boolean(b) if !b.0 => {
          serde_attrs.push(serde_attrs::DENY_UNKNOWN_FIELDS.to_string());
        }
        Schema::Object(schema_ref) => {
          let additional_schema = schema_ref
            .resolve(self.graph.spec())
            .map_err(|e| anyhow::anyhow!("Schema resolution failed for additionalProperties: {e}"))?;
          let value_type = self.type_resolver.schema_to_type_ref(&additional_schema)?;
          let map_type = TypeRef::new(format!(
            "std::collections::HashMap<String, {}>",
            value_type.to_rust_type()
          ));
          additional_field = Some(FieldDef {
            name: "additional_properties".to_string(),
            docs: vec!["/// Additional properties not defined in the schema.".to_string()],
            rust_type: map_type,
            serde_attrs: vec![serde_attrs::FLATTEN.to_string()],
            ..Default::default()
          });
        }
        Schema::Boolean(_) => {}
      }
    }
    Ok((serde_attrs, additional_field))
  }

  fn merge_child_schema_with_parent(
    &self,
    child_schema: &ObjectSchema,
    parent_schema: &ObjectSchema,
  ) -> ConversionResult<ObjectSchema> {
    let mut merged_properties = BTreeMap::new();
    let mut merged_required = BTreeSet::new();
    let mut merged_discriminator = parent_schema.discriminator.clone();

    self.collect_all_of_properties(
      child_schema,
      &mut merged_properties,
      &mut merged_required,
      &mut merged_discriminator,
    )?;

    let mut merged_schema = child_schema.clone();
    merged_schema.properties = merged_properties;
    merged_schema.required = merged_required.into_iter().collect();
    merged_schema.discriminator = merged_discriminator;
    merged_schema.all_of.clear();

    if merged_schema.additional_properties.is_none() {
      merged_schema
        .additional_properties
        .clone_from(&parent_schema.additional_properties);
    }

    Ok(merged_schema)
  }

  fn merge_all_of_schema(&self, schema: &ObjectSchema) -> ConversionResult<ObjectSchema> {
    let mut merged_properties = BTreeMap::new();
    let mut merged_required = BTreeSet::new();
    let mut merged_discriminator = None;

    self.collect_all_of_properties(
      schema,
      &mut merged_properties,
      &mut merged_required,
      &mut merged_discriminator,
    )?;

    let mut merged_schema = schema.clone();
    merged_schema.properties = merged_properties;
    merged_schema.required = merged_required.into_iter().collect();
    merged_schema.discriminator.clone_from(&merged_discriminator);

    Ok(merged_schema)
  }

  fn collect_all_of_properties(
    &self,
    schema: &ObjectSchema,
    properties: &mut BTreeMap<String, ObjectOrReference<ObjectSchema>>,
    required: &mut BTreeSet<String>,
    discriminator: &mut Option<Discriminator>,
  ) -> ConversionResult<()> {
    for all_of_ref in &schema.all_of {
      let all_of_schema = all_of_ref
        .resolve(self.graph.spec())
        .map_err(|e| anyhow::anyhow!("Schema resolution failed for allOf item: {e}"))?;
      self.collect_all_of_properties(&all_of_schema, properties, required, discriminator)?;
    }

    for (prop_name, prop_ref) in &schema.properties {
      properties.insert(prop_name.clone(), prop_ref.clone());
    }
    required.extend(schema.required.iter().cloned());

    if schema.discriminator.is_some() {
      discriminator.clone_from(&schema.discriminator);
    }
    Ok(())
  }

  fn get_merged_schema(
    &self,
    schema_name: &str,
    schema: &ObjectSchema,
    merged_schema_cache: &mut HashMap<String, ObjectSchema>,
  ) -> ConversionResult<ObjectSchema> {
    if let Some(cached) = merged_schema_cache.get(schema_name) {
      return Ok(cached.clone());
    }

    let merged = self.merge_all_of_schema(schema)?;
    merged_schema_cache.insert(schema_name.to_string(), merged.clone());
    Ok(merged)
  }

  fn detect_discriminated_parent(
    &self,
    schema: &ObjectSchema,
    merged_schema_cache: &mut HashMap<String, ObjectSchema>,
  ) -> Option<(String, ObjectSchema)> {
    if schema.all_of.is_empty() {
      return None;
    }

    schema.all_of.iter().find_map(|all_of_ref| {
      let ObjectOrReference::Ref { ref_path, .. } = all_of_ref else {
        return None;
      };
      let parent_name = SchemaGraph::extract_ref_name(ref_path)?;
      let parent_schema = self.graph.get_schema(&parent_name)?;

      parent_schema.discriminator.as_ref()?;

      let merged_parent = self
        .get_merged_schema(&parent_name, parent_schema, merged_schema_cache)
        .ok()?;
      if utils::is_discriminated_base_type(&merged_parent) {
        Some((parent_name, merged_parent))
      } else {
        None
      }
    })
  }
}

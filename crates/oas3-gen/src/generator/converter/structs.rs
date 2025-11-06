use std::collections::{BTreeMap, BTreeSet, HashSet};

use oas3::spec::{Discriminator, ObjectOrReference, ObjectSchema, Schema};

use super::{
  error::ConversionResult,
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

#[derive(Clone)]
pub(crate) struct StructConverter<'a> {
  graph: &'a SchemaGraph,
  type_resolver: TypeResolver<'a>,
}

impl<'a> StructConverter<'a> {
  pub(crate) fn new(graph: &'a SchemaGraph, type_resolver: TypeResolver<'a>) -> Self {
    Self { graph, type_resolver }
  }

  pub(crate) fn convert_all_of_schema(&self, name: &str, schema: &ObjectSchema) -> ConversionResult<Vec<RustType>> {
    if let Some((_, parent_schema)) = self.detect_discriminated_parent(schema) {
      return self.convert_discriminated_child(name, schema, &parent_schema);
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
      format!("{}Base", to_rust_type_name(name))
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
      serde_attrs.push("default".to_string());
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
  ) -> ConversionResult<Vec<RustType>> {
    if parent_schema.discriminator.is_none() {
      anyhow::bail!("Parent schema for discriminated child '{name}' is not a valid discriminator base");
    }

    let struct_name = to_rust_type_name(name);
    let merged_schema = self.merge_child_schema_with_parent(schema, parent_schema)?;

    let (mut fields, mut inline_types) = self.convert_fields_core(
      &struct_name,
      &merged_schema,
      utils::InlinePolicy::InlineUnions,
      None,
      Some(name),
    )?;

    let (serde_attrs, additional_field) = self.prepare_additional_properties(&merged_schema)?;
    if let Some(field) = additional_field {
      fields.push(field);
    }

    let all_read_only = !fields.is_empty() && fields.iter().all(|f| f.read_only);
    let all_write_only = !fields.is_empty() && fields.iter().all(|f| f.write_only);

    let mut all_types = vec![RustType::Struct(StructDef {
      name: struct_name,
      docs: metadata::extract_docs(schema.description.as_ref()),
      fields: fields.clone(),
      derives: utils::derives_for_struct(all_read_only, all_write_only),
      serde_attrs,
      outer_attrs: utils::container_outer_attrs(&fields),
      methods: vec![],
      kind: StructKind::Schema,
    })];

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
    let mut all_types = Vec::new();

    if utils::is_discriminated_base_type(schema) {
      let base_struct_name = match &main_type {
        RustType::Struct(def) => def.name.clone(),
        _ => format!("{}Base", to_rust_type_name(name)),
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

    let children = utils::extract_discriminator_children(self.graph, schema);
    let enum_name = to_rust_type_name(base_name);

    let mut variants = Vec::new();
    for (disc_value, child_schema_name) in children {
      let child_type_name = to_rust_type_name(&child_schema_name);
      let variant_name = child_type_name
        .strip_prefix(&enum_name)
        .filter(|s| !s.is_empty())
        .unwrap_or(&child_type_name)
        .to_string();

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
    let required_set: HashSet<&str> = schema.required.iter().map(String::as_str).collect();
    let mut fields = Vec::new();
    let mut inline_types = Vec::new();

    for (prop_name, prop_schema_ref) in &schema.properties {
      if exclude_field == Some(prop_name.as_str()) {
        continue;
      }

      let is_required = required_set.contains(prop_name.as_str());
      let (field, mut generated_types) = self.process_single_field(
        parent_name,
        prop_name,
        prop_schema_ref,
        is_required,
        policy,
        schema_name,
        schema,
      )?;

      fields.push(field);
      inline_types.append(&mut generated_types);
    }

    utils::deduplicate_field_names(&mut fields);
    Ok((fields, inline_types))
  }

  #[allow(clippy::too_many_arguments)]
  fn process_single_field(
    &self,
    parent_name: &str,
    prop_name: &str,
    prop_schema_ref: &ObjectOrReference<ObjectSchema>,
    is_required: bool,
    policy: utils::InlinePolicy,
    schema_name: Option<&str>,
    schema: &ObjectSchema,
  ) -> ConversionResult<(FieldDef, Vec<RustType>)> {
    let prop_schema = prop_schema_ref
      .resolve(self.graph.spec())
      .map_err(|e| anyhow::anyhow!("Schema resolution failed for property '{prop_name}': {e}"))?;

    let (base_type, generated_types) =
      self.resolve_field_type(parent_name, prop_name, &prop_schema, prop_schema_ref, policy)?;

    let final_type = utils::apply_optionality(base_type, !is_required);

    let metadata = FieldMetadata::from_schema(prop_name, is_required, &prop_schema);
    let serde_attrs = utils::serde_renamed_if_needed(prop_name);

    let (metadata, serde_attrs, extra_attrs, regex_validation) = self.apply_discriminator_attributes(
      prop_name,
      schema_name,
      schema.discriminator.as_ref(),
      metadata,
      serde_attrs,
      &final_type,
    );

    let regex_validation =
      regex_validation.or_else(|| metadata::filter_regex_validation(&final_type, metadata.regex_validation.clone()));

    let field = utils::build_field_def(
      prop_name,
      final_type,
      serde_attrs,
      metadata,
      regex_validation,
      extra_attrs,
    );

    Ok((field, generated_types))
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
    &self,
    prop_name: &str,
    schema_name: Option<&str>,
    schema_discriminator: Option<&Discriminator>,
    mut metadata: FieldMetadata,
    mut serde_attrs: Vec<String>,
    final_type: &TypeRef,
  ) -> (FieldMetadata, Vec<String>, Vec<String>, Option<String>) {
    let is_base_discriminator = schema_discriminator.is_some_and(|d| d.property_name == prop_name);
    let discriminator_info = schema_name.and_then(|name| utils::find_discriminator_mapping_value(self.graph, name));
    let is_child_discriminator = discriminator_info.as_ref().is_some_and(|(p, _)| p == prop_name);

    if !is_base_discriminator && !is_child_discriminator {
      let regex = metadata.regex_validation.clone();
      return (metadata, serde_attrs, Vec::new(), regex);
    }

    metadata.docs.clear();
    metadata.validation_attrs.clear();
    let extra_attrs = vec!["#[doc(hidden)]".to_string()];

    if let Some((_, disc_value)) = discriminator_info {
      metadata.default_value = Some(serde_json::Value::String(disc_value));
      serde_attrs.push("skip_deserializing".to_string());
      serde_attrs.push("default".to_string());
    } else {
      serde_attrs.push("skip".to_string());
      if final_type.is_string_like() {
        metadata.default_value = Some(serde_json::Value::String(String::new()));
      }
    }

    (metadata, serde_attrs, extra_attrs, None)
  }

  fn prepare_additional_properties(&self, schema: &ObjectSchema) -> ConversionResult<(Vec<String>, Option<FieldDef>)> {
    let mut serde_attrs = Vec::new();
    let mut additional_field = None;

    if let Some(ref additional) = schema.additional_properties {
      match additional {
        Schema::Boolean(b) if !b.0 => {
          serde_attrs.push("deny_unknown_fields".to_string());
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
            serde_attrs: vec!["flatten".to_string()],
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

  fn detect_discriminated_parent(&self, schema: &ObjectSchema) -> Option<(String, ObjectSchema)> {
    schema.all_of.iter().find_map(|all_of_ref| {
      let ObjectOrReference::Ref { ref_path, .. } = all_of_ref else {
        return None;
      };
      let parent_name = SchemaGraph::extract_ref_name(ref_path)?;
      let parent_schema = self.graph.get_schema(&parent_name)?;

      let merged_parent = self.merge_all_of_schema(parent_schema).ok()?;
      if utils::is_discriminated_base_type(&merged_parent) {
        Some((parent_name, merged_parent))
      } else {
        None
      }
    })
  }
}

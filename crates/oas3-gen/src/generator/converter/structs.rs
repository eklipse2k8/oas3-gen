use std::{
  collections::{BTreeMap, BTreeSet, HashSet},
  sync::Arc,
};

use anyhow::Context as _;
use oas3::spec::{ObjectSchema, Schema};

use super::{
  CodegenConfig, ConversionOutput, SchemaExt, cache::SharedSchemaCache, discriminator::DiscriminatorConverter,
  fields::FieldConverter, struct_summaries::StructSummary, type_resolver::TypeResolver,
};
use crate::generator::{
  ast::{
    Documentation, FieldDef, OuterAttr, RustType, SerdeAttribute, StructDef, StructKind, StructToken, TypeRef,
    tokens::FieldNameToken,
  },
  naming::{constants::DISCRIMINATED_BASE_SUFFIX, identifiers::to_rust_type_name},
  schema_registry::{DiscriminatorMapping, SchemaRegistry},
};

#[derive(Clone, Debug)]
struct AdditionalPropertiesResult {
  serde_attrs: Vec<SerdeAttribute>,
  additional_field: Option<FieldDef>,
}

/// Converter for OpenAPI object schemas into Rust Structs.
#[derive(Clone, Debug)]
pub(crate) struct StructConverter {
  type_resolver: TypeResolver,
  field_converter: FieldConverter,
}

impl StructConverter {
  pub(crate) fn new(
    graph: &Arc<SchemaRegistry>,
    config: &CodegenConfig,
    reachable_schemas: Option<Arc<BTreeSet<String>>>,
  ) -> Self {
    let type_resolver = TypeResolver::builder()
      .config(config.clone())
      .graph(graph.clone())
      .maybe_reachable_schemas(reachable_schemas)
      .build();
    let field_converter = FieldConverter::new(config);
    Self {
      type_resolver,
      field_converter,
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
    let required_set: HashSet<&String> = schema.required.iter().collect();

    let discriminator_mapping = schema_name
      .and_then(|name| self.type_resolver.graph().mapping(name))
      .map(DiscriminatorMapping::as_tuple);

    let mut fields = Vec::with_capacity(num_properties);
    let mut inline_types = vec![];

    for (prop_name, prop_schema_ref) in &schema.properties {
      if exclude_field == Some(prop_name.as_str()) {
        continue;
      }

      let is_required = required_set.contains(prop_name);

      let prop_schema = prop_schema_ref
        .resolve(self.type_resolver.graph().spec())
        .context(format!("Schema resolution failed for property '{prop_name}'"))?;

      let cache_borrow = cache.as_deref_mut();
      let resolved = self.type_resolver.resolve_property_type(
        parent_name,
        prop_name,
        &prop_schema,
        prop_schema_ref,
        cache_borrow,
      )?;

      let field = self.field_converter.convert_field(
        prop_name,
        schema,
        &prop_schema,
        resolved.result,
        is_required,
        discriminator_mapping.as_ref(),
      );
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
    cache: Option<&mut SharedSchemaCache>,
  ) -> anyhow::Result<Vec<RustType>> {
    let graph = self.type_resolver.graph();

    let merged_info = graph
      .merged(name)
      .ok_or_else(|| anyhow::anyhow!("Schema '{name}' not found in registry"))?;

    let handler = DiscriminatorConverter::new(graph, None);
    if let Some(parent_info) = handler.detect_discriminated_parent(name) {
      let parent_merged = graph
        .merged(&parent_info.parent_name)
        .ok_or_else(|| anyhow::anyhow!("Parent schema '{}' not found", parent_info.parent_name))?;
      return self.convert_discriminated_child(name, &merged_info.schema, &parent_merged.schema, cache);
    }

    let effective_schema = graph.resolved(name).unwrap_or(&merged_info.schema);

    let result = self.convert_struct(name, effective_schema, None, cache)?;
    self.finalize_struct_types(name, effective_schema, result.result, result.inline_types)
  }

  /// Converts a standard object schema into a Rust Struct.
  pub(crate) fn convert_struct(
    &self,
    name: &str,
    schema: &ObjectSchema,
    kind: Option<StructKind>,
    mut cache: Option<&mut SharedSchemaCache>,
  ) -> anyhow::Result<ConversionOutput<RustType>> {
    let is_discriminated = schema.is_discriminated_base_type();
    let struct_name = if is_discriminated {
      StructToken::from(format!("{}{DISCRIMINATED_BASE_SUFFIX}", to_rust_type_name(name)))
    } else {
      StructToken::from_raw(name)
    };

    let cache_for_fields = cache.as_deref_mut();
    let field_result = self.convert_fields(struct_name.as_str(), schema, None, Some(name), cache_for_fields)?;
    let additional_props = self.prepare_additional_properties(schema)?;

    let mut fields = field_result.result;
    let mut serde_attrs = additional_props.serde_attrs;

    if let Some(field) = additional_props.additional_field {
      fields.push(field);
    }

    let has_defaults = fields.iter().any(|f| f.default_value.is_some());
    if has_defaults {
      serde_attrs.push(SerdeAttribute::Default);
    }

    let has_serde_as = fields.iter().any(|f| f.serde_as_attr.is_some());
    let outer_attrs = if has_serde_as { vec![OuterAttr::SerdeAs] } else { vec![] };

    let struct_def = StructDef {
      name: struct_name,
      docs: Documentation::from_optional(schema.description.as_ref()),
      fields,
      serde_attrs,
      outer_attrs,
      methods: vec![],
      kind: kind.unwrap_or(StructKind::Schema),
      ..Default::default()
    };

    if let Some(ref mut c) = cache {
      c.register_struct_summary(struct_def.name.as_str(), StructSummary::from(&struct_def));
    }

    Ok(ConversionOutput::with_inline_types(
      RustType::Struct(struct_def),
      field_result.inline_types,
    ))
  }

  fn convert_discriminated_child(
    &self,
    name: &str,
    merged_schema: &ObjectSchema,
    parent_schema: &ObjectSchema,
    cache: Option<&mut SharedSchemaCache>,
  ) -> anyhow::Result<Vec<RustType>> {
    if parent_schema.discriminator.is_none() {
      anyhow::bail!("Parent schema for discriminated child '{name}' is not a valid discriminator base");
    }

    let struct_name = StructToken::from_raw(name);

    let field_result = self.convert_fields(struct_name.as_str(), merged_schema, None, Some(name), cache)?;
    let additional_props = self.prepare_additional_properties(merged_schema)?;

    let mut fields = field_result.result;
    let mut serde_attrs = additional_props.serde_attrs;

    if let Some(field) = additional_props.additional_field {
      fields.push(field);
    }

    if fields.iter().any(|f| f.default_value.is_some()) {
      serde_attrs.push(SerdeAttribute::Default);
    }

    let has_serde_as = fields.iter().any(|f| f.serde_as_attr.is_some());
    let outer_attrs = if has_serde_as { vec![OuterAttr::SerdeAs] } else { vec![] };

    let mut all_types = Vec::with_capacity(1 + field_result.inline_types.len());
    all_types.push(RustType::Struct(StructDef {
      name: struct_name,
      docs: Documentation::from_optional(merged_schema.description.as_ref()),
      fields,
      serde_attrs,
      outer_attrs,
      kind: StructKind::Schema,
      ..Default::default()
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
    let is_discriminated = schema.is_discriminated_base_type();
    let capacity = if is_discriminated { 2 } else { 1 } + inline_types.len();
    let mut all_types = Vec::with_capacity(capacity);

    if is_discriminated {
      let base_struct_name = match &main_type {
        RustType::Struct(def) => def.name.clone(),
        _ => StructToken::from(format!("{}{DISCRIMINATED_BASE_SUFFIX}", to_rust_type_name(name))),
      };
      let discriminated_enum = self
        .type_resolver
        .build_discriminated_enum(name, schema, base_struct_name.as_str())?;
      all_types.push(discriminated_enum);
    }

    all_types.push(main_type);
    all_types.append(&mut inline_types);
    Ok(all_types)
  }

  fn prepare_additional_properties(&self, schema: &ObjectSchema) -> anyhow::Result<AdditionalPropertiesResult> {
    let mut serde_attrs = vec![];
    let mut additional_field = None;

    if let Some(ref additional) = schema.additional_properties {
      match additional {
        Schema::Boolean(b) if !b.0 => {
          serde_attrs.push(SerdeAttribute::DenyUnknownFields);
        }
        Schema::Object(_) => {
          let value_type = self.type_resolver.resolve_additional_properties_type(additional)?;
          let map_type = TypeRef::new(format!(
            "std::collections::HashMap<String, {}>",
            value_type.to_rust_type()
          ));
          additional_field = Some(
            FieldDef::builder()
              .name(FieldNameToken::from_raw("additional_properties"))
              .docs(Documentation::from_lines([
                "Additional properties not defined in the schema.",
              ]))
              .rust_type(map_type)
              .serde_attrs(BTreeSet::from([SerdeAttribute::Flatten]))
              .build(),
          );
        }
        Schema::Boolean(_) => {}
      }
    }
    Ok(AdditionalPropertiesResult {
      serde_attrs,
      additional_field,
    })
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

use std::{
  collections::{BTreeMap, BTreeSet, HashMap, HashSet},
  sync::Arc,
};

use anyhow::Context as _;
use oas3::spec::{ObjectSchema, Schema};

use super::{
  CodegenConfig, ConversionOutput, SchemaExt,
  cache::SharedSchemaCache,
  discriminator::{apply_discriminator_attributes, get_discriminator_info},
  field_optionality::{FieldContext, FieldOptionalityPolicy},
  metadata::{self, FieldMetadata},
  type_resolver::TypeResolver,
};
use crate::generator::{
  ast::{
    FieldDef, FieldDefBuilder, RustType, SerdeAttribute, StructDef, StructKind, StructToken, TypeRef,
    tokens::FieldNameToken,
  },
  naming::{
    constants::{DISCRIMINATED_BASE_SUFFIX, MERGED_SCHEMA_CACHE_SUFFIX},
    identifiers::{to_rust_field_name, to_rust_type_name},
  },
  schema_registry::SchemaRegistry,
};

struct AdditionalPropertiesResult {
  serde_attrs: Vec<SerdeAttribute>,
  additional_field: Option<FieldDef>,
}

struct FieldProcessingContext<'a> {
  prop_name: &'a str,
  schema: &'a ObjectSchema,
}

/// Converter for OpenAPI object schemas into Rust Structs.
#[derive(Clone)]
pub(crate) struct StructConverter {
  type_resolver: TypeResolver,
  field_processor: FieldProcessor,
}

impl StructConverter {
  pub(crate) fn new(
    graph: &Arc<SchemaRegistry>,
    config: CodegenConfig,
    reachable_schemas: Option<Arc<BTreeSet<String>>>,
    optionality_policy: FieldOptionalityPolicy,
  ) -> Self {
    let type_resolver = TypeResolver::new_with_filter(graph, config, reachable_schemas, optionality_policy);
    let field_processor = FieldProcessor::new(optionality_policy, type_resolver.clone());
    Self {
      type_resolver,
      field_processor,
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

    let discriminator_mapping = schema_name.and_then(|name| self.type_resolver.graph().get_discriminator_mapping(name));

    let mut fields = Vec::with_capacity(num_properties);
    let mut inline_types = vec![];

    for (prop_name, prop_schema_ref) in &schema.properties {
      if exclude_field == Some(prop_name.as_str()) {
        continue;
      }

      let is_required = required_set.contains(prop_name);

      let prop_schema = prop_schema_ref
        .resolve(self.type_resolver.graph().spec())
        .with_context(|| format!("Schema resolution failed for property '{prop_name}'"))?;

      let cache_borrow = cache.as_deref_mut();
      let resolved = self.type_resolver.resolve_property_type(
        parent_name,
        prop_name,
        &prop_schema,
        prop_schema_ref,
        cache_borrow,
      )?;

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
      .type_resolver
      .detect_discriminated_parent(schema, &mut merged_schema_cache)
    {
      return self.convert_discriminated_child(name, schema, &parent_schema, &mut merged_schema_cache, cache);
    }

    let merged_schema = self.type_resolver.merge_all_of_schema(schema)?;
    let result = self.convert_struct(name, &merged_schema, None, cache)?;

    self.finalize_struct_types(name, &merged_schema, result.result, result.inline_types)
  }

  /// Converts a standard object schema into a Rust Struct.
  pub(crate) fn convert_struct(
    &self,
    name: &str,
    schema: &ObjectSchema,
    kind: Option<StructKind>,
    cache: Option<&mut SharedSchemaCache>,
  ) -> anyhow::Result<ConversionOutput<RustType>> {
    let is_discriminated = schema.is_discriminated_base_type();
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
      serde_attrs,
      outer_attrs: vec![],
      methods: vec![],
      kind: kind.unwrap_or(StructKind::Schema),
      ..Default::default()
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
      let new_merged = self
        .type_resolver
        .merge_child_schema_with_parent(schema, parent_schema)?;
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
      serde_attrs,
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
pub(crate) struct FieldProcessor {
  optionality_policy: FieldOptionalityPolicy,
  type_resolver: TypeResolver,
}

impl FieldProcessor {
  fn new(optionality_policy: FieldOptionalityPolicy, type_resolver: TypeResolver) -> Self {
    Self {
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
    let discriminator_info = get_discriminator_info(ctx.prop_name, ctx.schema, prop_schema, discriminator_mapping);

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

    let disc_attrs = apply_discriminator_attributes(metadata, serde_attrs, &final_type, discriminator_info.as_ref());

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

use std::{
  collections::{BTreeMap, BTreeSet, HashMap},
  rc::Rc,
};

use anyhow::Context as _;
use oas3::spec::{ObjectSchema, Schema};
use regex::Regex;
use string_cache::DefaultAtom;

use super::{ConversionOutput, SchemaExt, type_resolver::TypeResolver};
use crate::generator::{
  ast::{
    Documentation, FieldDef, FieldNameToken, OuterAttr, RustPrimitive, SerdeAsFieldAttr, SerdeAttribute, TypeRef,
    ValidationAttribute,
  },
  converter::ConverterContext,
  naming::inference::InferenceExt,
  schema_registry::DiscriminatorMapping,
};

#[derive(Clone, Debug)]
pub(crate) struct FieldConverter {
  context: Rc<ConverterContext>,
  type_resolver: TypeResolver,
  odata_support: bool,
  customizations: HashMap<String, String>,
}

impl FieldConverter {
  pub(crate) fn new(context: &Rc<ConverterContext>) -> Self {
    let config = context.config();
    Self {
      context: context.clone(),
      type_resolver: TypeResolver::new(context.clone()),
      odata_support: config.odata_support(),
      customizations: config.customizations.clone(),
    }
  }

  /// Collects fields from an object schema, applying deduplication and inline type extraction.
  pub(crate) fn collect_fields(
    &self,
    parent_name: &str,
    schema: &ObjectSchema,
    schema_name: Option<&str>,
  ) -> anyhow::Result<ConversionOutput<Vec<FieldDef>>> {
    let required = schema.required.iter().collect::<BTreeSet<_>>();
    let discriminator_mapping = schema_name
      .and_then(|name| self.context.graph().mapping(name))
      .map(DiscriminatorMapping::as_tuple);

    let conversions = schema
      .properties
      .iter()
      .map(|(prop_name, prop_schema_ref)| {
        let prop_schema = prop_schema_ref
          .resolve(self.context.graph().spec())
          .context(format!("Schema resolution failed for property '{prop_name}'"))?;

        let resolved = self
          .type_resolver
          .resolve_property(parent_name, prop_name, &prop_schema, prop_schema_ref)?;

        let field = self.convert_field(
          prop_name,
          schema,
          &prop_schema,
          resolved.result,
          required.contains(prop_name),
          discriminator_mapping.as_ref(),
        );

        Ok((field, resolved.inline_types))
      })
      .collect::<anyhow::Result<Vec<_>>>()?;

    let (fields, inline_types): (Vec<_>, Vec<_>) = conversions.into_iter().unzip();
    let inline_types = inline_types.into_iter().flatten().collect();

    Ok(ConversionOutput::with_inline_types(
      Self::deduplicate_names(fields),
      inline_types,
    ))
  }

  pub(crate) fn build_additional_properties(
    &self,
    schema: &ObjectSchema,
  ) -> anyhow::Result<(Vec<SerdeAttribute>, Option<FieldDef>)> {
    let Some(ref additional) = schema.additional_properties else {
      return Ok((vec![], None));
    };

    match additional {
      Schema::Boolean(b) if !b.0 => Ok((vec![SerdeAttribute::DenyUnknownFields], None)),
      Schema::Object(_) => {
        let value_type = self.type_resolver.additional_properties_type(additional)?;
        let map_type = TypeRef::new(format!(
          "std::collections::HashMap<String, {}>",
          value_type.to_rust_type()
        ));
        let field = FieldDef::builder()
          .name(FieldNameToken::from_raw("additional_properties"))
          .docs(Documentation::from_lines([
            "Additional properties not defined in the schema.",
          ]))
          .rust_type(map_type)
          .serde_attrs(BTreeSet::from([SerdeAttribute::Flatten]))
          .build();
        Ok((vec![], Some(field)))
      }
      Schema::Boolean(_) => Ok((vec![], None)),
    }
  }

  pub(crate) fn struct_attributes(
    fields: &[FieldDef],
    base_serde: Vec<SerdeAttribute>,
  ) -> (Vec<SerdeAttribute>, Vec<OuterAttr>) {
    let default_serde = fields
      .iter()
      .any(|f| f.default_value.is_some())
      .then_some(SerdeAttribute::Default);

    let serde_attrs = base_serde.into_iter().chain(default_serde).collect();

    let outer_attrs = fields
      .iter()
      .any(|f| f.serde_as_attr.is_some())
      .then_some(OuterAttr::SerdeAs)
      .into_iter()
      .collect();

    (serde_attrs, outer_attrs)
  }

  pub(crate) fn convert_field(
    &self,
    prop_name: &str,
    parent_schema: &ObjectSchema,
    prop_schema: &ObjectSchema,
    resolved_type: TypeRef,
    is_required: bool,
    discriminator_mapping: Option<&(String, String)>,
  ) -> FieldDef {
    let discriminator_info = DiscriminatorFieldInfo::new(prop_name, parent_schema, prop_schema, discriminator_mapping);

    let should_be_optional = self.is_field_optional(
      prop_name,
      parent_schema,
      prop_schema,
      discriminator_info.as_ref(),
      is_required,
    );
    let final_type = if should_be_optional && !resolved_type.nullable {
      resolved_type.with_option()
    } else {
      resolved_type
    };

    let docs = Documentation::from_optional(prop_schema.description.as_ref());
    let validation_attrs = Self::extract_all_validation(prop_name, is_required, prop_schema, &final_type);
    let default_value = Self::extract_default_value(prop_schema);

    let rust_field_name = FieldNameToken::from_raw(prop_name);
    let serde_attrs = if rust_field_name == prop_name {
      BTreeSet::new()
    } else {
      BTreeSet::from([SerdeAttribute::Rename(prop_name.to_string())])
    };

    let serde_as_attr = self.get_customization_for_type(&final_type);

    let field = FieldDef::builder()
      .deprecated(prop_schema.deprecated.unwrap_or(false))
      .docs(docs)
      .maybe_default_value(default_value)
      .maybe_multiple_of(prop_schema.multiple_of.clone())
      .maybe_serde_as_attr(serde_as_attr)
      .name(FieldNameToken::from_raw(prop_name))
      .rust_type(final_type)
      .serde_attrs(serde_attrs)
      .validation_attrs(validation_attrs)
      .build();

    match discriminator_info.as_ref().filter(|d| d.should_hide()) {
      Some(info) => field.with_discriminator_behavior(info.value.as_deref(), info.is_base),
      None => field,
    }
  }

  fn get_customization_for_type(&self, type_ref: &TypeRef) -> Option<SerdeAsFieldAttr> {
    let key = Self::primitive_to_key(&type_ref.base_type)?;
    let custom_type = self.customizations.get(&key)?;
    Some(SerdeAsFieldAttr::CustomOverride {
      custom_type: custom_type.clone(),
      optional: type_ref.nullable,
      is_array: type_ref.is_array,
    })
  }

  fn primitive_to_key(primitive: &RustPrimitive) -> Option<String> {
    match primitive {
      RustPrimitive::DateTime => Some("date_time".to_string()),
      RustPrimitive::Date => Some("date".to_string()),
      RustPrimitive::Time => Some("time".to_string()),
      RustPrimitive::Duration => Some("duration".to_string()),
      RustPrimitive::Uuid => Some("uuid".to_string()),
      RustPrimitive::Custom(name) => Some(name.to_string()),
      _ => None,
    }
  }

  pub(crate) fn extract_parameter_metadata(
    prop_name: &str,
    is_required: bool,
    schema: &ObjectSchema,
    type_ref: &TypeRef,
  ) -> (Vec<ValidationAttribute>, Option<serde_json::Value>) {
    let validation_attrs = Self::extract_all_validation(prop_name, is_required, schema, type_ref);
    let default_value = Self::extract_default_value(schema);
    (validation_attrs, default_value)
  }

  pub(crate) fn extract_default_value(schema: &ObjectSchema) -> Option<serde_json::Value> {
    schema
      .default
      .clone()
      .or_else(|| schema.const_value.clone())
      .or_else(|| {
        if schema.enum_values.len() == 1 {
          schema.enum_values.first().cloned()
        } else {
          None
        }
      })
  }

  pub(crate) fn extract_all_validation(
    prop_name: &str,
    is_required: bool,
    schema: &ObjectSchema,
    type_ref: &TypeRef,
  ) -> Vec<ValidationAttribute> {
    let mut attrs = Vec::with_capacity(3);

    if let Some(ref format) = schema.format {
      match format.as_str() {
        "email" => attrs.push(ValidationAttribute::Email),
        "uri" | "url" => attrs.push(ValidationAttribute::Url),
        _ => {}
      }
    }

    if schema.is_numeric() {
      if let Some(range_attr) = Self::build_range_validation_attr(schema, type_ref) {
        attrs.push(range_attr);
      }
      return attrs;
    }

    if schema.is_freeform_string() {
      let has_non_string_format = schema.format.as_ref().is_some_and(|f| Self::is_non_string_format(f));

      if !has_non_string_format {
        if let Some(length_attr) = Self::build_length_attribute(schema.min_length, schema.max_length, is_required) {
          attrs.push(length_attr);
        }

        if let Some(pattern) = schema.pattern.as_ref() {
          if Regex::new(pattern).is_ok() {
            if let Some(regex) = Self::filter_regex_validation(Some(pattern.clone()), type_ref) {
              attrs.push(ValidationAttribute::Regex(regex));
            }
          } else {
            eprintln!("Warning: Invalid regex pattern '{pattern}' for property '{prop_name}'");
          }
        }
      }
      return attrs;
    }

    if schema.is_array()
      && let Some(length_attr) = Self::build_length_attribute(schema.min_items, schema.max_items, false)
    {
      attrs.push(length_attr);
    }

    attrs
  }

  fn filter_regex_validation(regex: Option<String>, type_ref: &TypeRef) -> Option<String> {
    match &type_ref.base_type {
      RustPrimitive::DateTime | RustPrimitive::Date | RustPrimitive::Time | RustPrimitive::Uuid => None,
      _ => regex,
    }
  }

  fn is_non_string_format(format: &str) -> bool {
    matches!(
      format,
      "date" | "date-time" | "duration" | "time" | "binary" | "byte" | "uuid"
    )
  }

  fn build_range_validation_attr(schema: &ObjectSchema, type_ref: &TypeRef) -> Option<ValidationAttribute> {
    let exclusive_min = schema.exclusive_minimum.clone();
    let exclusive_max = schema.exclusive_maximum.clone();
    let min = schema.minimum.clone();
    let max = schema.maximum.clone();

    if exclusive_min.is_none() && exclusive_max.is_none() && min.is_none() && max.is_none() {
      return None;
    }

    Some(ValidationAttribute::Range {
      primitive: type_ref.base_type.clone(),
      min,
      max,
      exclusive_min,
      exclusive_max,
    })
  }

  fn build_length_attribute(
    min: Option<u64>,
    max: Option<u64>,
    is_required_non_empty: bool,
  ) -> Option<ValidationAttribute> {
    match (min, max) {
      (Some(min), Some(max)) => Some(ValidationAttribute::Length {
        min: Some(min),
        max: Some(max),
      }),
      (Some(min), None) => Some(ValidationAttribute::Length {
        min: Some(min),
        max: None,
      }),
      (None, Some(max)) => Some(ValidationAttribute::Length {
        min: None,
        max: Some(max),
      }),
      (None, None) if is_required_non_empty => Some(ValidationAttribute::Length {
        min: Some(1),
        max: None,
      }),
      _ => None,
    }
  }

  fn is_field_optional(
    &self,
    prop_name: &str,
    parent_schema: &ObjectSchema,
    prop_schema: &ObjectSchema,
    discriminator_info: Option<&DiscriminatorFieldInfo>,
    is_required: bool,
  ) -> bool {
    let has_default = prop_schema.default.is_some();
    let is_discriminator_field = discriminator_info.is_some();
    let discriminator_has_enum = discriminator_info.is_some_and(|i| i.has_enum);

    if !is_required || has_default {
      return true;
    }
    if is_discriminator_field && !discriminator_has_enum {
      return true;
    }
    if self.odata_support
      && prop_name.starts_with("@odata.")
      && parent_schema.discriminator.is_none()
      && !parent_schema.has_intersection()
    {
      return true;
    }
    false
  }

  pub(crate) fn deduplicate_names(fields: Vec<FieldDef>) -> Vec<FieldDef> {
    let indices_by_name =
      fields
        .iter()
        .enumerate()
        .fold(BTreeMap::<String, Vec<usize>>::new(), |mut acc, (i, field)| {
          acc.entry(field.name.to_string()).or_default().push(i);
          acc
        });

    let collisions = indices_by_name
      .into_iter()
      .filter(|(_, v)| v.len() > 1)
      .collect::<BTreeMap<_, _>>();

    if collisions.is_empty() {
      return fields;
    }

    let indices_to_remove = collisions
      .iter()
      .filter_map(|(_, indices)| {
        let (deprecated, non_deprecated): (Vec<_>, Vec<_>) =
          indices.iter().copied().partition(|&i| fields[i].deprecated);
        (!deprecated.is_empty() && !non_deprecated.is_empty()).then_some(deprecated)
      })
      .flatten()
      .collect::<BTreeSet<_>>();

    let suffix_renames = collisions
      .iter()
      .flat_map(|(name, indices)| {
        let (deprecated, non_deprecated): (Vec<_>, Vec<_>) =
          indices.iter().copied().partition(|&i| fields[i].deprecated);
        if deprecated.is_empty() || non_deprecated.is_empty() {
          indices
            .iter()
            .enumerate()
            .skip(1)
            .map(|(suffix_num, &idx)| (idx, format!("{name}_{}", suffix_num + 1)))
            .collect::<Vec<_>>()
        } else {
          vec![]
        }
      })
      .collect::<BTreeMap<_, _>>();

    fields
      .into_iter()
      .enumerate()
      .filter(|(i, _)| !indices_to_remove.contains(i))
      .map(|(i, field)| match suffix_renames.get(&i) {
        Some(new_name) => FieldDef {
          name: FieldNameToken::new(new_name),
          ..field
        },
        None => field,
      })
      .collect()
  }
}

#[derive(Debug, Clone)]
pub(crate) struct DiscriminatorFieldInfo {
  pub value: Option<DefaultAtom>,
  pub is_base: bool,
  pub has_enum: bool,
}

impl DiscriminatorFieldInfo {
  pub fn new(
    prop_name: &str,
    parent_schema: &ObjectSchema,
    prop_schema: &ObjectSchema,
    discriminator_mapping: Option<&(String, String)>,
  ) -> Option<Self> {
    let value = discriminator_mapping
      .filter(|(prop, _)| prop == prop_name)
      .map(|(_, v)| DefaultAtom::from(v.as_str()));

    let is_base_discriminator = parent_schema
      .discriminator
      .as_ref()
      .is_some_and(|d| d.property_name == prop_name);

    let is_child_discriminator = value.is_some();

    if !is_child_discriminator && !is_base_discriminator {
      return None;
    }

    Some(Self {
      value,
      is_base: is_base_discriminator && !is_child_discriminator,
      has_enum: prop_schema.has_enum_values(),
    })
  }

  pub fn should_hide(&self) -> bool {
    !self.has_enum && (self.value.is_some() || self.is_base)
  }
}

use oas3::spec::ObjectSchema;
use regex::Regex;

use super::{CodegenConfig, SchemaExt, discriminator::DiscriminatorInfo};
use crate::generator::{
  ast::{Documentation, FieldDef, FieldDefBuilder, RustPrimitive, SerdeAttribute, TypeRef, ValidationAttribute},
  naming::identifiers::to_rust_field_name,
};

#[derive(Clone, Debug)]
pub(crate) struct FieldConverter {
  odata_support: bool,
}

impl FieldConverter {
  pub(crate) fn new(config: CodegenConfig) -> Self {
    Self {
      odata_support: config.odata_support(),
    }
  }

  pub(crate) fn convert_field(
    &self,
    prop_name: &str,
    parent_schema: &ObjectSchema,
    prop_schema: &ObjectSchema,
    resolved_type: TypeRef,
    is_required: bool,
    discriminator_mapping: Option<&(String, String)>,
  ) -> anyhow::Result<FieldDef> {
    let discriminator_info = DiscriminatorInfo::new(prop_name, parent_schema, prop_schema, discriminator_mapping);

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

    let mut docs = Self::extract_docs(prop_schema);
    let mut validation_attrs = Self::extract_all_validation(prop_name, is_required, prop_schema, &final_type);
    let mut default_value = Self::extract_default_value(prop_schema);
    let deprecated = prop_schema.deprecated.unwrap_or(false);
    let multiple_of = prop_schema.multiple_of.clone();

    let rust_field_name = to_rust_field_name(prop_name);
    let base_serde_attrs = if rust_field_name == prop_name {
      vec![]
    } else {
      vec![SerdeAttribute::Rename(prop_name.to_string())]
    };

    let (serde_attrs, doc_hidden) = Self::apply_discriminator_attributes(
      &mut docs,
      &mut validation_attrs,
      &mut default_value,
      base_serde_attrs,
      &final_type,
      discriminator_info.as_ref(),
    );

    let field = FieldDefBuilder::default()
      .name(to_rust_field_name(prop_name))
      .rust_type(final_type)
      .docs(docs)
      .serde_attrs(serde_attrs)
      .doc_hidden(doc_hidden)
      .validation_attrs(validation_attrs)
      .default_value(default_value)
      .deprecated(deprecated)
      .multiple_of(multiple_of)
      .build()?;

    Ok(field)
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

  pub(crate) fn extract_docs(schema: &ObjectSchema) -> Documentation {
    Documentation::from_optional(schema.description.as_ref())
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

    if schema.is_string() && schema.enum_values.is_empty() {
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
    discriminator_info: Option<&DiscriminatorInfo>,
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
      && parent_schema.all_of.is_empty()
    {
      return true;
    }
    false
  }

  pub(crate) fn apply_discriminator_attributes(
    docs: &mut Documentation,
    validation_attrs: &mut Vec<ValidationAttribute>,
    default_value: &mut Option<serde_json::Value>,
    mut serde_attrs: Vec<SerdeAttribute>,
    final_type: &TypeRef,
    discriminator_info: Option<&DiscriminatorInfo>,
  ) -> (Vec<SerdeAttribute>, bool) {
    let Some(disc_info) = discriminator_info.filter(|d| d.should_hide()) else {
      return (serde_attrs, false);
    };

    docs.clear();
    validation_attrs.clear();

    if disc_info.value.is_some() {
      *default_value = Some(serde_json::Value::String(disc_info.value.as_ref().unwrap().to_string()));
      serde_attrs.push(SerdeAttribute::SkipDeserializing);
      serde_attrs.push(SerdeAttribute::Default);
    } else {
      serde_attrs.clear();
      serde_attrs.push(SerdeAttribute::Skip);
      if final_type.is_string_like() {
        *default_value = Some(serde_json::Value::String(String::new()));
      }
    }

    (serde_attrs, true)
  }
}

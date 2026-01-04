use std::{
  collections::{BTreeSet, HashMap},
  rc::Rc,
};

use oas3::spec::ObjectSchema;
use regex::Regex;

use super::{SchemaExt, discriminator::DiscriminatorInfo};
use crate::generator::{
  ast::{
    Documentation, FieldDef, FieldNameToken, RustPrimitive, SerdeAsFieldAttr, SerdeAttribute, TypeRef,
    ValidationAttribute,
  },
  converter::ConverterContext,
};

#[derive(Clone, Debug)]
pub(crate) struct FieldConverter {
  odata_support: bool,
  customizations: HashMap<String, String>,
}

impl FieldConverter {
  pub(crate) fn new(context: &Rc<ConverterContext>) -> Self {
    let config = context.config();
    Self {
      odata_support: config.odata_support(),
      customizations: config.customizations.clone(),
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
  ) -> FieldDef {
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
}

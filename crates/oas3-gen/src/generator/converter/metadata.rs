use oas3::spec::ObjectSchema;
use regex::Regex;

use super::SchemaExt;
use crate::generator::ast::{Documentation, RustPrimitive, TypeRef, ValidationAttribute};

/// Metadata extracted from a schema for a struct field.
#[derive(Clone, Default)]
pub(crate) struct FieldMetadata {
  pub docs: Documentation,
  pub validation_attrs: Vec<ValidationAttribute>,
  pub default_value: Option<serde_json::Value>,
  pub deprecated: bool,
  pub multiple_of: Option<serde_json::Number>,
}

impl FieldMetadata {
  /// Extracts metadata from a schema and type reference.
  pub(crate) fn from_schema(prop_name: &str, is_required: bool, schema: &ObjectSchema, type_ref: &TypeRef) -> Self {
    MetadataExtractor::new(prop_name, is_required, schema, type_ref).extract()
  }
}

/// Extracts documentation from a schema description.
pub(crate) fn extract_docs(desc: Option<&String>) -> Documentation {
  Documentation::from_optional(desc)
}

pub(crate) struct MetadataExtractor<'a> {
  prop_name: &'a str,
  is_required: bool,
  schema: &'a ObjectSchema,
  type_ref: &'a TypeRef,
}

impl<'a> MetadataExtractor<'a> {
  pub(crate) fn new(prop_name: &'a str, is_required: bool, schema: &'a ObjectSchema, type_ref: &'a TypeRef) -> Self {
    Self {
      prop_name,
      is_required,
      schema,
      type_ref,
    }
  }

  pub(crate) fn extract(&self) -> FieldMetadata {
    FieldMetadata {
      docs: self.extract_docs(),
      validation_attrs: self.extract_all_validation(),
      default_value: self.extract_default_value(),
      deprecated: self.schema.deprecated.unwrap_or(false),
      multiple_of: self.schema.multiple_of.clone(),
    }
  }

  pub(crate) fn extract_docs(&self) -> Documentation {
    extract_docs(self.schema.description.as_ref())
  }

  pub(crate) fn extract_default_value(&self) -> Option<serde_json::Value> {
    self
      .schema
      .default
      .clone()
      .or_else(|| self.schema.const_value.clone())
      .or_else(|| {
        if self.schema.enum_values.len() == 1 {
          self.schema.enum_values.first().cloned()
        } else {
          None
        }
      })
  }

  pub(crate) fn extract_all_validation(&self) -> Vec<ValidationAttribute> {
    let mut attrs = self.extract_validation_attrs();
    if let Some(regex) = self.extract_regex_validation() {
      attrs.push(regex);
    }
    attrs
  }

  /// Extracts validation attributes (Email, Url, Length, Range).
  pub(crate) fn extract_validation_attrs(&self) -> Vec<ValidationAttribute> {
    let mut attrs = vec![];

    if let Some(ref format) = self.schema.format {
      match format.as_str() {
        "email" => attrs.push(ValidationAttribute::Email),
        "uri" | "url" => attrs.push(ValidationAttribute::Url),
        _ => {}
      }
    }

    if self.schema.is_numeric()
      && let Some(range_attr) = self.build_range_validation_attr()
    {
      attrs.push(range_attr);
    }

    if self.schema.is_string()
      && self.schema.enum_values.is_empty()
      && let Some(length_attr) = self.build_string_length_validation_attr()
    {
      attrs.push(length_attr);
    }

    if self.schema.is_array()
      && let Some(length_attr) = self.build_array_length_validation_attr()
    {
      attrs.push(length_attr);
    }

    attrs
  }

  pub(crate) fn extract_regex_validation(&self) -> Option<ValidationAttribute> {
    let pattern = self.extract_validation_pattern()?;
    self
      .filter_regex_validation(Some(pattern.clone()))
      .map(ValidationAttribute::Regex)
  }

  pub(crate) fn extract_validation_pattern(&self) -> Option<&String> {
    if !self.schema.is_string() {
      return None;
    }

    let pattern = self.schema.pattern.as_ref()?;

    if let Some(format) = self.schema.format.as_ref()
      && Self::is_non_string_format(format)
    {
      return None;
    }

    if !self.schema.enum_values.is_empty() {
      return None;
    }

    if Regex::new(pattern).is_err() {
      eprintln!(
        "Warning: Invalid regex pattern '{pattern}' for property '{}'",
        self.prop_name
      );
      return None;
    }

    Some(pattern)
  }

  /// Filters regex validation based on the Rust type.
  ///
  /// Certain Rust types (DateTime, Date, Time, Uuid) have their own validation
  /// and should not have additional regex validation applied.
  pub(crate) fn filter_regex_validation(&self, regex: Option<String>) -> Option<String> {
    match &self.type_ref.base_type {
      RustPrimitive::DateTime | RustPrimitive::Date | RustPrimitive::Time | RustPrimitive::Uuid => None,
      _ => regex,
    }
  }

  pub(crate) fn is_non_string_format(format: &str) -> bool {
    matches!(
      format,
      "date" | "date-time" | "duration" | "time" | "binary" | "byte" | "uuid"
    )
  }

  pub(crate) fn build_range_validation_attr(&self) -> Option<ValidationAttribute> {
    let exclusive_min = self.schema.exclusive_minimum.clone();
    let exclusive_max = self.schema.exclusive_maximum.clone();
    let min = self.schema.minimum.clone();
    let max = self.schema.maximum.clone();

    if exclusive_min.is_none() && exclusive_max.is_none() && min.is_none() && max.is_none() {
      return None;
    }

    Some(ValidationAttribute::Range {
      primitive: self.type_ref.base_type.clone(),
      min,
      max,
      exclusive_min,
      exclusive_max,
    })
  }

  pub(crate) fn build_string_length_validation_attr(&self) -> Option<ValidationAttribute> {
    if let Some(format) = self.schema.format.as_ref()
      && Self::is_non_string_format(format)
    {
      return None;
    }

    Self::build_length_attribute(self.schema.min_length, self.schema.max_length, self.is_required)
  }

  pub(crate) fn build_array_length_validation_attr(&self) -> Option<ValidationAttribute> {
    Self::build_length_attribute(self.schema.min_items, self.schema.max_items, false)
  }

  pub(crate) fn build_length_attribute(
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
}

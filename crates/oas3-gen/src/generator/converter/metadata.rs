use std::sync::LazyLock;

use num_format::{CustomFormat, Grouping, ToFormattedString};
use oas3::spec::{ObjectSchema, SchemaType, SchemaTypeSet};
use regex::Regex;

use crate::generator::{
  ast::{RustPrimitive, TypeRef},
  utils::doc_comment_lines,
};

static UNDERSCORE_FORMAT: LazyLock<CustomFormat> = LazyLock::new(|| {
  CustomFormat::builder()
    .grouping(Grouping::Standard)
    .separator("_")
    .build()
    .expect("formatter failed to build.")
});

/// Metadata extracted from a schema for a struct field.
#[derive(Clone, Default)]
pub(crate) struct FieldMetadata {
  pub docs: Vec<String>,
  pub validation_attrs: Vec<String>,
  pub regex_validation: Option<String>,
  pub default_value: Option<serde_json::Value>,
  pub deprecated: bool,
  pub multiple_of: Option<serde_json::Number>,
}

impl FieldMetadata {
  /// Extracts metadata from a schema and type reference.
  pub(crate) fn from_schema(prop_name: &str, is_required: bool, schema: &ObjectSchema, type_ref: &TypeRef) -> Self {
    Self {
      docs: extract_docs(schema.description.as_ref()),
      validation_attrs: extract_validation_attrs(is_required, schema, type_ref),
      regex_validation: extract_validation_pattern(prop_name, schema).cloned(),
      default_value: extract_default_value(schema),
      deprecated: schema.deprecated.unwrap_or(false),
      multiple_of: schema.multiple_of.clone(),
    }
  }
}

/// Extracts documentation comments from a schema description.
pub(crate) fn extract_docs(desc: Option<&String>) -> Vec<String> {
  desc.map_or_else(Vec::new, |d| doc_comment_lines(d))
}

/// Extracts the default value from a schema, checking `default` and `const` fields.
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

/// Extracts a validation regex pattern, filtering out known non-string formats.
pub(crate) fn extract_validation_pattern<'s>(prop_name: &str, schema: &'s ObjectSchema) -> Option<&'s String> {
  match (schema.schema_type.as_ref(), schema.pattern.as_ref()) {
    (Some(SchemaTypeSet::Single(SchemaType::String)), Some(pattern)) => {
      let is_non_string_format = schema.format.as_ref().is_some_and(|f| {
        matches!(
          f.as_str(),
          "date" | "date-time" | "duration" | "time" | "binary" | "byte" | "uuid"
        )
      });

      if is_non_string_format {
        return None;
      }

      if !schema.enum_values.is_empty() {
        return None;
      }

      if Regex::new(pattern).is_ok() {
        Some(pattern)
      } else {
        eprintln!("Warning: Invalid regex pattern '{pattern}' for property '{prop_name}'");
        None
      }
    }
    _ => None,
  }
}

/// Filters regex validation based on the Rust type (e.g. skip for Dates).
pub(crate) fn filter_regex_validation(rust_type: &TypeRef, regex: Option<String>) -> Option<String> {
  match &rust_type.base_type {
    RustPrimitive::DateTime | RustPrimitive::Date | RustPrimitive::Time | RustPrimitive::Uuid => None,
    _ => regex,
  }
}

/// Extracts validation attributes (e.g. `length`, `range`, `email`) for validator crate.
pub(crate) fn extract_validation_attrs(is_required: bool, schema: &ObjectSchema, type_ref: &TypeRef) -> Vec<String> {
  let mut attrs = Vec::new();

  if let Some(ref format) = schema.format {
    match format.as_str() {
      "email" => attrs.push("email".to_string()),
      "uri" | "url" => attrs.push("url".to_string()),
      _ => {}
    }
  }

  if let Some(ref schema_type) = schema.schema_type {
    if matches!(
      schema_type,
      SchemaTypeSet::Single(SchemaType::Number | SchemaType::Integer)
    ) && let Some(range_attr) = build_range_validation_attr(schema, type_ref)
    {
      attrs.push(range_attr);
    }

    if matches!(schema_type, SchemaTypeSet::Single(SchemaType::String))
      && schema.enum_values.is_empty()
      && let Some(length_attr) = build_string_length_validation_attr(is_required, schema)
    {
      attrs.push(length_attr);
    }

    if matches!(schema_type, SchemaTypeSet::Single(SchemaType::Array))
      && let Some(length_attr) = build_array_length_validation_attr(schema)
    {
      attrs.push(length_attr);
    }
  }

  attrs
}

fn build_range_validation_attr(schema: &ObjectSchema, type_ref: &TypeRef) -> Option<String> {
  let primitive = &type_ref.base_type;

  let mut parts = Vec::<String>::new();
  if let Some(v) = schema.exclusive_minimum.as_ref() {
    parts.push(format!("exclusive_min = {}", primitive.format_number(v)));
  }
  if let Some(v) = schema.exclusive_maximum.as_ref() {
    parts.push(format!("exclusive_max = {}", primitive.format_number(v)));
  }
  if let Some(v) = schema.minimum.as_ref() {
    parts.push(format!("min = {}", primitive.format_number(v)));
  }
  if let Some(v) = schema.maximum.as_ref() {
    parts.push(format!("max = {}", primitive.format_number(v)));
  }

  if parts.is_empty() {
    None
  } else {
    Some(format!("range({})", parts.join(", ")))
  }
}

fn build_string_length_validation_attr(is_required: bool, schema: &ObjectSchema) -> Option<String> {
  let is_non_string_format = schema.format.as_ref().is_some_and(|f| {
    matches!(
      f.as_str(),
      "date" | "date-time" | "duration" | "time" | "binary" | "byte" | "uuid"
    )
  });

  if is_non_string_format {
    return None;
  }

  let min = schema.min_length.map(|l| l.to_formatted_string(&*UNDERSCORE_FORMAT));
  let max = schema.max_length.map(|l| l.to_formatted_string(&*UNDERSCORE_FORMAT));

  build_length_attribute(min, max, is_required)
}

fn build_array_length_validation_attr(schema: &ObjectSchema) -> Option<String> {
  let min = schema.min_items.map(|l| l.to_formatted_string(&*UNDERSCORE_FORMAT));
  let max = schema.max_items.map(|l| l.to_formatted_string(&*UNDERSCORE_FORMAT));
  build_length_attribute(min, max, false)
}

fn build_length_attribute(min: Option<String>, max: Option<String>, is_required_non_empty: bool) -> Option<String> {
  match (min, max) {
    (Some(min), Some(max)) => Some(format!("length(min = {min}, max = {max})")),
    (Some(min), None) => Some(format!("length(min = {min})")),
    (None, Some(max)) => Some(format!("length(max = {max})")),
    (None, None) if is_required_non_empty => Some("length(min = 1)".to_string()),
    _ => None,
  }
}

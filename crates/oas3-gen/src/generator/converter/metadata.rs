use oas3::spec::{ObjectSchema, SchemaType, SchemaTypeSet};
use regex::Regex;

use crate::{
  generator::ast::{RustPrimitive, TypeRef, ValidationAttribute},
  utils::text::doc_comment_lines,
};

/// Metadata extracted from a schema for a struct field.
#[derive(Clone, Default)]
pub(crate) struct FieldMetadata {
  pub docs: Vec<String>,
  pub validation_attrs: Vec<ValidationAttribute>,
  pub default_value: Option<serde_json::Value>,
  pub deprecated: bool,
  pub multiple_of: Option<serde_json::Number>,
}

impl FieldMetadata {
  /// Extracts metadata from a schema and type reference.
  pub(crate) fn from_schema(prop_name: &str, is_required: bool, schema: &ObjectSchema, type_ref: &TypeRef) -> Self {
    let mut validation_attrs = extract_validation_attrs(is_required, schema, type_ref);
    if let Some(regex_attr) = ValidationAttribute::extract_regex_if_applicable(prop_name, schema, type_ref) {
      validation_attrs.push(regex_attr);
    }

    Self {
      docs: extract_docs(schema.description.as_ref()),
      validation_attrs,
      default_value: extract_default_value(schema),
      deprecated: schema.deprecated.unwrap_or(false),
      multiple_of: schema.multiple_of.clone(),
    }
  }
}

/// Checks if the format represents a non-string type that should not have string validations.
pub(crate) fn is_non_string_format(format: &str) -> bool {
  matches!(
    format,
    "date" | "date-time" | "duration" | "time" | "binary" | "byte" | "uuid"
  )
}

/// Checks if the schema has a specific single type.
pub(crate) fn is_single_schema_type(schema: &ObjectSchema, schema_type: SchemaType) -> bool {
  matches!(
    schema.schema_type.as_ref(),
    Some(SchemaTypeSet::Single(t)) if *t == schema_type
  )
}

/// Extracts documentation comments from a schema description.
pub(crate) fn extract_docs(desc: Option<&String>) -> Vec<String> {
  desc.map_or_else(Vec::new, |d| doc_comment_lines(d))
}

/// Extracts the default value from a schema.
///
/// Checks in order: `default`, `const`, and single-value enums.
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

/// Extracts a validation regex pattern from a string schema.
///
/// Returns `None` if:
/// - Schema is not a string type
/// - Format represents a non-string type (date, uuid, etc.)
/// - Schema has enum values (validated by enum type itself)
/// - Pattern is invalid regex syntax
pub(crate) fn extract_validation_pattern<'s>(prop_name: &str, schema: &'s ObjectSchema) -> Option<&'s String> {
  if !is_single_schema_type(schema, SchemaType::String) {
    return None;
  }

  let pattern = schema.pattern.as_ref()?;

  if let Some(format) = schema.format.as_ref()
    && is_non_string_format(format)
  {
    return None;
  }

  if !schema.enum_values.is_empty() {
    return None;
  }

  if Regex::new(pattern).is_err() {
    eprintln!("Warning: Invalid regex pattern '{pattern}' for property '{prop_name}'");
    return None;
  }

  Some(pattern)
}

/// Filters regex validation based on the Rust type.
///
/// Certain Rust types (DateTime, Date, Time, Uuid) have their own validation
/// and should not have additional regex validation applied.
pub(crate) fn filter_regex_validation(rust_type: &TypeRef, regex: Option<String>) -> Option<String> {
  match &rust_type.base_type {
    RustPrimitive::DateTime | RustPrimitive::Date | RustPrimitive::Time | RustPrimitive::Uuid => None,
    _ => regex,
  }
}

/// Extracts validation attributes for the `validator` crate.
///
/// Generates attributes based on schema constraints:
/// - Email/URL validation from format field
/// - Range validation for numbers
/// - Length validation for strings and arrays
pub(crate) fn extract_validation_attrs(
  is_required: bool,
  schema: &ObjectSchema,
  type_ref: &TypeRef,
) -> Vec<ValidationAttribute> {
  let mut attrs = vec![];

  if let Some(ref format) = schema.format {
    match format.as_str() {
      "email" => attrs.push(ValidationAttribute::Email),
      "uri" | "url" => attrs.push(ValidationAttribute::Url),
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

    if is_single_schema_type(schema, SchemaType::String)
      && schema.enum_values.is_empty()
      && let Some(length_attr) = build_string_length_validation_attr(is_required, schema)
    {
      attrs.push(length_attr);
    }

    if is_single_schema_type(schema, SchemaType::Array)
      && let Some(length_attr) = build_array_length_validation_attr(schema)
    {
      attrs.push(length_attr);
    }
  }

  attrs
}

pub(crate) fn build_range_validation_attr(schema: &ObjectSchema, type_ref: &TypeRef) -> Option<ValidationAttribute> {
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

pub(crate) fn build_string_length_validation_attr(
  is_required: bool,
  schema: &ObjectSchema,
) -> Option<ValidationAttribute> {
  if let Some(format) = schema.format.as_ref()
    && is_non_string_format(format)
  {
    return None;
  }

  build_length_attribute(schema.min_length, schema.max_length, is_required)
}

pub(crate) fn build_array_length_validation_attr(schema: &ObjectSchema) -> Option<ValidationAttribute> {
  build_length_attribute(schema.min_items, schema.max_items, false)
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

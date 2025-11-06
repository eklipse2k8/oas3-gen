use std::sync::LazyLock;

use num_format::{CustomFormat, Grouping, ToFormattedString};
use oas3::spec::{ObjectSchema, SchemaType, SchemaTypeSet};
use regex::Regex;
use serde_json::Number;

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

#[derive(Clone, Default)]
pub(crate) struct FieldMetadata {
  pub docs: Vec<String>,
  pub validation_attrs: Vec<String>,
  pub regex_validation: Option<String>,
  pub default_value: Option<serde_json::Value>,
  pub read_only: bool,
  pub write_only: bool,
  pub deprecated: bool,
  pub multiple_of: Option<serde_json::Number>,
}

impl FieldMetadata {
  pub(crate) fn from_schema(prop_name: &str, is_required: bool, schema: &ObjectSchema) -> Self {
    Self {
      docs: extract_docs(schema.description.as_ref()),
      validation_attrs: extract_validation_attrs(is_required, schema),
      regex_validation: extract_validation_pattern(prop_name, schema).cloned(),
      default_value: extract_default_value(schema),
      read_only: schema.read_only.unwrap_or(false),
      write_only: schema.write_only.unwrap_or(false),
      deprecated: schema.deprecated.unwrap_or(false),
      multiple_of: schema.multiple_of.clone(),
    }
  }
}

pub(crate) fn extract_docs(desc: Option<&String>) -> Vec<String> {
  desc.map_or_else(Vec::new, |d| doc_comment_lines(d))
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

pub(crate) fn filter_regex_validation(rust_type: &TypeRef, regex: Option<String>) -> Option<String> {
  match &rust_type.base_type {
    RustPrimitive::DateTime | RustPrimitive::Date | RustPrimitive::Time | RustPrimitive::Uuid => None,
    _ => regex,
  }
}

pub(crate) fn extract_validation_attrs(is_required: bool, schema: &ObjectSchema) -> Vec<String> {
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
    ) && let Some(range_attr) = build_range_validation_attr(schema, schema_type)
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

fn build_range_validation_attr(schema: &ObjectSchema, schema_type: &SchemaTypeSet) -> Option<String> {
  let primitive = if matches!(schema_type, SchemaTypeSet::Single(SchemaType::Number)) {
    super::type_resolver::format_to_primitive(schema.format.as_ref()).unwrap_or(RustPrimitive::F64)
  } else {
    super::type_resolver::format_to_primitive(schema.format.as_ref()).unwrap_or(RustPrimitive::I64)
  };

  let mut parts = Vec::<String>::new();
  if let Some(v) = schema.exclusive_minimum.as_ref() {
    parts.push(format!("exclusive_min = {}", render_number(&primitive, v)));
  }
  if let Some(v) = schema.exclusive_maximum.as_ref() {
    parts.push(format!("exclusive_max = {}", render_number(&primitive, v)));
  }
  if let Some(v) = schema.minimum.as_ref() {
    parts.push(format!("min = {}", render_number(&primitive, v)));
  }
  if let Some(v) = schema.maximum.as_ref() {
    parts.push(format!("max = {}", render_number(&primitive, v)));
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

fn render_number(primitive: &RustPrimitive, num: &Number) -> String {
  if primitive.is_float() {
    let s = num.to_string();
    if s.contains('.') { s } else { format!("{s}.0") }
  } else if let Some(value) = num.as_i64() {
    render_integer(primitive, value)
  } else if let Some(value) = num.as_u64() {
    render_unsigned_integer(primitive, value)
  } else {
    num.to_string()
  }
}

fn render_integer(primitive: &RustPrimitive, value: i64) -> String {
  match primitive {
    RustPrimitive::I8 if value <= i64::from(i8::MIN) => "i8::MIN".to_string(),
    RustPrimitive::I8 if value >= i64::from(i8::MAX) => "i8::MAX".to_string(),
    RustPrimitive::I8 => format!("{}i8", value.to_formatted_string(&*UNDERSCORE_FORMAT)),
    RustPrimitive::I16 if value <= i64::from(i16::MIN) => "i16::MIN".to_string(),
    RustPrimitive::I16 if value >= i64::from(i16::MAX) => "i16::MAX".to_string(),
    RustPrimitive::I16 => format!("{}i16", value.to_formatted_string(&*UNDERSCORE_FORMAT)),
    RustPrimitive::I32 if value <= i64::from(i32::MIN) => "i32::MIN".to_string(),
    RustPrimitive::I32 if value >= i64::from(i32::MAX) => "i32::MAX".to_string(),
    RustPrimitive::I32 => format!("{}i32", value.to_formatted_string(&*UNDERSCORE_FORMAT)),
    RustPrimitive::I64 => format!("{}i64", value.to_formatted_string(&*UNDERSCORE_FORMAT)),
    _ => value.to_string(),
  }
}

fn render_unsigned_integer(primitive: &RustPrimitive, value: u64) -> String {
  match primitive {
    RustPrimitive::U8 if value >= u64::from(u8::MAX) => "u8::MAX".to_string(),
    RustPrimitive::U8 => format!("{}u8", value.to_formatted_string(&*UNDERSCORE_FORMAT)),
    RustPrimitive::U16 if value >= u64::from(u16::MAX) => "u16::MAX".to_string(),
    RustPrimitive::U16 => format!("{}u16", value.to_formatted_string(&*UNDERSCORE_FORMAT)),
    RustPrimitive::U32 if value >= u64::from(u32::MAX) => "u32::MAX".to_string(),
    RustPrimitive::U32 => format!("{}u32", value.to_formatted_string(&*UNDERSCORE_FORMAT)),
    RustPrimitive::U64 => format!("{}u64", value.to_formatted_string(&*UNDERSCORE_FORMAT)),
    _ => value.to_string(),
  }
}

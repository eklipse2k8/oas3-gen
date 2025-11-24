use std::fmt;

use oas3::spec::ObjectSchema;
use serde_json::Number;

use crate::generator::{
  ast::{RustPrimitive, TypeRef, types::render_unsigned_integer},
  converter::metadata,
};

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct RegexKey {
  owner_type: String,
  owner_variant: Option<String>,
  field: String,
}

impl RegexKey {
  pub fn for_struct(type_name: &str, field_name: &str) -> Self {
    Self {
      owner_type: type_name.to_string(),
      owner_variant: None,
      field: field_name.to_string(),
    }
  }

  pub fn parts(&self) -> Vec<&str> {
    let mut parts = vec![self.owner_type.as_str()];
    if let Some(variant) = &self.owner_variant {
      parts.push(variant.as_str());
    }
    parts.push(self.field.as_str());
    parts
  }
}

/// Represents a validation attribute from the `validator` crate.
///
/// These attributes are used to validate struct fields.
#[derive(Debug, Clone)]
pub enum ValidationAttribute {
  Email,
  Url,
  Length {
    min: Option<u64>,
    max: Option<u64>,
  },
  Range {
    primitive: RustPrimitive,
    min: Option<Number>,
    max: Option<Number>,
    exclusive_min: Option<Number>,
    exclusive_max: Option<Number>,
  },
  Regex(String),
}

impl ValidationAttribute {
  pub(crate) fn extract_regex_if_applicable(
    field_name: &str,
    schema: &ObjectSchema,
    type_ref: &TypeRef,
  ) -> Option<Self> {
    let pattern = metadata::extract_validation_pattern(field_name, schema)?;
    metadata::filter_regex_validation(type_ref, Some(pattern.clone())).map(ValidationAttribute::Regex)
  }
}

impl PartialEq for ValidationAttribute {
  fn eq(&self, other: &Self) -> bool {
    match (self, other) {
      (ValidationAttribute::Email, ValidationAttribute::Email)
      | (ValidationAttribute::Url, ValidationAttribute::Url) => true,
      (ValidationAttribute::Length { min: min1, max: max1 }, ValidationAttribute::Length { min: min2, max: max2 }) => {
        min1 == min2 && max1 == max2
      }
      (
        ValidationAttribute::Range {
          primitive: p1,
          min: min1,
          max: max1,
          exclusive_min: emin1,
          exclusive_max: emax1,
        },
        ValidationAttribute::Range {
          primitive: p2,
          min: min2,
          max: max2,
          exclusive_min: emin2,
          exclusive_max: emax2,
        },
      ) => {
        p1 == p2
          && compare_numbers(min1.as_ref(), min2.as_ref())
          && compare_numbers(max1.as_ref(), max2.as_ref())
          && compare_numbers(emin1.as_ref(), emin2.as_ref())
          && compare_numbers(emax1.as_ref(), emax2.as_ref())
      }
      (ValidationAttribute::Regex(s1), ValidationAttribute::Regex(s2)) => s1 == s2,
      _ => false,
    }
  }
}

impl Eq for ValidationAttribute {}

fn compare_numbers(n1: Option<&Number>, n2: Option<&Number>) -> bool {
  match (n1, n2) {
    (None, None) => true,
    (Some(a), Some(b)) => {
      if let (Some(a_i64), Some(b_i64)) = (a.as_i64(), b.as_i64()) {
        a_i64 == b_i64
      } else if let (Some(a_u64), Some(b_u64)) = (a.as_u64(), b.as_u64()) {
        a_u64 == b_u64
      } else if let (Some(a_f64), Some(b_f64)) = (a.as_f64(), b.as_f64()) {
        (a_f64 - b_f64).abs() < f64::EPSILON
      } else {
        false
      }
    }
    _ => false,
  }
}

impl fmt::Display for ValidationAttribute {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    match self {
      ValidationAttribute::Email => write!(f, "email"),
      ValidationAttribute::Url => write!(f, "url"),
      ValidationAttribute::Regex(path) => write!(f, "regex(path = \"{path}\")"),
      ValidationAttribute::Length { min, max } => {
        let mut parts = vec![];
        if let Some(m) = min {
          parts.push(format!("min = {}", render_unsigned_integer(&RustPrimitive::U64, *m)));
        }
        if let Some(m) = max {
          parts.push(format!("max = {}", render_unsigned_integer(&RustPrimitive::U64, *m)));
        }
        write!(f, "length({})", parts.join(", "))
      }
      ValidationAttribute::Range {
        primitive,
        min,
        max,
        exclusive_min,
        exclusive_max,
      } => {
        let mut parts = vec![];
        if let Some(m) = min {
          parts.push(format!("min = {}", primitive.format_number(m)));
        }
        if let Some(m) = max {
          parts.push(format!("max = {}", primitive.format_number(m)));
        }
        if let Some(m) = exclusive_min {
          parts.push(format!("exclusive_min = {}", primitive.format_number(m)));
        }
        if let Some(m) = exclusive_max {
          parts.push(format!("exclusive_max = {}", primitive.format_number(m)));
        }
        write!(f, "range({})", parts.join(", "))
      }
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn test_validation_attribute_length_display() {
    let attr = ValidationAttribute::Length {
      min: Some(1),
      max: Some(1_000),
    };
    assert_eq!(attr.to_string(), "length(min = 1u64, max = 1_000u64)");

    let attr_min_only = ValidationAttribute::Length {
      min: Some(10_000),
      max: None,
    };
    assert_eq!(attr_min_only.to_string(), "length(min = 10_000u64)");

    let attr_max_only = ValidationAttribute::Length {
      min: None,
      max: Some(1_000_000),
    };
    assert_eq!(attr_max_only.to_string(), "length(max = 1_000_000u64)");
  }

  #[test]
  fn test_validation_attribute_range_display() {
    let attr = ValidationAttribute::Range {
      primitive: RustPrimitive::I32,
      min: Some(serde_json::json!(1).as_number().unwrap().clone()),
      max: Some(serde_json::json!(1000).as_number().unwrap().clone()),
      exclusive_min: None,
      exclusive_max: None,
    };
    assert_eq!(attr.to_string(), "range(min = 1i32, max = 1_000i32)");

    let attr_exclusive = ValidationAttribute::Range {
      primitive: RustPrimitive::I64,
      min: None,
      max: None,
      exclusive_min: Some(serde_json::json!(0).as_number().unwrap().clone()),
      exclusive_max: Some(serde_json::json!(100).as_number().unwrap().clone()),
    };
    assert_eq!(
      attr_exclusive.to_string(),
      "range(exclusive_min = 0i64, exclusive_max = 100i64)"
    );

    let attr_float = ValidationAttribute::Range {
      primitive: RustPrimitive::F64,
      min: Some(serde_json::json!(0.5).as_number().unwrap().clone()),
      max: Some(serde_json::json!(1.0).as_number().unwrap().clone()),
      exclusive_min: None,
      exclusive_max: None,
    };
    assert_eq!(attr_float.to_string(), "range(min = 0.5, max = 1.0)");
  }
}

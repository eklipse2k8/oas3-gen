use proc_macro2::TokenStream;
use quote::{ToTokens, quote};
use serde_json::Number;

use crate::generator::ast::{RustPrimitive, StructToken, types::render_unsigned_integer};

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct RegexKey {
  owner_type: StructToken,
  owner_variant: Option<String>,
  field: String,
}

impl RegexKey {
  pub fn for_struct(type_name: &StructToken, field_name: &str) -> Self {
    Self {
      owner_type: type_name.clone(),
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
  Nested,
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

impl PartialEq for ValidationAttribute {
  fn eq(&self, other: &Self) -> bool {
    match (self, other) {
      (Self::Email, Self::Email) | (Self::Url, Self::Url) | (Self::Nested, Self::Nested) => true,
      (Self::Length { min: min1, max: max1 }, Self::Length { min: min2, max: max2 }) => min1 == min2 && max1 == max2,
      (
        Self::Range {
          primitive: p1,
          min: min1,
          max: max1,
          exclusive_min: emin1,
          exclusive_max: emax1,
        },
        Self::Range {
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
      (Self::Regex(s1), Self::Regex(s2)) => s1 == s2,
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

impl ToTokens for ValidationAttribute {
  fn to_tokens(&self, tokens: &mut TokenStream) {
    let attr = match self {
      Self::Email => quote! { email },
      Self::Url => quote! { url },
      Self::Nested => quote! { nested },
      Self::Regex(path) => quote! { regex(path = #path) },
      Self::Length { min, max } => {
        let min_part = min.map(|m| {
          let lit: TokenStream = render_unsigned_integer(&RustPrimitive::U64, m).parse().unwrap();
          quote! { min = #lit }
        });
        let max_part = max.map(|m| {
          let lit: TokenStream = render_unsigned_integer(&RustPrimitive::U64, m).parse().unwrap();
          quote! { max = #lit }
        });
        match (min_part, max_part) {
          (Some(min), Some(max)) => quote! { length(#min, #max) },
          (Some(min), None) => quote! { length(#min) },
          (None, Some(max)) => quote! { length(#max) },
          (None, None) => quote! { length() },
        }
      }
      Self::Range {
        primitive,
        min,
        max,
        exclusive_min,
        exclusive_max,
      } => {
        let mut parts = vec![];
        if let Some(m) = min {
          let lit: TokenStream = primitive.format_number(m).parse().unwrap();
          parts.push(quote! { min = #lit });
        }
        if let Some(m) = max {
          let lit: TokenStream = primitive.format_number(m).parse().unwrap();
          parts.push(quote! { max = #lit });
        }
        if let Some(m) = exclusive_min {
          let lit: TokenStream = primitive.format_number(m).parse().unwrap();
          parts.push(quote! { exclusive_min = #lit });
        }
        if let Some(m) = exclusive_max {
          let lit: TokenStream = primitive.format_number(m).parse().unwrap();
          parts.push(quote! { exclusive_max = #lit });
        }
        quote! { range(#(#parts),*) }
      }
    };
    tokens.extend(attr);
  }
}

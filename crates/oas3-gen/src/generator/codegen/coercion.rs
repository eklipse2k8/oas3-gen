use proc_macro2::TokenStream;
use quote::quote;

use crate::generator::ast::{RustPrimitive, TypeRef};

pub(crate) fn json_to_rust_literal(value: &serde_json::Value, rust_type: &TypeRef) -> TokenStream {
  if matches!(value, serde_json::Value::Null) {
    return quote! { None };
  }

  let base_expr = coerce_to_rust_type(value, &rust_type.base_type);

  if rust_type.nullable {
    quote! { Some(#base_expr) }
  } else {
    base_expr
  }
}

pub(crate) fn parse_type_string(type_str: &str) -> TokenStream {
  type_str.parse().unwrap_or_else(|_| quote! { serde_json::Value })
}

fn coerce_to_rust_type(value: &serde_json::Value, rust_type: &RustPrimitive) -> TokenStream {
  match rust_type {
    RustPrimitive::String => coerce_to_string(value),
    RustPrimitive::I8
    | RustPrimitive::I16
    | RustPrimitive::I32
    | RustPrimitive::I64
    | RustPrimitive::I128
    | RustPrimitive::Isize => coerce_to_int(value, rust_type),
    RustPrimitive::U8
    | RustPrimitive::U16
    | RustPrimitive::U32
    | RustPrimitive::U64
    | RustPrimitive::U128
    | RustPrimitive::Usize => coerce_to_uint(value, rust_type),
    RustPrimitive::F32 | RustPrimitive::F64 => coerce_to_float(value, rust_type),
    RustPrimitive::Bool => coerce_to_bool(value),
    _ => quote! { Default::default() },
  }
}

fn coerce_to_string(value: &serde_json::Value) -> TokenStream {
  match value {
    serde_json::Value::String(s) if s.is_empty() => quote! { String::new() },
    serde_json::Value::String(s) => quote! { #s.to_string() },
    serde_json::Value::Number(n) => {
      let n_str = n.to_string();
      quote! { #n_str.to_string() }
    }
    serde_json::Value::Bool(b) => {
      let b_str = b.to_string();
      quote! { #b_str.to_string() }
    }
    _ => quote! { Default::default() },
  }
}

fn coerce_to_bool(value: &serde_json::Value) -> TokenStream {
  match value {
    serde_json::Value::Bool(b) => quote! { #b },
    serde_json::Value::Number(n) => {
      let b = n.as_i64().is_some_and(|i| i != 0);
      quote! { #b }
    }
    serde_json::Value::String(s) => {
      let b = matches!(s.to_lowercase().as_str(), "true" | "1" | "yes");
      quote! { #b }
    }
    _ => quote! { Default::default() },
  }
}

#[allow(clippy::match_same_arms)]
fn coerce_to_int(value: &serde_json::Value, rust_type: &RustPrimitive) -> TokenStream {
  let type_suffix = match rust_type {
    RustPrimitive::I8 => "i8",
    RustPrimitive::I16 => "i16",
    RustPrimitive::I32 => "i32",
    RustPrimitive::I64 => "i64",
    RustPrimitive::I128 => "i128",
    RustPrimitive::Isize => "isize",
    _ => "i64",
  };

  match value {
    serde_json::Value::Number(n) => n.as_i64().map_or_else(
      || quote! { Default::default() },
      |i| {
        let literal = format!("{i}{type_suffix}");
        literal
          .parse::<TokenStream>()
          .unwrap_or_else(|_| quote! { Default::default() })
      },
    ),
    serde_json::Value::String(s) => s.parse::<i64>().ok().map_or_else(
      || quote! { Default::default() },
      |i| {
        let literal = format!("{i}{type_suffix}");
        literal
          .parse::<TokenStream>()
          .unwrap_or_else(|_| quote! { Default::default() })
      },
    ),
    _ => quote! { Default::default() },
  }
}

#[allow(clippy::match_same_arms)]
fn coerce_to_uint(value: &serde_json::Value, rust_type: &RustPrimitive) -> TokenStream {
  let type_suffix = match rust_type {
    RustPrimitive::U8 => "u8",
    RustPrimitive::U16 => "u16",
    RustPrimitive::U32 => "u32",
    RustPrimitive::U64 => "u64",
    RustPrimitive::U128 => "u128",
    RustPrimitive::Usize => "usize",
    _ => "u64",
  };

  match value {
    serde_json::Value::Number(n) => n.as_u64().map_or_else(
      || quote! { Default::default() },
      |u| {
        let literal = format!("{u}{type_suffix}");
        literal
          .parse::<TokenStream>()
          .unwrap_or_else(|_| quote! { Default::default() })
      },
    ),
    serde_json::Value::String(s) => s.parse::<u64>().ok().map_or_else(
      || quote! { Default::default() },
      |u| {
        let literal = format!("{u}{type_suffix}");
        literal
          .parse::<TokenStream>()
          .unwrap_or_else(|_| quote! { Default::default() })
      },
    ),
    _ => quote! { Default::default() },
  }
}

fn coerce_to_float(value: &serde_json::Value, rust_type: &RustPrimitive) -> TokenStream {
  #[allow(clippy::match_same_arms)]
  let type_suffix = match rust_type {
    RustPrimitive::F32 => "f32",
    RustPrimitive::F64 => "f64",
    _ => "f64",
  };

  match value {
    serde_json::Value::Number(n) => {
      if let Some(f) = n.as_f64() {
        let literal = format!("{f}{type_suffix}");
        literal
          .parse::<TokenStream>()
          .unwrap_or_else(|_| quote! { Default::default() })
      } else if let Some(i) = n.as_i64() {
        #[allow(clippy::cast_precision_loss)]
        let f = i as f64;
        let literal = format!("{f}{type_suffix}");
        literal
          .parse::<TokenStream>()
          .unwrap_or_else(|_| quote! { Default::default() })
      } else {
        quote! { Default::default() }
      }
    }
    serde_json::Value::String(s) => s.parse::<f64>().ok().map_or_else(
      || quote! { Default::default() },
      |f| {
        let literal = format!("{f}{type_suffix}");
        literal
          .parse::<TokenStream>()
          .unwrap_or_else(|_| quote! { Default::default() })
      },
    ),
    _ => quote! { Default::default() },
  }
}

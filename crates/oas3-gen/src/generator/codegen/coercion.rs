use proc_macro2::TokenStream;
use quote::quote;
use serde_json::Value;

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

fn typed_literal(value: impl std::fmt::Display, type_suffix: &str) -> TokenStream {
  format!("{value}{type_suffix}")
    .parse()
    .unwrap_or_else(|_| quote! { Default::default() })
}

fn coerce_to_rust_type(value: &serde_json::Value, rust_type: &RustPrimitive) -> TokenStream {
  match rust_type {
    RustPrimitive::String => coerce_to_string(value),
    RustPrimitive::StaticStr => coerce_to_static_str(value),
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

fn coerce_to_string(value: &Value) -> TokenStream {
  match value {
    Value::String(s) if s.is_empty() => quote! { String::new() },
    Value::String(s) => quote! { #s.to_string() },
    Value::Number(n) => {
      let n_str = n.to_string();
      quote! { #n_str.to_string() }
    }
    Value::Bool(b) => {
      let b_str = b.to_string();
      quote! { #b_str.to_string() }
    }
    _ => quote! { Default::default() },
  }
}

fn coerce_to_static_str(value: &Value) -> TokenStream {
  match value {
    Value::String(s) => quote! { #s },
    Value::Number(n) => {
      let n_str = n.to_string();
      quote! { #n_str }
    }
    Value::Bool(b) => {
      let b_str = b.to_string();
      quote! { #b_str }
    }
    _ => quote! { "" },
  }
}

fn coerce_to_bool(value: &Value) -> TokenStream {
  match value {
    Value::Bool(b) => quote! { #b },
    Value::Number(n) => {
      let b = n.as_i64().is_some_and(|i| i != 0);
      quote! { #b }
    }
    Value::String(s) => {
      let b = matches!(s.to_lowercase().as_str(), "true" | "1" | "yes");
      quote! { #b }
    }
    _ => quote! { Default::default() },
  }
}

fn coerce_to_int(value: &Value, rust_type: &RustPrimitive) -> TokenStream {
  let type_suffix = rust_type.to_string();
  let to_literal = |i| typed_literal(i, &type_suffix);

  match value {
    Value::Number(n) => n.as_i64().map_or_else(|| quote! { Default::default() }, to_literal),
    Value::String(s) => s
      .parse::<i64>()
      .ok()
      .map_or_else(|| quote! { Default::default() }, to_literal),
    _ => quote! { Default::default() },
  }
}

fn coerce_to_uint(value: &Value, rust_type: &RustPrimitive) -> TokenStream {
  let type_suffix = rust_type.to_string();
  let to_literal = |u| typed_literal(u, &type_suffix);

  match value {
    Value::Number(n) => n.as_u64().map_or_else(|| quote! { Default::default() }, to_literal),
    Value::String(s) => s
      .parse::<u64>()
      .ok()
      .map_or_else(|| quote! { Default::default() }, to_literal),
    _ => quote! { Default::default() },
  }
}

fn coerce_to_float(value: &Value, rust_type: &RustPrimitive) -> TokenStream {
  let type_suffix = rust_type.to_string();
  let to_literal = |f: f64| typed_literal(f, &type_suffix);

  match value {
    Value::Number(n) => {
      if let Some(f) = n.as_f64() {
        to_literal(f)
      } else if let Some(i) = n.as_i64() {
        #[allow(clippy::cast_precision_loss)]
        to_literal(i as f64)
      } else {
        quote! { Default::default() }
      }
    }
    Value::String(s) => s
      .parse::<f64>()
      .ok()
      .map_or_else(|| quote! { Default::default() }, to_literal),
    _ => quote! { Default::default() },
  }
}

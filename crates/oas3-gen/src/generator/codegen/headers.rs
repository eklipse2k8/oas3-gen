use proc_macro2::TokenStream;
use quote::quote;

use crate::generator::ast::{FieldDef, StructDef, StructKind, TypeRef, tokens::ConstToken};

pub(crate) struct HeaderMapGenerator {
  def: StructDef,
}

impl HeaderMapGenerator {
  pub(crate) fn new(def: &StructDef) -> Self {
    Self { def: def.clone() }
  }

  pub(crate) fn should_generate(&self) -> bool {
    matches!(self.def.kind, StructKind::HeaderParams) && !self.def.fields.is_empty()
  }

  pub(crate) fn emit(&self) -> TokenStream {
    if !self.should_generate() {
      return quote! {};
    }

    let struct_name = &self.def.name;
    let field_count = self.def.fields.len();
    let insertions = self.generate_field_insertions();

    quote! {
      impl core::convert::TryFrom<&#struct_name> for http::HeaderMap {
        type Error = http::header::InvalidHeaderValue;

        fn try_from(headers: &#struct_name) -> core::result::Result<Self, Self::Error> {
          let mut map = http::HeaderMap::with_capacity(#field_count);
          #insertions
          Ok(map)
        }
      }

      impl core::convert::TryFrom<#struct_name> for http::HeaderMap {
        type Error = http::header::InvalidHeaderValue;

        fn try_from(headers: #struct_name) -> core::result::Result<Self, Self::Error> {
          http::HeaderMap::try_from(&headers)
        }
      }
    }
  }

  fn generate_field_insertions(&self) -> TokenStream {
    let insertions: Vec<TokenStream> = self.def.fields.iter().map(generate_field_insertion).collect();

    quote! { #(#insertions)* }
  }
}

fn generate_field_insertion(field: &FieldDef) -> TokenStream {
  let field_name = &field.name;
  let Some(original_name) = &field.original_name else {
    return quote! {};
  };

  let header_const = ConstToken::from_raw(original_name);
  let ty = &field.rust_type;

  if field.is_required() {
    let header_value = header_value_expr(ty, quote! { &headers.#field_name });
    quote! {
      let header_value = http::HeaderValue::try_from(#header_value)?;
      map.insert(#header_const, header_value);
    }
  } else {
    let header_value = header_value_expr(ty, quote! { value });
    quote! {
      if let Some(value) = &headers.#field_name {
        let header_value = http::HeaderValue::try_from(#header_value)?;
        map.insert(#header_const, header_value);
      }
    }
  }
}

fn header_value_expr(ty: &TypeRef, accessor: TokenStream) -> TokenStream {
  if ty.is_string_like() {
    accessor
  } else if ty.is_array {
    quote! { #accessor.iter().map(|v| v.to_string()).collect::<Vec<_>>().join(",") }
  } else {
    quote! { #accessor.to_string() }
  }
}

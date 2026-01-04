use proc_macro2::TokenStream;
use quote::quote;

use crate::generator::ast::{FieldDef, FieldNameToken, StructDef, StructKind, TypeRef, tokens::ConstToken};

pub(crate) struct HeaderMapGenerator<'a> {
  def: &'a StructDef,
}

impl<'a> HeaderMapGenerator<'a> {
  pub(crate) fn new(def: &'a StructDef) -> Self {
    Self { def }
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
    let insertions: Vec<TokenStream> = self.def.fields.iter().map(Self::generate_field_insertion).collect();

    quote! { #(#insertions)* }
  }

  fn generate_field_insertion(field: &FieldDef) -> TokenStream {
    let field_name = &field.name;
    let Some(original_name) = &field.original_name else {
      return quote! {};
    };

    let header_const = ConstToken::from_raw(original_name);
    let is_required = field.is_required();

    let conversion = Self::value_conversion(&field.rust_type, field_name, is_required);

    if is_required {
      quote! {
        #conversion
        map.insert(#header_const, header_value);
      }
    } else {
      quote! {
        if let Some(value) = &headers.#field_name {
          #conversion
          map.insert(#header_const, header_value);
        }
      }
    }
  }

  fn value_conversion(ty: &TypeRef, field_name: &FieldNameToken, required: bool) -> TokenStream {
    if ty.is_string_like() {
      if required {
        quote! {
          let header_value = http::HeaderValue::try_from(&headers.#field_name)?;
        }
      } else {
        quote! {
          let header_value = http::HeaderValue::try_from(value)?;
        }
      }
    } else if ty.is_primitive_type() {
      if required {
        quote! {
          let header_value = http::HeaderValue::try_from(headers.#field_name.to_string())?;
        }
      } else {
        quote! {
          let header_value = http::HeaderValue::try_from(value.to_string())?;
        }
      }
    } else if required {
      quote! {
        let header_value = http::HeaderValue::try_from(
          serde_plain::to_string(&headers.#field_name).map_err(|_| http::header::InvalidHeaderValue::new())?
        )?;
      }
    } else {
      quote! {
        let header_value = http::HeaderValue::try_from(
          serde_plain::to_string(value).map_err(|_| http::header::InvalidHeaderValue::new())?
        )?;
      }
    }
  }
}

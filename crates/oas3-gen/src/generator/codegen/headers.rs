use proc_macro2::TokenStream;
use quote::{ToTokens, quote};

use crate::generator::ast::{FieldDef, StructDef, StructKind, TypeRef, tokens::ConstToken};

#[derive(Clone, Debug)]
pub(crate) struct HeaderMapFragment {
  def: StructDef,
}

impl HeaderMapFragment {
  pub(crate) fn new(def: StructDef) -> Self {
    Self { def }
  }

  fn should_generate(&self) -> bool {
    matches!(self.def.kind, StructKind::HeaderParams) && !self.def.fields.is_empty()
  }
}

impl ToTokens for HeaderMapFragment {
  fn to_tokens(&self, tokens: &mut TokenStream) {
    if !self.should_generate() {
      return;
    }

    let struct_name = &self.def.name;
    let field_count = self.def.fields.len();
    let insertions: Vec<HeaderFieldInsertionFragment> = self
      .def
      .fields
      .iter()
      .cloned()
      .map(HeaderFieldInsertionFragment::new)
      .collect();

    tokens.extend(quote! {
      impl core::convert::TryFrom<&#struct_name> for http::HeaderMap {
        type Error = http::header::InvalidHeaderValue;

        fn try_from(headers: &#struct_name) -> core::result::Result<Self, Self::Error> {
          let mut map = http::HeaderMap::with_capacity(#field_count);
          #(#insertions)*
          Ok(map)
        }
      }

      impl core::convert::TryFrom<#struct_name> for http::HeaderMap {
        type Error = http::header::InvalidHeaderValue;

        fn try_from(headers: #struct_name) -> core::result::Result<Self, Self::Error> {
          http::HeaderMap::try_from(&headers)
        }
      }
    });
  }
}

#[derive(Clone, Debug)]
pub(crate) struct HeaderFieldInsertionFragment {
  field: FieldDef,
}

impl HeaderFieldInsertionFragment {
  pub(crate) fn new(field: FieldDef) -> Self {
    Self { field }
  }
}

impl ToTokens for HeaderFieldInsertionFragment {
  fn to_tokens(&self, tokens: &mut TokenStream) {
    let field_name = &self.field.name;
    let Some(original_name) = &self.field.original_name else {
      return;
    };

    let header_const = ConstToken::from_raw(original_name);
    let ty = &self.field.rust_type;

    let insertion = if self.field.is_required() {
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
    };

    tokens.extend(insertion);
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

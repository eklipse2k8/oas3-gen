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

#[derive(Clone, Debug)]
pub(crate) struct HeaderFromMapFragment {
  def: StructDef,
}

impl HeaderFromMapFragment {
  pub(crate) fn new(def: StructDef) -> Self {
    Self { def }
  }

  fn should_generate(&self) -> bool {
    matches!(self.def.kind, StructKind::HeaderParams) && !self.def.fields.is_empty()
  }
}

impl ToTokens for HeaderFromMapFragment {
  fn to_tokens(&self, tokens: &mut TokenStream) {
    if !self.should_generate() {
      return;
    }

    let struct_name = &self.def.name;
    let extractions: Vec<HeaderFieldExtractionFragment> = self
      .def
      .fields
      .iter()
      .cloned()
      .map(HeaderFieldExtractionFragment::new)
      .collect::<Vec<_>>();

    tokens.extend(quote! {
      impl core::convert::TryFrom<&http::HeaderMap> for #struct_name {
        type Error = http::header::InvalidHeaderValue;

        fn try_from(headers: &http::HeaderMap) -> core::result::Result<Self, Self::Error> {
          Ok(Self {
            #(#extractions),*
          })
        }
      }

      impl core::convert::TryFrom<http::HeaderMap> for #struct_name {
        type Error = http::header::InvalidHeaderValue;

        fn try_from(headers: http::HeaderMap) -> core::result::Result<Self, Self::Error> {
          Self::try_from(&headers)
        }
      }
    });
  }
}

#[derive(Clone, Debug)]
struct HeaderFieldExtractionFragment {
  field: FieldDef,
}

impl HeaderFieldExtractionFragment {
  fn new(field: FieldDef) -> Self {
    Self { field }
  }
}

impl ToTokens for HeaderFieldExtractionFragment {
  fn to_tokens(&self, tokens: &mut TokenStream) {
    let field_name = &self.field.name;
    let Some(original_name) = &self.field.original_name else {
      tokens.extend(quote! { #field_name: Default::default() });
      return;
    };

    let header_const = ConstToken::from_raw(original_name);
    let parse_expr = header_parse_expr(&self.field.rust_type, &quote! { value });
    let default_suffix = self.field.is_required().then(|| quote! { .unwrap_or_default() });

    tokens.extend(quote! {
      #field_name: headers
        .get(#header_const)
        .and_then(|v| v.to_str().ok())
        .map(|value| #parse_expr)
        #default_suffix
    });
  }
}

fn header_parse_expr(ty: &TypeRef, accessor: &TokenStream) -> TokenStream {
  if ty.is_string_like() {
    quote! { #accessor.to_string() }
  } else if ty.is_array {
    quote! { #accessor.split(',').map(|s| s.trim()).filter_map(|s| s.parse().ok()).collect() }
  } else {
    quote! { #accessor.parse().unwrap_or_default() }
  }
}

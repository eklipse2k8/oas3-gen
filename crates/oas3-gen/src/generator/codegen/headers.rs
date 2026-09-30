use proc_macro2::TokenStream;
use quote::{ToTokens, quote};

use crate::generator::{
  ast::{FieldDef, StructDef, StructKind, TypeRef, tokens::ConstToken},
  converter::GenerationTarget,
};

#[derive(Clone, Copy, Debug)]
pub(crate) struct HeaderMapFragment<'a> {
  def: &'a StructDef,
}

impl<'a> HeaderMapFragment<'a> {
  pub(crate) fn new(def: &'a StructDef) -> Self {
    Self { def }
  }

  fn should_generate(self) -> bool {
    self.def.kind.is_header_struct() && !self.def.fields.is_empty()
  }
}

impl ToTokens for HeaderMapFragment<'_> {
  fn to_tokens(&self, tokens: &mut TokenStream) {
    if !self.should_generate() {
      return;
    }

    let struct_name = &self.def.name;
    let field_count = self.def.fields.len();
    let insertions: Vec<HeaderFieldInsertionFragment<'_>> =
      self.def.fields.iter().map(HeaderFieldInsertionFragment::new).collect();

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

#[derive(Clone, Copy, Debug)]
pub(crate) struct HeaderFieldInsertionFragment<'a> {
  field: &'a FieldDef,
}

impl<'a> HeaderFieldInsertionFragment<'a> {
  pub(crate) fn new(field: &'a FieldDef) -> Self {
    Self { field }
  }
}

impl ToTokens for HeaderFieldInsertionFragment<'_> {
  fn to_tokens(&self, tokens: &mut TokenStream) {
    let field_name = &self.field.name;
    let Some(original_name) = &self.field.original_name else {
      return;
    };

    let header_const = ConstToken::from_raw(original_name);
    let ty = &self.field.rust_type;

    let insertion = if self.field.rust_type.nullable {
      let header_value = header_value_expr(ty, quote! { value });
      quote! {
        if let Some(value) = &headers.#field_name {
          let header_value = http::HeaderValue::try_from(#header_value)?;
          map.insert(#header_const, header_value);
        }
      }
    } else {
      let header_value = header_value_expr(ty, quote! { &headers.#field_name });
      quote! {
        let header_value = http::HeaderValue::try_from(#header_value)?;
        map.insert(#header_const, header_value);
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

#[derive(Clone, Copy, Debug)]
pub(crate) struct HeaderFromMapFragment<'a> {
  def: &'a StructDef,
  target: GenerationTarget,
}

impl<'a> HeaderFromMapFragment<'a> {
  pub(crate) fn new(def: &'a StructDef, target: GenerationTarget) -> Self {
    Self { def, target }
  }

  fn should_generate(self) -> bool {
    let parsed = match self.def.kind {
      StructKind::HeaderParams => self.target == GenerationTarget::Server,
      StructKind::ResponseHeaders => true,
      _ => false,
    };
    parsed && !self.def.fields.is_empty()
  }
}

impl ToTokens for HeaderFromMapFragment<'_> {
  fn to_tokens(&self, tokens: &mut TokenStream) {
    if !self.should_generate() {
      return;
    }

    let struct_name = &self.def.name;
    let extractions: Vec<HeaderFieldExtractionFragment<'_>> = self
      .def
      .fields
      .iter()
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

#[derive(Clone, Copy, Debug)]
struct HeaderFieldExtractionFragment<'a> {
  field: &'a FieldDef,
}

impl<'a> HeaderFieldExtractionFragment<'a> {
  fn new(field: &'a FieldDef) -> Self {
    Self { field }
  }
}

impl ToTokens for HeaderFieldExtractionFragment<'_> {
  fn to_tokens(&self, tokens: &mut TokenStream) {
    let field_name = &self.field.name;
    let Some(original_name) = &self.field.original_name else {
      tokens.extend(quote! { #field_name: Default::default() });
      return;
    };

    let header_const = ConstToken::from_raw(original_name);
    let parse_expr = header_parse_expr(&self.field.rust_type, &quote! { value });
    let default_suffix = (!self.field.rust_type.nullable).then(|| quote! { .unwrap_or_default() });

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

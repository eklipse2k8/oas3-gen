use std::collections::BTreeSet;

use proc_macro2::TokenStream;
use quote::{ToTokens, quote};

use crate::generator::ast::{
  DeriveTrait, Documentation, FieldDef, OuterAttr, SerdeAsFieldAttr, SerdeAttribute, ValidationAttribute,
};

pub(crate) fn generate_docs_for_field(field: &FieldDef) -> Documentation {
  let mut docs = field.docs.clone();

  if let Some(ref example) = field.example_value {
    let mut formatted_example = field.rust_type.format_example(example);
    if field.rust_type.is_string_like() && !formatted_example.ends_with(".to_string()") {
      formatted_example = format!("{formatted_example}.to_string()");
    }
    let display_example = if field.rust_type.nullable {
      format!("Some({formatted_example})")
    } else {
      formatted_example
    };
    docs.push(format!("- Example: `{display_example}`"));
  }

  if let Some(ref multiple_of) = field.multiple_of {
    docs.push(format!("Validation: Must be a multiple of {multiple_of}"));
  }

  docs
}

pub(crate) fn generate_derives_from_slice(derives: &BTreeSet<DeriveTrait>) -> TokenStream {
  if derives.is_empty() {
    return quote! {};
  }
  let derive_idents = derives.iter().filter_map(|d| d.to_string().parse::<TokenStream>().ok());
  quote! { #[derive(#(#derive_idents),*)] }
}

pub(crate) fn generate_outer_attrs(attrs: &[OuterAttr]) -> TokenStream {
  if attrs.is_empty() {
    return quote! {};
  }
  let attr_tokens: Vec<TokenStream> = attrs.iter().map(quote::ToTokens::to_token_stream).collect();
  quote! { #(#attr_tokens)* }
}

/// Generates a single combined `#[serde(...)]` attribute for the given serde attributes.
///
/// If attrs is empty, returns nothing. Otherwise combines all attributes into a single
/// `#[serde(attr1, attr2, ...)]` attribute to reduce output noise.
pub(crate) fn generate_serde_attrs(attrs: &[SerdeAttribute]) -> TokenStream {
  if attrs.is_empty() {
    return quote! {};
  }
  let attr_tokens: Vec<_> = attrs.iter().map(quote::ToTokens::to_token_stream).collect();
  quote! { #[serde(#(#attr_tokens),*)] }
}

/// Generates a single combined `#[validate(...)]` attribute for the given validation attributes.
///
/// If attrs is empty, returns nothing. Otherwise combines all attributes into a single
/// `#[validate(attr1, attr2, ...)]` attribute.
pub(crate) fn generate_validation_attrs(attrs: &[ValidationAttribute]) -> TokenStream {
  if attrs.is_empty() {
    return quote! {};
  }

  let attr_tokens: Vec<_> = attrs.iter().map(quote::ToTokens::to_token_stream).collect();

  quote! { #[validate(#(#attr_tokens),*)] }
}

pub(crate) fn generate_deprecated_attr(deprecated: bool) -> TokenStream {
  if deprecated {
    quote! { #[deprecated] }
  } else {
    quote! {}
  }
}

pub(crate) fn generate_serde_as_attr(attr: Option<&SerdeAsFieldAttr>) -> TokenStream {
  match attr {
    Some(a) => a.to_token_stream(),
    None => quote! {},
  }
}

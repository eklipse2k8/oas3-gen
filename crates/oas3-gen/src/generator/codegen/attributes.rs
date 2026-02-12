use std::collections::BTreeSet;

use proc_macro2::TokenStream;
use quote::{ToTokens, TokenStreamExt as _, quote};

use super::coercion;
use crate::generator::ast::{
  DeriveTrait, Documentation, FieldDef, OuterAttr, RustPrimitive, SerdeAsFieldAttr, SerdeAttribute,
  ValidationAttribute, bon_attrs::BuilderAttribute,
};

pub(crate) fn generate_docs_for_field(field: &FieldDef) -> Documentation {
  let mut docs = field.docs.clone();

  if let Some(ref example) = field.example_value {
    let mut formatted_example = field.rust_type.format_example(example);
    if field.rust_type.base_type == RustPrimitive::String && !formatted_example.ends_with(".to_string()") {
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

#[derive(Clone, Debug)]
pub struct DeriveAttribute<T>(BTreeSet<T>);

impl<T: ToTokens> DeriveAttribute<T> {
  pub fn new(derives: BTreeSet<T>) -> Self {
    Self(derives)
  }
}

impl<T: ToTokens> ToTokens for DeriveAttribute<T> {
  fn to_tokens(&self, tokens: &mut TokenStream) {
    if self.0.is_empty() {
      return;
    }

    let derives = &self.0;
    tokens.append_all(quote! { #[derive(#(#derives),*)] });
  }
}

pub(crate) fn generate_derives_from_slice<'a>(derives: impl IntoIterator<Item = &'a DeriveTrait>) -> TokenStream {
  let derive_idents = derives
    .into_iter()
    .map(quote::ToTokens::to_token_stream)
    .collect::<Vec<_>>();

  if derive_idents.is_empty() {
    quote! {}
  } else {
    quote! { #[derive(#(#derive_idents),*)] }
  }
}

pub(crate) fn generate_outer_attrs<'a>(attrs: impl IntoIterator<Item = &'a OuterAttr>) -> TokenStream {
  let attr_tokens = attrs
    .into_iter()
    .map(quote::ToTokens::to_token_stream)
    .collect::<Vec<_>>();

  if attr_tokens.is_empty() {
    quote! {}
  } else {
    quote! { #(#attr_tokens)* }
  }
}

/// Generates a single combined `#[serde(...)]` attribute for the given serde attributes.
///
/// If attrs is empty, returns nothing. Otherwise combines all attributes into a single
/// `#[serde(attr1, attr2, ...)]` attribute to reduce output noise.
pub(crate) fn generate_serde_attrs<'a>(attrs: impl IntoIterator<Item = &'a SerdeAttribute>) -> TokenStream {
  let attr_tokens = attrs
    .into_iter()
    .map(quote::ToTokens::to_token_stream)
    .collect::<Vec<_>>();

  if attr_tokens.is_empty() {
    quote! {}
  } else {
    quote! { #[serde(#(#attr_tokens),*)] }
  }
}

/// Generates a single combined `#[validate(...)]` attribute for the given validation attributes.
///
/// If attrs is empty, returns nothing. Otherwise combines all attributes into a single
/// `#[validate(attr1, attr2, ...)]` attribute.
pub(crate) fn generate_validation_attrs<'a>(attrs: impl IntoIterator<Item = &'a ValidationAttribute>) -> TokenStream {
  let attr_tokens = attrs
    .into_iter()
    .map(quote::ToTokens::to_token_stream)
    .collect::<Vec<_>>();

  if attr_tokens.is_empty() {
    quote! {}
  } else {
    quote! { #[validate(#(#attr_tokens),*)] }
  }
}

/// Generates a single combined `#[builder(...)]` attribute for the given builder attributes.
///
/// If attrs is empty, returns nothing. Otherwise combines all attributes into a single
/// `#[builder(attr1, attr2, ...)]` attribute to reduce output noise.
///
/// `Default` and `Skip` variants carry a `serde_json::Value` + `TypeRef` which are
/// coerced to Rust expressions via [`coercion::json_to_rust_literal`].
pub(crate) fn generate_builder_attrs<'a>(attrs: impl IntoIterator<Item = &'a BuilderAttribute>) -> TokenStream {
  let attr_tokens = attrs
    .into_iter()
    .map(|attr| match attr {
      BuilderAttribute::Default { value, type_ref } => {
        let expr = coercion::json_to_rust_literal(value, type_ref);
        quote! { default = #expr }
      }
      BuilderAttribute::Rename(name) => {
        let ident = syn::Ident::new(name, proc_macro2::Span::call_site());
        quote! { name = #ident }
      }
      BuilderAttribute::Skip { value, type_ref } => {
        let expr = coercion::json_to_rust_literal(value, type_ref);
        quote! { skip = #expr }
      }
    })
    .collect::<Vec<_>>();

  if attr_tokens.is_empty() {
    quote! {}
  } else {
    quote! { #[builder(#(#attr_tokens),*)] }
  }
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

pub(crate) fn generate_doc_hidden_attr(hidden: bool) -> TokenStream {
  if hidden {
    quote! { #[doc(hidden)] }
  } else {
    quote! {}
  }
}

pub(crate) fn generate_field_default_attr(field: &FieldDef) -> TokenStream {
  field.default_value.as_ref().map_or_else(
    || quote! {},
    |default_value| {
      let default_expr = coercion::json_to_rust_literal(default_value, &field.rust_type);
      quote! { #[default(#default_expr)] }
    },
  )
}

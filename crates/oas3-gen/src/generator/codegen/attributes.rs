use proc_macro2::TokenStream;
use quote::quote;

use crate::generator::ast::FieldDef;

pub(crate) fn generate_docs(docs: &[String]) -> TokenStream {
  if docs.is_empty() {
    return quote! {};
  }
  let doc_lines: Vec<TokenStream> = docs
    .iter()
    .map(|line| {
      let clean = line.strip_prefix("/// ").unwrap_or(line);
      quote! { #[doc = #clean] }
    })
    .collect();
  quote! { #(#doc_lines)* }
}

pub(crate) fn generate_docs_for_field(field: &FieldDef) -> TokenStream {
  let mut docs = field.docs.clone();
  if let Some(ref multiple_of) = field.multiple_of {
    docs.push(format!("/// Validation: Must be a multiple of {multiple_of}"));
  }
  generate_docs(&docs)
}

pub(crate) fn generate_derives_from_slice(derives: &[String]) -> TokenStream {
  if derives.is_empty() {
    return quote! {};
  }
  let derive_idents = derives.iter().filter_map(|d| d.parse::<TokenStream>().ok());
  quote! { #[derive(#(#derive_idents),*)] }
}

pub(crate) fn generate_outer_attrs(attrs: &[String]) -> TokenStream {
  if attrs.is_empty() {
    return quote! {};
  }
  let attr_tokens: Vec<TokenStream> = attrs
    .iter()
    .filter_map(|attr| {
      let trimmed = attr.trim();
      if trimmed.is_empty() {
        return None;
      }
      let source = if trimmed.starts_with("#[") {
        trimmed.to_string()
      } else {
        format!("#[{trimmed}]")
      };
      source.parse::<TokenStream>().ok()
    })
    .collect();
  quote! { #(#attr_tokens)* }
}

pub(crate) fn generate_serde_attrs(attrs: &[String]) -> TokenStream {
  if attrs.is_empty() {
    return quote! {};
  }
  let attr_tokens: Vec<TokenStream> = attrs
    .iter()
    .filter_map(|attr| {
      let tokens: TokenStream = attr.as_str().parse().ok()?;
      Some(quote! { #[serde(#tokens)] })
    })
    .collect();
  quote! { #(#attr_tokens)* }
}

pub(crate) fn generate_validation_attrs(regex_const: Option<&str>, attrs: &[String]) -> TokenStream {
  if attrs.is_empty() && regex_const.is_none() {
    return quote! {};
  }

  let mut combined = attrs.to_owned();

  if let Some(const_name) = regex_const {
    combined.push(format!("regex(path = \"{const_name}\")"));
  }

  let attr_tokens: Vec<TokenStream> = combined.iter().filter_map(|attr| attr.parse().ok()).collect();

  quote! { #[validate(#(#attr_tokens),*)] }
}

pub(crate) fn generate_deprecated_attr(deprecated: bool) -> TokenStream {
  if deprecated {
    quote! { #[deprecated] }
  } else {
    quote! {}
  }
}

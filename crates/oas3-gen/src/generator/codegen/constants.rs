use std::collections::BTreeMap;

use proc_macro2::TokenStream;
use quote::{format_ident, quote};

use crate::{
  generator::ast::RustType,
  reserved::{header_const_name, regex_const_name},
};

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct RegexKey {
  owner_type: String,
  owner_variant: Option<String>,
  field: String,
}

impl RegexKey {
  pub fn for_struct(type_name: &str, field_name: &str) -> Self {
    Self {
      owner_type: type_name.to_string(),
      owner_variant: None,
      field: field_name.to_string(),
    }
  }

  #[allow(dead_code)]
  pub fn for_variant(type_name: &str, variant_name: &str, field_name: &str) -> Self {
    Self {
      owner_type: type_name.to_string(),
      owner_variant: Some(variant_name.to_string()),
      field: field_name.to_string(),
    }
  }

  pub fn parts(&self) -> Vec<&str> {
    let mut parts = vec![self.owner_type.as_str()];
    if let Some(variant) = &self.owner_variant {
      parts.push(variant.as_str());
    }
    parts.push(self.field.as_str());
    parts
  }
}

pub(crate) fn generate_regex_constants(types: &[&RustType]) -> (TokenStream, BTreeMap<RegexKey, String>) {
  let mut const_defs: BTreeMap<String, String> = BTreeMap::new();
  let mut lookup: BTreeMap<RegexKey, String> = BTreeMap::new();
  let mut pattern_to_const: BTreeMap<String, String> = BTreeMap::new();

  for rust_type in types {
    match rust_type {
      RustType::Struct(def) => {
        for field in &def.fields {
          let Some(pattern) = &field.regex_validation else {
            continue;
          };
          let key = RegexKey::for_struct(&def.name, &field.name);
          let pattern_key = pattern.clone();
          let const_name = if let Some(existing) = pattern_to_const.get(&pattern_key) {
            existing.clone()
          } else {
            let name = regex_const_name(&key.parts());
            pattern_to_const.insert(pattern_key.clone(), name.clone());
            const_defs.insert(name.clone(), pattern_key);
            name
          };
          lookup.insert(key, const_name);
        }
      }
      RustType::Enum(_) | RustType::TypeAlias(_) | RustType::DiscriminatedEnum(_) | RustType::ResponseEnum(_) => {}
    }
  }

  if const_defs.is_empty() {
    return (quote! {}, lookup);
  }

  let regex_defs: Vec<TokenStream> = const_defs
    .into_iter()
    .map(|(name, pattern)| {
      let ident = format_ident!("{}", name);
      quote! {
        static #ident: std::sync::LazyLock<regex::Regex> =
          std::sync::LazyLock::new(|| regex::Regex::new(#pattern).expect("invalid regex"));
      }
    })
    .collect();

  (quote! { #(#regex_defs)* }, lookup)
}

pub(crate) fn generate_header_constants(headers: &[&String]) -> TokenStream {
  if headers.is_empty() {
    return quote! {};
  }

  let const_tokens: Vec<TokenStream> = headers
    .iter()
    .map(|header| {
      let const_name = header_const_name(header);
      let ident = format_ident!("{}", const_name);
      quote! {
        pub const #ident: http::HeaderName = http::HeaderName::from_static(#header);
      }
    })
    .collect();

  quote! { #(#const_tokens)* }
}

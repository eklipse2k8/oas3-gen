use std::collections::{BTreeMap, btree_map::Entry};

use proc_macro2::TokenStream;
use quote::{format_ident, quote};

use crate::generator::{
  ast::{RegexKey, RustType, ValidationAttribute},
  naming::identifiers::{header_const_name, regex_const_name},
};

pub(crate) fn generate_regex_constants(types: &[&RustType]) -> (TokenStream, BTreeMap<RegexKey, String>) {
  let mut const_defs: BTreeMap<String, String> = BTreeMap::new();
  let mut lookup: BTreeMap<RegexKey, String> = BTreeMap::new();
  let mut pattern_to_const: BTreeMap<String, String> = BTreeMap::new();

  for rust_type in types {
    let RustType::Struct(def) = rust_type else {
      continue;
    };

    for field in &def.fields {
      let Some(pattern) = field.validation_attrs.iter().find_map(|attr| match attr {
        ValidationAttribute::Regex(p) => Some(p),
        _ => None,
      }) else {
        continue;
      };

      let key = RegexKey::for_struct(&def.name, &field.name);
      let const_name = match pattern_to_const.entry(pattern.clone()) {
        Entry::Occupied(entry) => entry.get().clone(),
        Entry::Vacant(entry) => {
          let name = regex_const_name(&key.parts());
          const_defs.insert(name.clone(), pattern.clone());
          entry.insert(name.clone());
          name
        }
      };
      lookup.insert(key, const_name);
    }
  }

  if const_defs.is_empty() {
    return (quote! {}, lookup);
  }

  let regex_defs: Vec<TokenStream> = const_defs
    .into_iter()
    .map(|(name, pattern)| {
      let ident = format_ident!("{name}");
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
      let ident = format_ident!("{const_name}");
      quote! {
        pub const #ident: http::HeaderName = http::HeaderName::from_static(#header);
      }
    })
    .collect();

  quote! { #(#const_tokens)* }
}

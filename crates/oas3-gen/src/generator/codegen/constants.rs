use std::collections::{BTreeMap, btree_map::Entry};

use proc_macro2::TokenStream;
use quote::quote;

use crate::generator::ast::{RegexKey, RustType, ValidationAttribute, constants::HttpHeaderRef, tokens::ConstToken};

pub(crate) fn generate_regex_constants(types: &[RustType]) -> (TokenStream, BTreeMap<RegexKey, ConstToken>) {
  let mut const_defs: BTreeMap<ConstToken, String> = BTreeMap::new();
  let mut lookup: BTreeMap<RegexKey, ConstToken> = BTreeMap::new();
  let mut pattern_to_const: BTreeMap<String, ConstToken> = BTreeMap::new();

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

      let key = RegexKey::for_struct(&def.name, field.name.as_str());
      let const_token = match pattern_to_const.entry(pattern.clone()) {
        Entry::Occupied(entry) => entry.get().clone(),
        Entry::Vacant(entry) => {
          let token = ConstToken::from(&key);
          const_defs.insert(token.clone(), pattern.clone());
          entry.insert(token.clone());
          token
        }
      };
      lookup.insert(key, const_token);
    }
  }

  if const_defs.is_empty() {
    return (quote! {}, lookup);
  }

  let regex_defs: Vec<TokenStream> = const_defs
    .into_iter()
    .map(|(const_token, pattern)| {
      quote! {
        static #const_token: std::sync::LazyLock<regex::Regex> =
          std::sync::LazyLock::new(|| regex::Regex::new(#pattern).expect("invalid regex"));
      }
    })
    .collect();

  (quote! { #(#regex_defs)* }, lookup)
}

pub(crate) fn generate_header_constants(headers: &[HttpHeaderRef]) -> TokenStream {
  if headers.is_empty() {
    return quote! {};
  }
  let const_tokens = headers.iter().map(|token| quote! { #token }).collect::<Vec<_>>();
  quote! { #(#const_tokens)* }
}

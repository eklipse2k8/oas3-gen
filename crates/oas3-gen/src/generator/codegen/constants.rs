use std::collections::{BTreeMap, btree_map::Entry};

use proc_macro2::TokenStream;
use quote::quote;

use crate::generator::ast::{
  RegexKey, RustType, ValidationAttribute,
  tokens::{ConstToken, HeaderToken, LinkServerToken},
};

pub(crate) fn generate_regex_constants(types: &[&RustType]) -> (TokenStream, BTreeMap<RegexKey, ConstToken>) {
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

pub(crate) fn generate_header_constants(headers: &[HeaderToken]) -> TokenStream {
  if headers.is_empty() {
    return quote! {};
  }

  let const_tokens: Vec<TokenStream> = headers
    .iter()
    .map(|header| {
      let const_token = &header.const_token;
      let header_name = &header.header_name;
      quote! {
        pub const #const_token: http::HeaderName = http::HeaderName::from_static(#header_name);
      }
    })
    .collect();

  quote! { #(#const_tokens)* }
}

pub(crate) fn generate_link_server_constants(servers: &[LinkServerToken]) -> TokenStream {
  if servers.is_empty() {
    return quote! {};
  }

  let const_tokens: Vec<TokenStream> = servers
    .iter()
    .map(|server| {
      let const_token = &server.const_token;
      let server_url = &server.server_url;
      let doc = format!("Alternative server URL for the `{}` link.", server.link_name);
      quote! {
        #[doc = #doc]
        pub const #const_token: &str = #server_url;
      }
    })
    .collect();

  quote! { #(#const_tokens)* }
}

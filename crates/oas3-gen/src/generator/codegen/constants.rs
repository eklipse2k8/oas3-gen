use std::collections::{BTreeMap, btree_map::Entry};

use proc_macro2::TokenStream;
use quote::{ToTokens, quote};

use crate::generator::ast::{RegexKey, RustType, ValidationAttribute, constants::HttpHeaderRef, tokens::ConstToken};

#[derive(Clone, Debug)]
pub(crate) struct RegexConstantFragment {
  const_token: ConstToken,
  pattern: String,
}

impl RegexConstantFragment {
  pub(crate) fn new(const_token: ConstToken, pattern: String) -> Self {
    Self { const_token, pattern }
  }
}

impl ToTokens for RegexConstantFragment {
  fn to_tokens(&self, tokens: &mut TokenStream) {
    let const_token = &self.const_token;
    let pattern = &self.pattern;
    tokens.extend(quote! {
      static #const_token: std::sync::LazyLock<regex::Regex> =
        std::sync::LazyLock::new(|| regex::Regex::new(#pattern).expect("invalid regex"));
    });
  }
}

#[derive(Clone, Debug, Default)]
pub(crate) struct RegexConstantsResult {
  fragments: Vec<RegexConstantFragment>,
  pub lookup: BTreeMap<RegexKey, ConstToken>,
}

impl RegexConstantsResult {
  pub(crate) fn from_types(types: &[RustType]) -> Self {
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

    let fragments = const_defs
      .into_iter()
      .map(|(const_token, pattern)| RegexConstantFragment::new(const_token, pattern))
      .collect();

    Self { fragments, lookup }
  }
}

impl ToTokens for RegexConstantsResult {
  fn to_tokens(&self, tokens: &mut TokenStream) {
    for fragment in &self.fragments {
      fragment.to_tokens(tokens);
    }
  }
}

#[derive(Clone, Debug, Default)]
pub(crate) struct HeaderConstantsFragment(Vec<HttpHeaderRef>);

impl HeaderConstantsFragment {
  pub(crate) fn new(headers: impl Into<Vec<HttpHeaderRef>>) -> Self {
    Self(headers.into())
  }
}

impl ToTokens for HeaderConstantsFragment {
  fn to_tokens(&self, tokens: &mut TokenStream) {
    for header in &self.0 {
      header.to_tokens(tokens);
    }
  }
}

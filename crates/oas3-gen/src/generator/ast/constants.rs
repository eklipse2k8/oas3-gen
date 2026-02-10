use inflections::Inflect as _;
use quote::{ToTokens, quote};

use crate::generator::{
  ast::{
    RegexKey,
    tokens::{ConstToken, HeaderNameToken},
  },
  naming::identifiers::sanitize,
};

impl From<&RegexKey> for ConstToken {
  fn from(key: &RegexKey) -> Self {
    let joined = key
      .parts()
      .iter()
      .map(|part| sanitize(part))
      .collect::<Vec<_>>()
      .join("_");

    let mut ident = joined.to_constant_case();

    if ident.starts_with(|c: char| c.is_ascii_digit()) {
      ident.insert(0, '_');
    }

    ConstToken::new(format!("REGEX_{ident}"))
  }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, bon::Builder)]
pub struct HttpHeaderRef {
  pub const_token: ConstToken,
  pub header_name: HeaderNameToken,
}

impl<T: ToString> From<T> for HttpHeaderRef {
  fn from(s: T) -> Self {
    let header_name_str = s.to_string();
    Self {
      const_token: ConstToken::from_raw(&header_name_str),
      header_name: HeaderNameToken::from_raw(&header_name_str),
    }
  }
}

impl ToTokens for HttpHeaderRef {
  fn to_tokens(&self, tokens: &mut proc_macro2::TokenStream) {
    let const_token = &self.const_token;
    let header_name = &self.header_name;

    let header = quote! {
      pub const #const_token: http::HeaderName = http::HeaderName::from_static(#header_name);
    };

    tokens.extend(header);
  }
}

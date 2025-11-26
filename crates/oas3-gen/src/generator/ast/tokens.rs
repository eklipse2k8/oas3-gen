use std::fmt::{Display, Formatter};

use proc_macro2::{Span, TokenStream};
use quote::ToTokens;
use string_cache::DefaultAtom;
use syn::Ident;

use crate::generator::naming::identifiers::header_const_name;

#[derive(Debug, Clone, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ConstToken(pub DefaultAtom);

impl From<&str> for ConstToken {
  fn from(s: &str) -> Self {
    ConstToken(DefaultAtom::from(header_const_name(s)))
  }
}

impl Display for ConstToken {
  fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
    self.0.fmt(f)
  }
}

impl ToTokens for ConstToken {
  fn to_tokens(&self, tokens: &mut TokenStream) {
    let token = Ident::new(&self.0, Span::call_site());
    token.to_tokens(tokens);
  }
}

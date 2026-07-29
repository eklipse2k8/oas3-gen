use proc_macro2::TokenStream;
use quote::{ToTokens, quote};

use super::Visibility;
use crate::generator::ast::TypeAliasDef;

#[derive(Clone, Copy, Debug)]
pub(crate) struct TypeAliasFragment<'a> {
  def: &'a TypeAliasDef,
  visibility: Visibility,
}

impl<'a> TypeAliasFragment<'a> {
  pub(crate) fn new(def: &'a TypeAliasDef, visibility: Visibility) -> Self {
    Self { def, visibility }
  }
}

impl ToTokens for TypeAliasFragment<'_> {
  fn to_tokens(&self, tokens: &mut TokenStream) {
    let name = &self.def.name;
    let docs = &self.def.docs;
    let target = &self.def.target;
    let vis = &self.visibility;

    tokens.extend(quote! {
      #docs
      #vis type #name = #target;
    });
  }
}

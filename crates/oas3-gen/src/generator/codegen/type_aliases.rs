use proc_macro2::TokenStream;
use quote::{ToTokens, quote};

use super::Visibility;
use crate::generator::ast::TypeAliasDef;

#[derive(Clone, Debug)]
pub(crate) struct TypeAliasFragment {
  def: TypeAliasDef,
  visibility: Visibility,
}

impl TypeAliasFragment {
  pub(crate) fn new(def: TypeAliasDef, visibility: Visibility) -> Self {
    Self { def, visibility }
  }
}

impl ToTokens for TypeAliasFragment {
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

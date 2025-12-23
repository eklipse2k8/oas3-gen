use proc_macro2::TokenStream;
use quote::{format_ident, quote};

use super::{Visibility, coercion};
use crate::generator::ast::TypeAliasDef;

pub(crate) fn generate_type_alias(def: &TypeAliasDef, visibility: Visibility) -> TokenStream {
  let name = format_ident!("{}", def.name);
  let docs = &def.docs;
  let vis = visibility.to_tokens();
  let target = coercion::parse_type_string(&def.target.to_rust_type());

  quote! {
    #docs
    #vis type #name = #target;
  }
}

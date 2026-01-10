use proc_macro2::TokenStream;
use quote::{ToTokens, quote};

use super::Visibility;
use crate::generator::ast::{ClientRootNode, OperationInfo};

pub struct ServerGenerator<'a> {
  metadata: &'a ClientRootNode,
  operations: &'a [OperationInfo],
  visibility: Visibility,
  with_types_import: bool,
}

impl<'a> ServerGenerator<'a> {
  pub fn new(metadata: &'a ClientRootNode, operations: &'a [OperationInfo], visibility: Visibility) -> Self {
    Self {
      metadata,
      operations,
      visibility,
      with_types_import: false,
    }
  }

  pub fn with_types_import(mut self) -> Self {
    self.with_types_import = true;
    self
  }
}

impl ToTokens for ServerGenerator<'_> {
  fn to_tokens(&self, tokens: &mut TokenStream) {
    let vis = self.visibility.to_tokens();

    let types_import = self.with_types_import.then(|| {
      quote! { use super::types::*; }
    });

    let stub = quote! {
      #types_import

      #vis trait ApiServer {
        // TODO: Generate trait methods from operations
      }
    };

    tokens.extend(stub);
  }
}

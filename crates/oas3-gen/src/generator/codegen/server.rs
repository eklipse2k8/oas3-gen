use proc_macro2::TokenStream;
use quote::{ToTokens, quote};

use super::Visibility;
use crate::generator::ast::{ClientRootNode, OperationInfo};

#[allow(dead_code)]
pub struct ServerGenerator {
  metadata: ClientRootNode,
  operations: Vec<OperationInfo>,
  visibility: Visibility,
  with_types_import: bool,
}

impl ServerGenerator {
  pub fn new(metadata: &ClientRootNode, operations: &[OperationInfo], visibility: Visibility) -> Self {
    Self {
      metadata: metadata.clone(),
      operations: operations.to_vec(),
      visibility,
      with_types_import: false,
    }
  }

  pub fn with_types_import(mut self) -> Self {
    self.with_types_import = true;
    self
  }
}

impl ToTokens for ServerGenerator {
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

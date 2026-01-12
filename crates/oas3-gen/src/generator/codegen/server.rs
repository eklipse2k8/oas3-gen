use proc_macro2::TokenStream;
use quote::{ToTokens, quote};

use super::Visibility;
use crate::generator::{
  ast::{ClientRootNode, OperationInfo, ResponseEnumDef, ResponseVariant},
  codegen::http::HttpStatusCode,
};

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

/// Wrapper to convert `ResponseEnumDef` to axum::IntoResponse impl tokens
#[derive(Clone, Debug)]
pub(crate) struct AxumIntoResponse(ResponseEnumDef);

impl AxumIntoResponse {
  pub(crate) fn new(def: ResponseEnumDef) -> Self {
    Self(def)
  }
}

impl ToTokens for AxumIntoResponse {
  fn to_tokens(&self, tokens: &mut TokenStream) {
    let name = &self.0.name;
    let variants = self
      .0
      .variants
      .iter()
      .map(|v| AxumIntoResponseVariant::new(v.clone()))
      .collect::<Vec<_>>();

    let ts = quote! {
      impl IntoResponse for #name {
        fn into_response(self) -> axum::response::Response {
          match self {
            #(#variants),*
          }
        }
      }
    };

    tokens.extend(ts);
  }
}

/// Wrapper to convert ResponseVariant to axum::Response tokens
#[derive(Clone, Debug)]
pub(crate) struct AxumIntoResponseVariant(ResponseVariant);

impl AxumIntoResponseVariant {
  pub(crate) fn new(variant: ResponseVariant) -> Self {
    Self(variant)
  }
}

impl ToTokens for AxumIntoResponseVariant {
  fn to_tokens(&self, tokens: &mut TokenStream) {
    let variant = &self.0.variant_name;
    let status_code = HttpStatusCode::new(self.0.status_code);

    let ts = if self.0.schema_type.is_some() {
      quote! {
        Self::#variant(data) => (#status_code, axum::Json(data)).into_response()
      }
    } else {
      quote! {
        Self::#variant => #status_code.into_response()
      }
    };

    tokens.extend(ts);
  }
}

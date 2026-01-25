use proc_macro2::TokenStream;
use quote::{ToTokens, quote};

use super::{Visibility, enums::ResponseEnumFragment};
use crate::generator::{
  ast::{ResponseEnumDef, ResponseVariant, ServerRequestTraitDef, ServerTraitMethod},
  codegen::http::HttpStatusCode,
};

pub struct ServerGenerator {
  server_trait: Option<ServerRequestTraitDef>,
  visibility: Visibility,
  with_types_import: bool,
}

impl ServerGenerator {
  pub fn new(server_trait: Option<ServerRequestTraitDef>, visibility: Visibility) -> Self {
    Self {
      server_trait,
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
    let types_import = self.with_types_import.then(|| quote! { use super::types::*; });

    let trait_def = self
      .server_trait
      .as_ref()
      .map(|def| ServerTraitFragment::new(def.clone(), self.visibility));

    tokens.extend(quote! {
      #types_import
      #trait_def
    });
  }
}

#[derive(Clone, Debug)]
struct ServerTraitFragment {
  def: ServerRequestTraitDef,
  vis: Visibility,
}

impl ServerTraitFragment {
  fn new(def: ServerRequestTraitDef, vis: Visibility) -> Self {
    Self { def, vis }
  }
}

impl ToTokens for ServerTraitFragment {
  fn to_tokens(&self, tokens: &mut TokenStream) {
    let vis = self.vis.to_tokens();
    let name = &self.def.name;
    let methods = self.def.methods.iter().cloned().map(ServerTraitMethodFragment);

    tokens.extend(quote! {
      #vis trait #name {
        #(#methods)*
      }
    });
  }
}

#[derive(Clone, Debug)]
struct ServerTraitMethodFragment(ServerTraitMethod);

impl ToTokens for ServerTraitMethodFragment {
  fn to_tokens(&self, tokens: &mut TokenStream) {
    let name = &self.0.name;
    let docs = &self.0.docs;

    let (request_param, return_type) = match (&self.0.request_type, &self.0.response_type) {
      (Some(req), Some(resp)) => (quote! { request: #req }, quote! { #resp }),
      (Some(req), None) => (quote! { request: #req }, quote! { () }),
      (None, Some(resp)) => (quote! {}, quote! { #resp }),
      (None, None) => (quote! {}, quote! { () }),
    };

    tokens.extend(quote! {
      #docs
      async fn #name(&self, #request_param) -> anyhow::Result<#return_type>;
    });
  }
}

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

#[derive(Clone, Debug)]
pub(crate) struct AxumResponseEnumFragment {
  vis: Visibility,
  def: ResponseEnumDef,
}

impl AxumResponseEnumFragment {
  pub(crate) fn new(vis: Visibility, def: ResponseEnumDef) -> Self {
    Self { vis, def }
  }
}

impl ToTokens for AxumResponseEnumFragment {
  fn to_tokens(&self, tokens: &mut TokenStream) {
    let response = ResponseEnumFragment::new(self.vis, self.def.clone());
    let into_response = AxumIntoResponse::new(self.def.clone());

    let ts = quote! {
      #response
      #into_response
    };

    tokens.extend(ts);
  }
}

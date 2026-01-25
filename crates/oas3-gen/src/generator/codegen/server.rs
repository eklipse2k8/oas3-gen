use std::collections::BTreeMap;

use http::Method;
use proc_macro2::TokenStream;
use quote::{ToTokens, format_ident, quote};

use super::{Visibility, enums::ResponseEnumFragment};
use crate::generator::{
  ast::{ContentCategory, HandlerBodyInfo, ResponseEnumDef, ResponseVariant, ServerRequestTraitDef, ServerTraitMethod},
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

    let Some(def) = self.server_trait.as_ref() else {
      return;
    };

    let trait_fragment = ServerTraitFragment::new(def.clone(), self.visibility);

    let handlers = def
      .methods
      .iter()
      .map(|m| HandlerFunctionFragment::new(m.clone(), self.visibility))
      .collect::<Vec<_>>();

    let router = RouterFragment::new(def.methods.clone(), self.visibility);

    tokens.extend(quote! {
      use axum::{
        Router,
        extract::{Path, Query, State},
        http::HeaderMap,
        response::IntoResponse,
        routing::{delete, get, head, options, patch, post, put, trace},
      };

      #types_import

      #trait_fragment

      #(#handlers)*

      #router
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
      #vis trait #name: Send + Sync {
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
      fn #name(&self, #request_param) -> impl std::future::Future<Output = anyhow::Result<#return_type>> + Send;
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

#[derive(Clone, Debug)]
struct HandlerFunctionFragment {
  method: ServerTraitMethod,
  vis: Visibility,
}

impl HandlerFunctionFragment {
  fn new(method: ServerTraitMethod, vis: Visibility) -> Self {
    Self { method, vis }
  }
}

impl ToTokens for HandlerFunctionFragment {
  fn to_tokens(&self, tokens: &mut TokenStream) {
    let vis = self.vis.to_tokens();
    let fn_name = &self.method.name;
    let trait_name = format_ident!("ApiServer");

    let extractors = ExtractorsFragment::new(self.method.clone());
    let request_construction = RequestConstructionFragment::new(self.method.clone());

    let return_type = self
      .method
      .response_type
      .as_ref()
      .map_or_else(|| quote! { impl IntoResponse }, |resp| quote! { #resp });

    let service_call = if self.method.request_type.is_some() {
      quote! { service.#fn_name(request).await }
    } else {
      quote! { service.#fn_name().await }
    };

    let error_handling = quote! {
      match result {
        Ok(response) => response.into_response(),
        Err(e) => (
          axum::http::StatusCode::INTERNAL_SERVER_ERROR,
          format!("Internal error: {e}")
        ).into_response(),
      }
    };

    tokens.extend(quote! {
      #vis async fn #fn_name<S>(
        #extractors
      ) -> impl IntoResponse
      where
        S: #trait_name + Clone + Send + Sync + 'static,
      {
        #request_construction
        let result: anyhow::Result<#return_type> = #service_call;
        #error_handling
      }
    });
  }
}

#[derive(Clone, Debug)]
struct ExtractorsFragment {
  method: ServerTraitMethod,
}

impl ExtractorsFragment {
  fn new(method: ServerTraitMethod) -> Self {
    Self { method }
  }
}

impl ToTokens for ExtractorsFragment {
  fn to_tokens(&self, tokens: &mut TokenStream) {
    let mut parts = vec![quote! { State(service): State<S> }];

    if let Some(path_type) = &self.method.path_params_type {
      parts.push(quote! { Path(path): Path<#path_type> });
    }

    if let Some(query_type) = &self.method.query_params_type {
      parts.push(quote! { Query(query): Query<#query_type> });
    }

    if self.method.header_params_type.is_some() {
      parts.push(quote! { headers: HeaderMap });
    }

    if let Some(body_info) = &self.method.body_info {
      let body_extractor = BodyExtractorFragment::new(body_info.clone());
      parts.push(body_extractor.into_token_stream());
    }

    let ts = quote! { #(#parts),* };
    tokens.extend(ts);
  }
}

#[derive(Clone, Debug)]
struct BodyExtractorFragment {
  body_info: HandlerBodyInfo,
}

impl BodyExtractorFragment {
  fn new(body_info: HandlerBodyInfo) -> Self {
    Self { body_info }
  }
}

impl ToTokens for BodyExtractorFragment {
  fn to_tokens(&self, tokens: &mut TokenStream) {
    let body_type = &self.body_info.body_type;

    let ts = match self.body_info.content_category {
      ContentCategory::Json | ContentCategory::Multipart => {
        if self.body_info.optional {
          quote! { body: Option<axum::Json<#body_type>> }
        } else {
          quote! { axum::Json(body): axum::Json<#body_type> }
        }
      }
      ContentCategory::FormUrlEncoded => {
        if self.body_info.optional {
          quote! { body: Option<axum::extract::Form<#body_type>> }
        } else {
          quote! { axum::extract::Form(body): axum::extract::Form<#body_type> }
        }
      }
      ContentCategory::Text | ContentCategory::EventStream | ContentCategory::Xml => {
        if self.body_info.optional {
          quote! { body: Option<String> }
        } else {
          quote! { body: String }
        }
      }
      ContentCategory::Binary => {
        if self.body_info.optional {
          quote! { body: Option<axum::body::Bytes> }
        } else {
          quote! { body: axum::body::Bytes }
        }
      }
    };

    tokens.extend(ts);
  }
}

#[derive(Clone, Debug)]
struct RequestConstructionFragment {
  method: ServerTraitMethod,
}

impl RequestConstructionFragment {
  fn new(method: ServerTraitMethod) -> Self {
    Self { method }
  }
}

impl ToTokens for RequestConstructionFragment {
  fn to_tokens(&self, tokens: &mut TokenStream) {
    let Some(request_type) = &self.method.request_type else {
      return;
    };

    let mut field_assignments = vec![];

    if self.method.path_params_type.is_some() {
      field_assignments.push(quote! { path });
    }

    if self.method.query_params_type.is_some() {
      field_assignments.push(quote! { query });
    }

    if self.method.header_params_type.is_some() {
      field_assignments.push(quote! {
        header: (&headers).try_into().unwrap_or_default()
      });
    }

    if let Some(body_info) = &self.method.body_info {
      let needs_unwrap = matches!(
        body_info.content_category,
        ContentCategory::Json | ContentCategory::FormUrlEncoded | ContentCategory::Multipart
      );
      let body_expr = if needs_unwrap && body_info.optional {
        quote! { body: body.map(|b| b.0) }
      } else {
        quote! { body }
      };
      field_assignments.push(body_expr);
    }

    tokens.extend(quote! {
      let request = #request_type {
        #(#field_assignments),*
      };
    });
  }
}

#[derive(Clone, Debug)]
struct RouterFragment {
  methods: Vec<ServerTraitMethod>,
  vis: Visibility,
}

impl RouterFragment {
  fn new(methods: Vec<ServerTraitMethod>, vis: Visibility) -> Self {
    Self { methods, vis }
  }
}

impl ToTokens for RouterFragment {
  fn to_tokens(&self, tokens: &mut TokenStream) {
    let vis = self.vis.to_tokens();

    let routes_by_path: BTreeMap<String, Vec<&ServerTraitMethod>> =
      self.methods.iter().fold(BTreeMap::new(), |mut acc, method| {
        let path = method.path.to_axum_path();
        acc.entry(path).or_default().push(method);
        acc
      });

    let route_definitions = routes_by_path.into_iter().map(|(path, methods)| {
      let method_handlers = methods.iter().map(|m| {
        let fn_name = &m.name;
        let http_method = HttpMethodFragment::new(m.http_method.clone());
        quote! { #http_method(#fn_name::<S>) }
      });

      let chained = method_handlers.reduce(|acc, handler| quote! { #acc.#handler });

      quote! { .route(#path, #chained) }
    });

    tokens.extend(quote! {
      #vis fn router<S>(service: S) -> Router
      where
        S: ApiServer + Clone + Send + Sync + 'static,
      {
        Router::new()
          #(#route_definitions)*
          .with_state(service)
      }
    });
  }
}

#[derive(Clone, Debug)]
struct HttpMethodFragment {
  method: Method,
}

impl HttpMethodFragment {
  fn new(method: Method) -> Self {
    Self { method }
  }
}

impl ToTokens for HttpMethodFragment {
  fn to_tokens(&self, tokens: &mut TokenStream) {
    let ts = match self.method {
      Method::POST => quote! { post },
      Method::PUT => quote! { put },
      Method::DELETE => quote! { delete },
      Method::PATCH => quote! { patch },
      Method::HEAD => quote! { head },
      Method::OPTIONS => quote! { options },
      Method::TRACE => quote! { trace },
      _ => quote! { get },
    };
    tokens.extend(ts);
  }
}

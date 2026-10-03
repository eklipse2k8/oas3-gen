use http::Method;
use indexmap::IndexMap;
use itertools::Itertools as _;
use proc_macro2::TokenStream;
use quote::{ToTokens, quote};

use super::Visibility;
use crate::generator::{
  ast::{
    ApiKeyLocation, ContentCategory, CredentialKind, CredentialScheme, HandlerBodyInfo, HandlerCredentials,
    OperationResponse, ResponseEnumDef, ResponseParam, ResponsePayload, RustPrimitive, ServerRequestTraitDef,
    ServerTraitMethod, TraitToken, constants::HttpHeaderRef,
  },
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
    let credentials = def
      .methods
      .iter()
      .filter_map(|m| m.credentials.as_ref())
      .unique_by(|credentials| &credentials.type_name)
      .map(CredentialsExtractorFragment::new);

    let handlers = def
      .methods
      .iter()
      .map(|m| HandlerFunctionFragment::new(m.clone(), def.name.clone(), def.response_enum.as_ref(), self.visibility))
      .collect::<Vec<_>>();

    let router = RouterFragment::new(def.methods.clone(), def.name.clone(), self.visibility);

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

      #(#credentials)*

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

/// Extracts one credentials struct from the request parts, answering `401` when the
/// credentials satisfy none of the operation's security requirements.
///
/// Handlers take it before the body, so axum refuses the request without reading it.
#[derive(Clone, Copy, Debug)]
struct CredentialsExtractorFragment<'a> {
  credentials: &'a HandlerCredentials,
}

impl<'a> CredentialsExtractorFragment<'a> {
  fn new(credentials: &'a HandlerCredentials) -> Self {
    Self { credentials }
  }
}

impl ToTokens for CredentialsExtractorFragment<'_> {
  fn to_tokens(&self, tokens: &mut TokenStream) {
    let type_name = &self.credentials.type_name;
    let security = &self.credentials.security;

    let query = security.reads(ApiKeyLocation::Query).then(|| {
      quote! {
        let Query(query) = Query::<Vec<(String, String)>>::try_from_uri(&parts.uri).unwrap_or_default();
      }
    });
    let cookies = security.reads(ApiKeyLocation::Cookie).then(|| {
      quote! { let cookies = axum_extra::extract::CookieJar::from_headers(&parts.headers); }
    });

    let fields = security.schemes.iter().map(|scheme| {
      let field = &scheme.field;
      let value = credential_value(scheme);
      if security.requires(field) {
        let missing = format!("missing {} in {}", scheme.kind.noun(), scheme.whereabouts());
        quote! { #field: #value.ok_or((axum::http::StatusCode::UNAUTHORIZED, #missing))? }
      } else {
        quote! { #field: #value }
      }
    });

    let satisfied = security
      .unchecked_alternatives()
      .into_iter()
      .map(|fields| {
        let present = fields.iter().map(|field| quote! { credentials.#field.is_some() });
        if fields.len() > 1 {
          quote! { (#(#present)&&*) }
        } else {
          quote! { #(#present)* }
        }
      })
      .collect::<Vec<_>>();

    let body = if satisfied.is_empty() {
      quote! { Ok(Self { #(#fields),* }) }
    } else {
      let missing = format!("missing {}", security.describe_alternatives());
      quote! {
        let credentials = Self { #(#fields),* };
        if !(#(#satisfied)||*) {
          return Err((axum::http::StatusCode::UNAUTHORIZED, #missing));
        }
        Ok(credentials)
      }
    };

    tokens.extend(quote! {
      impl #type_name {
        fn from_parts(
          parts: &axum::http::request::Parts,
        ) -> Result<Self, (axum::http::StatusCode, &'static str)> {
          #query
          #cookies
          #body
        }
      }

      impl<S: Send + Sync> axum::extract::FromRequestParts<S> for #type_name {
        type Rejection = (axum::http::StatusCode, &'static str);

        fn from_request_parts(
          parts: &mut axum::http::request::Parts,
          _state: &S,
        ) -> impl std::future::Future<Output = Result<Self, Self::Rejection>> + Send {
          std::future::ready(Self::from_parts(parts))
        }
      }
    });
  }
}

/// Reads one credential, as an optional `secrecy::SecretString`, from the request
/// part it travels in.
///
/// A bearer token is the `Authorization` header's value after a `Bearer` scheme,
/// which matches in any case.
fn credential_value(scheme: &CredentialScheme) -> TokenStream {
  match &scheme.kind {
    CredentialKind::ApiKey {
      location: ApiKeyLocation::Header,
      parameter_name,
    } => {
      let header = HttpHeaderRef::from(parameter_name).path();
      quote! {
        parts.headers.get(#header).and_then(|value| value.to_str().ok()).map(secrecy::SecretString::from)
      }
    }
    CredentialKind::ApiKey {
      location: ApiKeyLocation::Query,
      parameter_name,
    } => quote! {
      query
        .iter()
        .find(|(name, _)| name == #parameter_name)
        .map(|(_, value)| secrecy::SecretString::from(value.as_str()))
    },
    CredentialKind::ApiKey {
      location: ApiKeyLocation::Cookie,
      parameter_name,
    } => quote! {
      cookies.get(#parameter_name).map(|cookie| secrecy::SecretString::from(cookie.value_trimmed()))
    },
    CredentialKind::Bearer => quote! {
      parts
        .headers
        .get(http::header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.split_once(' '))
        .filter(|(scheme, _)| scheme.eq_ignore_ascii_case("bearer"))
        .map(|(_, token)| secrecy::SecretString::from(token.trim_start()))
    },
  }
}

#[derive(Clone, Debug)]
struct ServerTraitMethodFragment(ServerTraitMethod);

impl ToTokens for ServerTraitMethodFragment {
  fn to_tokens(&self, tokens: &mut TokenStream) {
    let name = &self.0.name;
    let docs = &self.0.docs;

    let (request_param, return_type) = match (&self.0.request_type, &self.0.response) {
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
struct HandlerFunctionFragment<'a> {
  method: ServerTraitMethod,
  trait_name: TraitToken,
  response_enum: Option<&'a ResponseEnumDef>,
  vis: Visibility,
}

impl<'a> HandlerFunctionFragment<'a> {
  fn new(
    method: ServerTraitMethod,
    trait_name: TraitToken,
    response_enum: Option<&'a ResponseEnumDef>,
    vis: Visibility,
  ) -> Self {
    Self {
      method,
      trait_name,
      response_enum,
      vis,
    }
  }
}

impl ToTokens for HandlerFunctionFragment<'_> {
  fn to_tokens(&self, tokens: &mut TokenStream) {
    let vis = self.vis.to_tokens();
    let fn_name = &self.method.name;
    let trait_name = &self.trait_name;

    let extractors = ExtractorsFragment::new(self.method.clone());
    let request_construction = RequestConstructionFragment::new(self.method.clone());

    let return_type = self
      .method
      .response
      .as_ref()
      .map_or_else(|| quote! { impl IntoResponse }, |resp| quote! { #resp });

    let response_arms = if let (Some(response), Some(response_enum)) = (&self.method.response, self.response_enum) {
      ResponseArmsFragment::new(response_enum, response).into_token_stream()
    } else {
      quote! { Ok(response) => response.into_response(), }
    };

    let service_call = if self.method.request_type.is_some() {
      quote! { service.#fn_name(request).await }
    } else {
      quote! { service.#fn_name().await }
    };

    let internal_error = internal_error();
    let error_handling = quote! {
      match result {
        #response_arms
        Err(e) => #internal_error,
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

/// Answers `500` with the error bound as `e` in the enclosing match arm.
fn internal_error() -> TokenStream {
  quote! {
    (
      axum::http::StatusCode::INTERNAL_SERVER_ERROR,
      format!("Internal error: {e}")
    ).into_response()
  }
}

#[derive(Clone, Copy, Debug)]
struct ResponseArmsFragment<'a> {
  response_enum: &'a ResponseEnumDef,
  response: &'a OperationResponse,
}

impl<'a> ResponseArmsFragment<'a> {
  fn new(response_enum: &'a ResponseEnumDef, response: &'a OperationResponse) -> Self {
    Self {
      response_enum,
      response,
    }
  }

  fn body_arms(
    variant_path: &TokenStream,
    status_pattern: Option<&TokenStream>,
    status_expr: &TokenStream,
    param: Option<&ResponseParam>,
  ) -> TokenStream {
    let wrapper = param.and_then(|p| p.headers.as_ref()).map(|headers| &headers.wrapper);
    let arm = |pattern: TokenStream, headed_pattern: TokenStream, body: Option<TokenStream>| {
      let (payload, response) = match (wrapper, body) {
        (Some(wrapper), body) => {
          let body = body.map(|body| quote! { , #body });
          let internal_error = internal_error();
          (
            quote! { #wrapper { headers, #headed_pattern } },
            quote! {
              match http::HeaderMap::try_from(headers) {
                Ok(headers) => (#status_expr, headers #body).into_response(),
                Err(e) => #internal_error,
              }
            },
          )
        }
        (None, Some(body)) => (pattern, quote! { (#status_expr, #body).into_response() }),
        (None, None) => (pattern, quote! { #status_expr.into_response() }),
      };
      let fields = status_pattern.into_iter().cloned().chain(std::iter::once(payload));

      quote! { Ok(#variant_path(#(#fields),*)) => #response, }
    };

    match param {
      Some(param) if param.body.nullable => {
        let some = arm(
          quote! { Some(body) },
          quote! { body: Some(body) },
          Some(quote! { axum::Json(body) }),
        );
        let none = arm(quote! { None }, quote! { body: None }, None);
        quote! { #some #none }
      }
      Some(param) if !matches!(param.body.base_type, RustPrimitive::Unit) => {
        arm(quote! { body }, quote! { body }, Some(quote! { axum::Json(body) }))
      }
      _ => arm(quote! { _ }, quote! { .. }, None),
    }
  }
}

impl ToTokens for ResponseArmsFragment<'_> {
  fn to_tokens(&self, tokens: &mut TokenStream) {
    let response_enum_name = &self.response_enum.name;

    for variant in &self.response_enum.variants {
      let variant_name = &variant.name;
      let variant_path = quote! { #response_enum_name::#variant_name };
      let status_code = HttpStatusCode::new(variant.status_code);
      let (status_pattern, status_expr) = if variant.status_code.carries_status() {
        (Some(quote! { status }), quote! { status })
      } else {
        (None, quote! { #status_code })
      };

      let arms = match variant.payload {
        ResponsePayload::None => {
          let fields = status_pattern.as_ref().map(|status| quote! { (#status) });
          quote! { Ok(#variant_path #fields) => #status_expr.into_response(), }
        }
        ResponsePayload::Raw => {
          quote! { Ok(#variant_path(status, body)) => (status, body).into_response(), }
        }
        ResponsePayload::Value | ResponsePayload::Failure => Self::body_arms(
          &variant_path,
          status_pattern.as_ref(),
          &status_expr,
          self.response.param(variant.payload),
        ),
      };

      tokens.extend(arms);
    }
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

    if let Some(credentials) = &self.method.credentials {
      let field = &credentials.field;
      let credentials_type = &credentials.type_name;
      parts.push(quote! { #field: #credentials_type });
    }

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

    let header = self.method.header_params_type.as_ref().map(|header_type| {
      quote! {
        let header = match #header_type::try_from(&headers) {
          Ok(header) => header,
          Err(e) => return (
            axum::http::StatusCode::BAD_REQUEST,
            format!("Bad request: {e}")
          ).into_response(),
        };
      }
    });
    if header.is_some() {
      field_assignments.push(quote! { header });
    }

    if let Some(credentials) = &self.method.credentials {
      let field = &credentials.field;
      field_assignments.push(quote! { #field });
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
      #header
      let request = #request_type {
        #(#field_assignments),*
      };
    });
  }
}

#[derive(Clone, Debug)]
struct RouterFragment {
  methods: Vec<ServerTraitMethod>,
  trait_name: TraitToken,
  vis: Visibility,
}

impl RouterFragment {
  fn new(methods: Vec<ServerTraitMethod>, trait_name: TraitToken, vis: Visibility) -> Self {
    Self {
      methods,
      trait_name,
      vis,
    }
  }
}

impl ToTokens for RouterFragment {
  fn to_tokens(&self, tokens: &mut TokenStream) {
    let vis = self.vis.to_tokens();
    let trait_name = &self.trait_name;

    let routes_by_path: IndexMap<String, Vec<&ServerTraitMethod>> =
      self.methods.iter().fold(IndexMap::new(), |mut acc, method| {
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
        S: #trait_name + Clone + Send + Sync + 'static,
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

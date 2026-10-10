use http::Method;
use indexmap::IndexMap;
use itertools::Itertools as _;
use proc_macro2::TokenStream;
use quote::{ToTokens, quote};

use super::{Visibility, cookies::request_cookie_lookup, multipart::MultipartDecodeFragment};
use crate::generator::{
  ast::{
    ApiKeyLocation, BodyMediaType, ContentCategory, CredentialKind, CredentialScheme, HandlerBodyInfo,
    HandlerCredentials, OperationResponse, ResponseEnumDef, ResponseParam, ResponsePayload, RustPrimitive,
    ServerRequestTraitDef, ServerTraitMethod, StatusCodeToken, StreamItem, StructToken, TraitToken,
    constants::HttpHeaderRef,
  },
  codegen::http::HttpStatusCode,
  naming::constants::DEFAULT_MEDIA_TYPE,
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
        extract::{Path, State},
        http::HeaderMap,
        response::IntoResponse,
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
        let axum::extract::Query(query) =
          axum::extract::Query::<Vec<(String, String)>>::try_from_uri(&parts.uri).unwrap_or_default();
      }
    });
    let cookies = security
      .reads(ApiKeyLocation::Cookie)
      .then(|| request_cookie_lookup(&quote! { parts.headers }));

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
    } => quote! { cookie(#parameter_name).map(secrecy::SecretString::from) },
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

    let multipart_decode = self
      .method
      .body_info
      .as_ref()
      .filter(|body_info| body_info.content_category == ContentCategory::Multipart)
      .map(MultipartDecodeFragment::new);

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
        #multipart_decode
        #request_construction
        let result: anyhow::Result<#return_type> = #service_call;
        #error_handling
      }
    });
  }
}

/// Answers `400` with the error bound as `e` in the enclosing match arm.
pub(super) fn bad_request() -> TokenStream {
  quote! {
    (
      axum::http::StatusCode::BAD_REQUEST,
      format!("Bad request: {e}")
    ).into_response()
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

  /// The payload pattern and response of each arm for one status code's body. A response
  /// union gets one arm per member, each sent under the media type that member was
  /// declared with.
  fn body_arms(status_expr: &TokenStream, param: Option<&ResponseParam>, status: StatusCodeToken) -> Vec<MatchArm> {
    let wrapper = param.and_then(|p| p.headers.as_ref()).map(|headers| &headers.wrapper);
    let arm = |pattern: TokenStream, headed_pattern: TokenStream, body: Option<TokenStream>| match (wrapper, body) {
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

    let Some(param) = param.filter(|param| !matches!(param.body.base_type, RustPrimitive::Unit)) else {
      return vec![arm(quote! { _ }, quote! { .. }, None)];
    };
    let union = &param.body.base_type;
    let members = param
      .media_types
      .iter()
      .map(|entry| entry.member.as_ref())
      .unique()
      .collect::<Vec<_>>();
    let members = if members.is_empty() { vec![None] } else { members };

    let mut arms = members
      .into_iter()
      .map(|member| {
        let body = Some(Self::body_response(param.body_media(status, member)));
        let (inner, field) = match member {
          Some(member) => (quote! { #union::#member(body) }, quote! { body: #union::#member(body) }),
          None => (quote! { body }, quote! { body }),
        };
        if param.body.nullable {
          arm(quote! { Some(#inner) }, quote! { body: Some(#inner) }, body)
        } else {
          arm(inner, field, body)
        }
      })
      .collect::<Vec<_>>();
    if param.body.nullable {
      arms.push(arm(quote! { None }, quote! { body: None }, None));
    }
    arms
  }

  /// The response for a body bound as `body`, encoded and labelled as `entry` says, or
  /// serialized as JSON without one.
  ///
  /// JSON keeps a declared label other than `application/json`. Text bodies other than
  /// strings and bytes are written through `Display`. An event stream goes through axum's
  /// `Sse`, sending each item as the JSON data of one event, or as one whole event for a
  /// frame type, and ends with its sentinel when it declares one.
  fn body_response(entry: Option<&BodyMediaType>) -> TokenStream {
    let Some(entry) = entry else {
      return quote! { axum::Json(body) };
    };
    let content_type = entry.content_type.as_str();
    let declared = |body: TokenStream| {
      quote! {
        (
          [(axum::http::header::CONTENT_TYPE, axum::http::HeaderValue::from_static(#content_type))],
          #body
        )
      }
    };

    match entry.category {
      ContentCategory::Json if content_type != DEFAULT_MEDIA_TYPE => declared(quote! { axum::Json(body) }),
      ContentCategory::Text if matches!(entry.body.base_type, RustPrimitive::String | RustPrimitive::Bytes) => {
        declared(quote! { body })
      }
      ContentCategory::Text => declared(quote! { body.to_string() }),
      ContentCategory::Binary => declared(quote! { body }),
      ContentCategory::EventStream => {
        let stream = entry.stream.as_ref();
        let events = match stream.map(|stream| stream.item).unwrap_or_default() {
          StreamItem::Frame(_) | StreamItem::FrameUnion => {
            quote! { futures::StreamExt::map(body, axum::response::sse::Event::try_from) }
          }
          StreamItem::Payload => {
            quote! { futures::StreamExt::map(body, |item| axum::response::sse::Event::default().json_data(item)) }
          }
        };
        let events = match stream.and_then(|stream| stream.sentinel.as_deref()) {
          Some(sentinel) => quote! {
            futures::StreamExt::chain(
              #events,
              futures::stream::iter([Ok(axum::response::sse::Event::default().data(#sentinel))]),
            )
          },
          None => events,
        };
        quote! { axum::response::sse::Sse::new(#events) }
      }
      ContentCategory::Json | ContentCategory::Xml | ContentCategory::Multipart | ContentCategory::FormUrlEncoded => {
        quote! { axum::Json(body) }
      }
    }
  }
}

/// One arm of the `match` over a service result: the payload pattern inside the
/// status variant's `Ok(..)`, and the response it sends.
type MatchArm = (TokenStream, TokenStream);

/// Arms that send the same response, joined into one or-pattern. Arms only join when
/// their payload patterns match, so every alternative binds `body` at the same type.
type MergedArms = IndexMap<(ResponsePayload, String, String), (Vec<TokenStream>, TokenStream)>;

impl ToTokens for ResponseArmsFragment<'_> {
  fn to_tokens(&self, tokens: &mut TokenStream) {
    let response_enum_name = &self.response_enum.name;
    let mut merged = MergedArms::new();

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
          vec![(
            quote! { #variant_path #fields },
            String::new(),
            quote! { #status_expr.into_response() },
          )]
        }
        ResponsePayload::Raw => vec![(
          quote! { #variant_path(status, body) },
          String::new(),
          quote! { (status, body).into_response() },
        )],
        ResponsePayload::Unread => vec![],
        ResponsePayload::Value | ResponsePayload::Failure => {
          Self::body_arms(&status_expr, self.response.param(variant.payload), variant.status_code)
            .into_iter()
            .map(|(payload, response)| {
              let fields = status_pattern.iter().chain(std::iter::once(&payload));
              (quote! { #variant_path(#(#fields),*) }, payload.to_string(), response)
            })
            .collect()
        }
      };

      for (pattern, payload, response) in arms {
        let key = (variant.payload, payload, response.to_string());
        merged.entry(key).or_insert_with(|| (vec![], response)).0.push(pattern);
      }
    }

    for (patterns, response) in merged.into_values() {
      tokens.extend(quote! { Ok(#(#patterns)|*) => #response, });
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
      parts.push(quote! { axum_extra::extract::Query(query): axum_extra::extract::Query<#query_type> });
    }

    let optional_body_without_option = self.method.body_info.as_ref().is_some_and(|body_info| {
      body_info.optional
        && matches!(
          body_info.content_category,
          ContentCategory::Multipart | ContentCategory::FormUrlEncoded
        )
    });
    if self.method.header_params_type.is_some()
      || self.method.cookie_params_type.is_some()
      || optional_body_without_option
    {
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
      ContentCategory::Multipart => {
        if self.body_info.optional {
          quote! { multipart: Option<axum::extract::Multipart> }
        } else {
          quote! { multipart: axum::extract::Multipart }
        }
      }
      ContentCategory::Json => {
        if self.body_info.optional {
          quote! { body: Result<Option<axum::Json<#body_type>>, axum::extract::rejection::JsonRejection> }
        } else {
          quote! { body: Result<axum::Json<#body_type>, axum::extract::rejection::JsonRejection> }
        }
      }
      ContentCategory::FormUrlEncoded => {
        quote! { body: Result<axum::extract::Form<#body_type>, axum::extract::rejection::FormRejection> }
      }
      ContentCategory::Text | ContentCategory::EventStream | ContentCategory::Xml => quote! { body: String },
      ContentCategory::Binary => quote! { body: axum::body::Bytes },
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

    let header = self
      .method
      .header_params_type
      .as_ref()
      .map(|header_type| parse_from_headers(&quote! { header }, header_type));
    if header.is_some() {
      field_assignments.push(quote! { header });
    }

    let cookie = self
      .method
      .cookie_params_type
      .as_ref()
      .map(|cookie_type| parse_from_headers(&quote! { cookie }, cookie_type));
    if cookie.is_some() {
      field_assignments.push(quote! { cookie });
    }

    if let Some(credentials) = &self.method.credentials {
      let field = &credentials.field;
      field_assignments.push(quote! { #field });
    }

    let body = self.method.body_info.as_ref().and_then(|body_info| {
      let bad_request = bad_request();
      let accepted = match (body_info.content_category, body_info.optional) {
        (ContentCategory::Json, false) => quote! { Ok(axum::Json(body)) => body, },
        (ContentCategory::Json, true) => quote! { Ok(body) => body.map(|axum::Json(body)| body), },
        (ContentCategory::FormUrlEncoded, false) => quote! { Ok(axum::extract::Form(body)) => body, },
        (ContentCategory::FormUrlEncoded, true) => quote! {
          Ok(axum::extract::Form(body)) => Some(body),
          Err(_) if !headers.contains_key(axum::http::header::CONTENT_TYPE) => None,
        },
        _ => return None,
      };
      Some(quote! {
        let body = match body {
          #accepted
          Err(e) => return #bad_request,
        };
      })
    });

    if let Some(body_info) = &self.method.body_info {
      let body_expr = match (body_info.content_category, body_info.optional) {
        (ContentCategory::Text | ContentCategory::EventStream | ContentCategory::Xml, true) => {
          quote! { body: (!body.is_empty()).then_some(body) }
        }
        (ContentCategory::Binary, true) => quote! { body: (!body.is_empty()).then(|| Vec::from(body)) },
        (ContentCategory::Binary, false) => quote! { body: Vec::from(body) },
        _ => quote! { body },
      };
      field_assignments.push(body_expr);
    }

    tokens.extend(quote! {
      #header
      #cookie
      #body
      let request = #request_type {
        #(#field_assignments),*
      };
    });
  }
}

/// Binds `binding` to the parameter struct `params_type` parsed from the request
/// `headers`, answering `400` when they don't parse.
fn parse_from_headers(binding: &TokenStream, params_type: &StructToken) -> TokenStream {
  let bad_request = bad_request();
  quote! {
    let #binding = match #params_type::try_from(&headers) {
      Ok(#binding) => #binding,
      Err(e) => return #bad_request,
    };
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

      quote! { .route(#path, axum::routing::#chained) }
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

use anyhow::Context as _;
use http::Method;
use itertools::Itertools;
use proc_macro2::{Span, TokenStream};
use quote::{ToTokens, format_ident, quote};
use syn::{Ident, LitStr};

use super::{
  Visibility, attributes::generate_deprecated_attr, cookies::CookieHeaderFragment, multipart::MultipartFormFragment,
};
use crate::generator::{
  ast::{
    ApiKeyLocation, ClientRootNode, ContentCategory, CredentialKind, CredentialScheme, Documentation, EnumVariantToken,
    FieldDef, FieldNameToken, OperationBody, OperationInfo, OperationKind, OperationResponse, OperationSecurity,
    ParameterLocation, ParsedPath, StructToken, constants::HttpHeaderRef,
  },
  naming::constants::DEFAULT_MEDIA_TYPE,
};

#[derive(Clone, Debug)]
pub(crate) struct HttpInitFragment {
  method: Method,
}

impl HttpInitFragment {
  pub(crate) fn new(method: Method) -> Self {
    Self { method }
  }
}

impl ToTokens for HttpInitFragment {
  fn to_tokens(&self, tokens: &mut TokenStream) {
    let ts = match self.method {
      Method::GET => quote! { self.client.get(url) },
      Method::POST => quote! { self.client.post(url) },
      Method::PUT => quote! { self.client.put(url) },
      Method::DELETE => quote! { self.client.delete(url) },
      Method::PATCH => quote! { self.client.patch(url) },
      Method::HEAD => quote! { self.client.head(url) },
      _ => {
        let method = format_ident!("{}", self.method.as_str());
        quote! { self.client.request(reqwest::Method::#method, url) }
      }
    };
    tokens.extend(ts);
  }
}

/// Appends an operation's query parameters to `url`.
///
/// `serde_html_form` writes the query struct into `url.query_pairs_mut()`, after any query
/// the path declares, and repeats an exploded array's name once per item, the way the
/// generated server's `axum_extra::extract::Query` reads it back. A query whose parameters
/// are all `None` leaves no empty `?` behind.
#[derive(Clone, Debug)]
pub(crate) struct QueryParamsFragment {
  has_query: bool,
}

impl QueryParamsFragment {
  pub(crate) fn new(parameters: &[FieldDef]) -> Self {
    let has_query = parameters
      .iter()
      .any(|p| matches!(p.parameter_location, Some(ParameterLocation::Query)));
    Self { has_query }
  }
}

impl ToTokens for QueryParamsFragment {
  fn to_tokens(&self, tokens: &mut TokenStream) {
    if self.has_query {
      tokens.extend(quote! {
        serde::Serialize::serialize(&request.query, serde_html_form::Serializer::new(&mut url.query_pairs_mut()))
          .context("encoding query parameters")?;
        if url.query() == Some("") {
          url.set_query(None);
        }
      });
    }
  }
}

#[derive(Clone, Debug)]
pub(crate) struct HeaderParamsFragment {
  has_headers: bool,
}

impl HeaderParamsFragment {
  pub(crate) fn new(parameters: &[FieldDef]) -> Self {
    let has_headers = parameters
      .iter()
      .any(|p| matches!(p.parameter_location, Some(ParameterLocation::Header)));
    Self { has_headers }
  }
}

impl ToTokens for HeaderParamsFragment {
  fn to_tokens(&self, tokens: &mut TokenStream) {
    if self.has_headers {
      tokens.extend(quote! {
        .headers(http::HeaderMap::try_from(&request.header)
          .context("building request headers")?)
      });
    }
  }
}

#[derive(Clone, Debug)]
pub(crate) struct XmlBodyFragment {
  field: FieldNameToken,
  optional: bool,
}

impl XmlBodyFragment {
  pub(crate) fn new(field: FieldNameToken, optional: bool) -> Self {
    Self { field, optional }
  }

  pub(crate) fn needs_conditional(&self) -> bool {
    self.optional
  }
}

impl ToTokens for XmlBodyFragment {
  fn to_tokens(&self, tokens: &mut TokenStream) {
    let field = &self.field;

    let ts = if self.optional {
      quote! {
        if let Some(body) = request.#field.as_ref() {
          let xml_string = body.to_string();
          req_builder = req_builder.header("Content-Type", "application/xml").body(xml_string);
        }
      }
    } else {
      quote! {
        .header("Content-Type", "application/xml")
        .body(request.#field.to_string())
      }
    };

    tokens.extend(ts);
  }
}

#[derive(Clone, Debug)]
enum BodyChainKind {
  Json,
  Form,
  Text,
  Binary,
}

#[derive(Clone, Debug)]
pub(crate) struct SimpleBodyFragment {
  field: FieldNameToken,
  optional: bool,
  kind: BodyChainKind,
  content_type: Option<String>,
}

impl SimpleBodyFragment {
  fn new(field: FieldNameToken, optional: bool, kind: BodyChainKind, content_type: Option<String>) -> Self {
    Self {
      field,
      optional,
      kind,
      content_type,
    }
  }

  /// The builder calls that send `expr` as the body, labelled with its declared media type.
  ///
  /// `json` and `form` only label a body that has no `Content-Type` yet, so a declared
  /// `+json` media type such as `application/merge-patch+json` is set first.
  fn make_chain(&self, expr: &TokenStream) -> TokenStream {
    let content_type = self.content_type.as_deref().map(|content_type| {
      quote! { .header(reqwest::header::CONTENT_TYPE, #content_type) }
    });
    match self.kind {
      BodyChainKind::Json => quote! { #content_type .json(#expr) },
      BodyChainKind::Form => quote! { .form(#expr) },
      BodyChainKind::Text => quote! { #content_type .body((#expr).to_string()) },
      BodyChainKind::Binary => quote! { #content_type .body((#expr).clone()) },
    }
  }

  pub(crate) fn needs_conditional(&self) -> bool {
    self.optional
  }
}

impl ToTokens for SimpleBodyFragment {
  fn to_tokens(&self, tokens: &mut TokenStream) {
    let field = &self.field;

    let ts = if self.optional {
      let chain = self.make_chain(&quote! { body });
      quote! {
        if let Some(body) = request.#field.as_ref() {
          req_builder = req_builder #chain;
        }
      }
    } else {
      self.make_chain(&quote! { &request.#field })
    };

    tokens.extend(ts);
  }
}

#[derive(Clone, Debug)]
pub(crate) enum RequestBodyFragment {
  None,
  Simple(SimpleBodyFragment),
  Xml(XmlBodyFragment),
  Multipart(MultipartFormFragment),
}

impl RequestBodyFragment {
  pub(crate) fn new(body: Option<&OperationBody>) -> Self {
    let Some(body) = body else {
      return Self::None;
    };

    let field = body.field_name.clone();
    let optional = body.optional;

    let declared = || Some(body.content_type.clone());
    match body.content_category {
      ContentCategory::Json => Self::Simple(SimpleBodyFragment::new(
        field,
        optional,
        BodyChainKind::Json,
        declared().filter(|content_type| content_type != DEFAULT_MEDIA_TYPE),
      )),
      ContentCategory::FormUrlEncoded => {
        Self::Simple(SimpleBodyFragment::new(field, optional, BodyChainKind::Form, None))
      }
      ContentCategory::Text | ContentCategory::EventStream => Self::Simple(SimpleBodyFragment::new(
        field,
        optional,
        BodyChainKind::Text,
        declared(),
      )),
      ContentCategory::Binary => Self::Simple(SimpleBodyFragment::new(
        field,
        optional,
        BodyChainKind::Binary,
        declared(),
      )),
      ContentCategory::Xml => Self::Xml(XmlBodyFragment::new(field, optional)),
      ContentCategory::Multipart => Self::Multipart(MultipartFormFragment::new(body.clone())),
    }
  }

  pub(crate) fn needs_conditional(&self) -> bool {
    match self {
      Self::None => false,
      Self::Simple(s) => s.needs_conditional(),
      Self::Xml(x) => x.needs_conditional(),
      Self::Multipart(_) => true,
    }
  }
}

impl ToTokens for RequestBodyFragment {
  fn to_tokens(&self, tokens: &mut TokenStream) {
    match self {
      Self::None => {}
      Self::Simple(s) => s.to_tokens(tokens),
      Self::Xml(x) => x.to_tokens(tokens),
      Self::Multipart(m) => m.to_tokens(tokens),
    }
  }
}

#[derive(Clone, Debug)]
pub(crate) struct UrlConstructionFragment {
  path: ParsedPath,
}

impl UrlConstructionFragment {
  pub(crate) fn new(path: ParsedPath) -> Self {
    Self { path }
  }
}

impl ToTokens for UrlConstructionFragment {
  fn to_tokens(&self, tokens: &mut TokenStream) {
    let segments = &self.path.segments;

    let query_setter = self.path.query_string.as_ref().map(|qs| {
      quote! { url.set_query(Some(#qs)); }
    });

    let ts = quote! {
      let mut url = self.base_url.clone();
      url.path_segments_mut()
         .map_err(|()| anyhow::anyhow!("URL cannot be a base"))?
         #(#segments)*;
      #query_setter
    };

    tokens.extend(ts);
  }
}

/// Passes the header and query API keys and the bearer tokens an operation accepts, in
/// declaration order, to the client's `set_security`, as a statement on `req_builder`.
/// Cookie keys live in the client's cookie store, which `reqwest` reads on every request.
#[derive(Clone, Copy, Debug)]
pub(crate) struct CredentialsFragment<'a> {
  security: &'a OperationSecurity,
}

impl<'a> CredentialsFragment<'a> {
  pub(crate) fn new(security: &'a OperationSecurity) -> Self {
    Self { security }
  }
}

impl ToTokens for CredentialsFragment<'_> {
  fn to_tokens(&self, tokens: &mut TokenStream) {
    let variants = self
      .security
      .schemes
      .iter()
      .filter(|scheme| !scheme.is_cookie())
      .map(security_variant);
    tokens.extend(quote! {
      req_builder = self.set_security(req_builder, &[#(Security::#variants),*]);
    });
  }
}

/// Emits the client's private `Security` enum, one variant per credential it stores
/// in a field, and the `set_security` method operations attach them with.
#[derive(Clone, Debug)]
pub(crate) struct SecurityFragment<'a> {
  schemes: Vec<&'a CredentialScheme>,
}

impl<'a> SecurityFragment<'a> {
  /// Returns `None` when the client stores no credential in a field.
  pub(crate) fn new(schemes: &[&'a CredentialScheme]) -> Option<Self> {
    let schemes = schemes
      .iter()
      .copied()
      .filter(|scheme| !scheme.is_cookie())
      .collect::<Vec<_>>();
    (!schemes.is_empty()).then_some(Self { schemes })
  }

  /// The `set_security` method, for the client's `impl` block.
  pub(crate) fn method(&self) -> TokenStream {
    let guarded = self.schemes.iter().filter(|scheme| scheme.uses_authorization()).count() > 1;
    let authorized = guarded.then(|| quote! { let mut authorized = false; });
    let arms = self.schemes.iter().filter_map(|scheme| {
      let variant = security_variant(scheme);
      let attach = attach_credential(scheme, guarded && scheme.uses_authorization())?;
      Some(quote! { Security::#variant => #attach })
    });
    quote! {
      /// Attaches the configured credentials among `schemes`, which an operation lists in
      /// declaration order, to `builder`. A request carries one `Authorization` header,
      /// so of the credentials that travel in it, the last one configured wins.
      fn set_security(&self, mut builder: reqwest::RequestBuilder, schemes: &[Security]) -> reqwest::RequestBuilder {
        #authorized
        for scheme in schemes.iter().rev() {
          match scheme {
            #(#arms)*
          }
        }
        builder
      }
    }
  }
}

impl ToTokens for SecurityFragment<'_> {
  fn to_tokens(&self, tokens: &mut TokenStream) {
    let variants = self.schemes.iter().map(|scheme| security_variant(scheme));
    tokens.extend(quote! {
      /// A credential the client attaches to the operations that accept it.
      #[allow(clippy::enum_variant_names)]
      enum Security {
        #(#variants),*
      }
    });
  }
}

/// Names `scheme`'s `Security` variant after its client field, e.g. `ApiKeyAuth` for
/// `api_key_auth`.
fn security_variant(scheme: &CredentialScheme) -> EnumVariantToken {
  EnumVariantToken::from_raw(scheme.field.as_str())
}

/// The `set_security` match arm body that attaches `scheme`'s credential to `builder`
/// when the client has one, or `None` for a cookie key. A `guarded` credential leaves
/// the `Authorization` header to one attached before it.
fn attach_credential(scheme: &CredentialScheme, guarded: bool) -> Option<TokenStream> {
  let field = &scheme.field;
  let value = credential_value(scheme);
  let secret = quote! { secrecy::ExposeSecret::expose_secret(#value) };
  let attach = match &scheme.kind {
    CredentialKind::ApiKey {
      location: ApiKeyLocation::Header,
      parameter_name,
    } => {
      let header = HttpHeaderRef::from(parameter_name).path();
      quote! { builder.header(#header, #value) }
    }
    CredentialKind::ApiKey {
      location: ApiKeyLocation::Query,
      parameter_name,
    } => quote! { builder.query(&[(#parameter_name, #secret)]) },
    CredentialKind::ApiKey {
      location: ApiKeyLocation::Cookie,
      ..
    } => return None,
    CredentialKind::Bearer => quote! { builder.bearer_auth(#secret) },
  };
  Some(if guarded {
    quote! {
      if !authorized && let Some(#value) = &self.#field {
        builder = #attach;
        authorized = true;
      },
    }
  } else {
    quote! {
      if let Some(#value) = &self.#field {
        builder = #attach;
      },
    }
  })
}

/// Names a credential's value in generated code: `token` for a bearer token and
/// `api_key` for an API key.
fn credential_value(scheme: &CredentialScheme) -> Ident {
  match scheme.kind {
    CredentialKind::ApiKey { .. } => format_ident!("api_key"),
    CredentialKind::Bearer => format_ident!("token"),
  }
}

#[derive(Clone)]
enum ResponseKind {
  Enum {
    response: OperationResponse,
    request_type: String,
  },
  Typed {
    resp_type: Box<syn::Type>,
    category: ContentCategory,
  },
  Raw,
}

impl std::fmt::Debug for ResponseKind {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    match self {
      Self::Enum { response, request_type } => f
        .debug_struct("Enum")
        .field("response", response)
        .field("request_type", request_type)
        .finish(),
      Self::Typed { category, .. } => f
        .debug_struct("Typed")
        .field("resp_type", &"<syn::Type>")
        .field("category", category)
        .finish(),
      Self::Raw => write!(f, "Raw"),
    }
  }
}

#[derive(Clone, Debug)]
pub(crate) struct ResponseParsingFragment {
  kind: ResponseKind,
}

impl ResponseParsingFragment {
  pub(crate) fn new(op: &OperationInfo) -> Self {
    if let Some(response) = &op.response {
      return Self {
        kind: ResponseKind::Enum {
          response: response.clone(),
          request_type: op.request_type.as_ref().unwrap().to_string(),
        },
      };
    }

    let Some(resp_type_str) = &op.response_type else {
      return Self {
        kind: ResponseKind::Raw,
      };
    };

    let Ok(resp_ty) = syn::parse_str::<syn::Type>(resp_type_str).context("parsing response type") else {
      return Self {
        kind: ResponseKind::Raw,
      };
    };

    let category = op
      .response_media_types
      .first()
      .map_or(ContentCategory::Json, |m| m.category);

    Self {
      kind: ResponseKind::Typed {
        resp_type: Box::new(resp_ty),
        category,
      },
    }
  }

  pub(crate) fn success_type(&self) -> TokenStream {
    match &self.kind {
      ResponseKind::Enum { response, .. } => quote! { #response },
      ResponseKind::Typed { resp_type, category } => match category {
        ContentCategory::Text => quote! { String },
        ContentCategory::EventStream => {
          quote! { oas3_gen_support::EventStream<oas3_gen_support::SseFrame<#resp_type>> }
        }
        ContentCategory::Json => quote! { #resp_type },
        _ => quote! { reqwest::Response },
      },
      ResponseKind::Raw => quote! { reqwest::Response },
    }
  }

  pub(crate) fn parse_body(&self) -> TokenStream {
    match &self.kind {
      ResponseKind::Enum { request_type, .. } => {
        let req_ident = format_ident!("{}", request_type);
        quote! { Ok(#req_ident::parse_response(response).await?) }
      }
      ResponseKind::Typed { resp_type, category } => match category {
        ContentCategory::Json => quote! { Ok(response.json::<#resp_type>().await?) },
        ContentCategory::Text => quote! { Ok(response.text().await?) },
        ContentCategory::EventStream => quote! { Ok(oas3_gen_support::EventStream::json(response, None)) },
        _ => quote! { Ok(response) },
      },
      ResponseKind::Raw => quote! { Ok(response) },
    }
  }
}

#[derive(Clone, Debug)]
pub(crate) struct ClientMethodFragment {
  op: OperationInfo,
  visibility: Visibility,
  has_cookie_store: bool,
}

impl ClientMethodFragment {
  /// Generates `op`'s method; `has_cookie_store` says the client struct keeps a
  /// cookie store, which cookie parameters merge with.
  pub(crate) fn new(op: OperationInfo, visibility: Visibility, has_cookie_store: bool) -> Self {
    Self {
      op,
      visibility,
      has_cookie_store,
    }
  }

  pub(crate) fn generate(&self) -> anyhow::Result<TokenStream> {
    let Some(request_ident) = self.op.request_type.as_ref().map(|r| format_ident!("{r}")) else {
      anyhow::bail!("operation `{}` is missing request type", self.op.operation_id);
    };

    let method_name = format_ident!("{}", self.op.stable_id);
    let doc_attrs = &self.op.documentation;
    let deprecated = generate_deprecated_attr(self.op.deprecated);

    let http_init = HttpInitFragment::new(self.op.method.clone());
    let url_construction = UrlConstructionFragment::new(self.op.path.clone());
    let query_params = QueryParamsFragment::new(&self.op.parameters);
    let header_chain = HeaderParamsFragment::new(&self.op.parameters);
    let body_fragment = RequestBodyFragment::new(self.op.body.as_ref());
    let credentials = self
      .op
      .security
      .as_ref()
      .filter(|security| security.schemes.iter().any(|scheme| !scheme.is_cookie()))
      .map(CredentialsFragment::new);
    let response_fragment = ResponseParsingFragment::new(&self.op);
    let cookie_header = CookieHeaderFragment::new(&self.op.parameters, self.has_cookie_store);
    let attach_cookies = cookie_header.is_some().then(CookieHeaderFragment::attach);

    let vis = self.visibility.to_tokens();
    let return_type = response_fragment.success_type();
    let parse_block = response_fragment.parse_body();

    let request_chain = if body_fragment.needs_conditional() || credentials.is_some() || cookie_header.is_some() {
      let (body_chain, body_statements) = if body_fragment.needs_conditional() {
        (None, Some(&body_fragment))
      } else {
        (Some(&body_fragment), None)
      };
      quote! {
        let mut req_builder = #http_init #header_chain #body_chain;
        #credentials
        #attach_cookies
        #body_statements
        let response = req_builder.send().await?;
      }
    } else {
      quote! {
        let response = #http_init #header_chain #body_fragment
          .send()
          .await?;
      }
    };

    Ok(quote! {
      #doc_attrs
      #deprecated
      #vis async fn #method_name(&self, request: #request_ident) -> anyhow::Result<#return_type> {
        request.validate().context("parameter validation")?;
        #url_construction
        #query_params
        #cookie_header
        #request_chain
        #parse_block
      }
    })
  }
}

#[derive(Clone, Debug)]
pub(crate) struct ClientStructFragment<'a> {
  name: StructToken,
  schemes: &'a [&'a CredentialScheme],
  visibility: Visibility,
}

impl<'a> ClientStructFragment<'a> {
  pub(crate) fn new(name: StructToken, schemes: &'a [&'a CredentialScheme], visibility: Visibility) -> Self {
    Self {
      name,
      schemes,
      visibility,
    }
  }
}

impl ToTokens for ClientStructFragment<'_> {
  fn to_tokens(&self, tokens: &mut TokenStream) {
    let name = &self.name;
    let vis = self.visibility.to_tokens();
    let stored = self
      .schemes
      .iter()
      .filter(|scheme| !scheme.is_cookie())
      .collect::<Vec<_>>();
    let credential_fields = stored.iter().map(|scheme| {
      let field = &scheme.field;
      let docs = Documentation::from_lines([format!(
        "`{}` {}, sent in {}.",
        scheme.scheme_name,
        scheme.kind.noun(),
        scheme.whereabouts()
      )]);
      let ty = if scheme.is_header_key() {
        quote! { http::HeaderValue }
      } else {
        quote! { secrecy::SecretString }
      };
      quote! {
        #docs
        #vis #field: Option<#ty>,
      }
    });

    let has_cookies = stored.len() < self.schemes.len();
    let derives = if has_cookies {
      quote! { #[derive(Clone)] }
    } else {
      quote! { #[derive(Debug, Clone)] }
    };
    let cookies_field = has_cookies.then(|| {
      quote! {
        /// Cookie API keys, which `client` sends with every request to the host of `base_url`.
        #vis cookies: std::sync::Arc<reqwest_cookie_store::CookieStoreMutex>,
      }
    });
    let debug_impl = has_cookies.then(|| {
      let label = name.to_string();
      let fields = stored.iter().map(|scheme| {
        let field = &scheme.field;
        let field_label = field.as_str().trim_start_matches("r#");
        quote! { .field(#field_label, &self.#field) }
      });
      quote! {
        impl core::fmt::Debug for #name {
          fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
            f.debug_struct(#label)
              .field("client", &self.client)
              .field("base_url", &self.base_url)
              #(#fields)*
              .finish_non_exhaustive()
          }
        }
      }
    });

    let ts = quote! {
      #derives
      #vis struct #name {
        #vis client: Client,
        #vis base_url: Url,
        #cookies_field
        #(#credential_fields)*
      }

      #debug_impl
    };

    tokens.extend(ts);
  }
}

#[derive(Clone, Debug)]
pub(crate) struct ClientDefaultImplFragment {
  name: StructToken,
}

impl ClientDefaultImplFragment {
  pub(crate) fn new(name: StructToken) -> Self {
    Self { name }
  }
}

impl ToTokens for ClientDefaultImplFragment {
  fn to_tokens(&self, tokens: &mut TokenStream) {
    let name = &self.name;

    let ts = quote! {
      impl Default for #name {
        fn default() -> Self {
          Self::new()
        }
      }
    };

    tokens.extend(ts);
  }
}

#[derive(Clone, Debug)]
pub(crate) struct ClientConstructorsFragment<'a> {
  schemes: &'a [&'a CredentialScheme],
  visibility: Visibility,
}

impl<'a> ClientConstructorsFragment<'a> {
  pub(crate) fn new(schemes: &'a [&'a CredentialScheme], visibility: Visibility) -> Self {
    Self { schemes, visibility }
  }
}

impl ToTokens for ClientConstructorsFragment<'_> {
  fn to_tokens(&self, tokens: &mut TokenStream) {
    let vis = self.visibility.to_tokens();
    let unset = self.schemes.iter().filter(|scheme| !scheme.is_cookie()).map(|scheme| {
      let field = &scheme.field;
      quote! { #field: None, }
    });
    let unset = quote! { #(#unset)* };
    let setters = self.schemes.iter().map(|scheme| {
      let field = &scheme.field;
      let setter = format_ident!("with_{}", field.as_str().trim_start_matches("r#"));
      if let CredentialKind::ApiKey {
        location: ApiKeyLocation::Cookie,
        parameter_name,
      } = &scheme.kind
      {
        let docs = Documentation::from_lines([
          format!(
            "Set the `{}` API key, sent in {} with every request to the host of `base_url`.",
            scheme.scheme_name,
            scheme.whereabouts()
          ),
          String::new(),
          "The key goes into `cookies`, which clones of this client share.".to_string(),
        ]);
        return quote! {
          #docs
          #[must_use]
          #vis fn #setter(self, api_key: impl Into<String>) -> Self {
            let cookie = reqwest_cookie_store::RawCookie::new(#parameter_name, api_key.into());
            let _ = self
              .cookies
              .lock()
              .unwrap_or_else(std::sync::PoisonError::into_inner)
              .insert_raw(&cookie, &self.base_url);
            self
          }
        };
      }
      let value = credential_value(scheme);
      let summary = format!(
        "Set the `{}` {}, sent in {} to operations that accept it.",
        scheme.scheme_name,
        scheme.kind.noun(),
        scheme.whereabouts()
      );
      if scheme.is_header_key() {
        let docs = Documentation::from_lines([
          summary,
          String::new(),
          "An empty key, or one that isn't a valid header value, leaves the key unset.".to_string(),
        ]);
        return quote! {
          #docs
          #[must_use]
          #vis fn #setter(mut self, #value: impl Into<String>) -> Self {
            let #value = #value.into();

            if #value.is_empty() {
              return self;
            }

            let Ok(mut header) = http::HeaderValue::from_str(&#value) else {
              return self;
            };
            header.set_sensitive(true);

            self.#field = Some(header);
            self
          }
        };
      }
      let docs = Documentation::from_lines([summary]);
      quote! {
        #docs
        #[must_use]
        #vis fn #setter(mut self, #value: impl Into<String>) -> Self {
          self.#field = Some(secrecy::SecretString::from(#value.into()));
          self
        }
      }
    });

    let has_cookies = self.schemes.iter().any(|scheme| scheme.is_cookie());
    let new_cookies = has_cookies.then(|| {
      quote! { let cookies = std::sync::Arc::new(reqwest_cookie_store::CookieStoreMutex::default()); }
    });
    let cookie_provider = has_cookies.then(|| quote! { .cookie_provider(std::sync::Arc::clone(&cookies)) });
    let cookies_field = has_cookies.then(|| quote! { cookies, });
    let empty_cookies = has_cookies.then(|| quote! { cookies: std::sync::Arc::default(), });
    let with_client_docs = has_cookies.then(|| {
      quote! {
        ///
        /// Cookie API keys go into a new `cookies` store, which `client` doesn't read. To
        /// send them, rebuild `client` with `cookie_provider(Arc::clone(&cookies))`.
      }
    });

    let ts = quote! {
      /// Create a client using the OpenAPI `servers[0]` URL.
      #[must_use]
      #[track_caller]
      #vis fn new() -> Self {
        #new_cookies
        Self {
          client: Client::builder() #cookie_provider .build().expect("client"),
          base_url: Url::parse(BASE_URL).expect("valid base url"),
          #cookies_field
          #unset
        }
      }

      /// Create a client with a custom base URL.
      #vis fn with_base_url(base_url: impl AsRef<str>) -> anyhow::Result<Self> {
        #new_cookies
        Ok(Self {
          client: Client::builder() #cookie_provider .build().context("building reqwest client")?,
          base_url: Url::parse(base_url.as_ref()).context("parsing base url")?,
          #cookies_field
          #unset
        })
      }

      /// Create a client from an existing `reqwest::Client`.
      #with_client_docs
      #vis fn with_client(base_url: impl AsRef<str>, client: Client) -> anyhow::Result<Self> {
        let url = Url::parse(base_url.as_ref()).context("parsing base url")?;
        Ok(Self { client, base_url: url, #empty_cookies #unset })
      }

      #(#setters)*
    };

    tokens.extend(ts);
  }
}

#[derive(Clone, Debug)]
pub struct ClientFragment {
  def: ClientRootNode,
  operations: Vec<OperationInfo>,
  visibility: Visibility,
  use_types_import: bool,
}

impl ClientFragment {
  pub fn new(def: &ClientRootNode, operations: &[OperationInfo], visibility: Visibility) -> Self {
    Self {
      def: def.clone(),
      operations: operations.to_vec(),
      visibility,
      use_types_import: false,
    }
  }

  pub fn with_types_import(mut self) -> Self {
    self.use_types_import = true;
    self
  }
}

impl ToTokens for ClientFragment {
  fn to_tokens(&self, tokens: &mut TokenStream) {
    let client_ident = &self.def.name;
    let vis = self.visibility.to_tokens();
    let base_url = LitStr::new(&self.def.base_url, Span::call_site());

    let types_import = if self.use_types_import {
      quote! { use super::types::*; }
    } else {
      quote! {}
    };

    let schemes = self
      .operations
      .iter()
      .filter(|op| op.kind == OperationKind::Http)
      .filter_map(|op| op.security.as_ref())
      .flat_map(|security| &security.schemes)
      .unique_by(|scheme| &scheme.field)
      .collect::<Vec<_>>();
    let has_cookie_store = schemes.iter().any(|scheme| scheme.is_cookie());

    let methods = self
      .operations
      .iter()
      .filter(|op| op.kind == OperationKind::Http)
      .filter_map(|op| {
        ClientMethodFragment::new(op.clone(), self.visibility, has_cookie_store)
          .generate()
          .ok()
      });

    let client_struct = ClientStructFragment::new(client_ident.clone(), &schemes, self.visibility);
    let default_impl = ClientDefaultImplFragment::new(client_ident.clone());
    let constructors = ClientConstructorsFragment::new(&schemes, self.visibility);
    let security = SecurityFragment::new(&schemes);
    let set_security = security.as_ref().map(SecurityFragment::method);

    quote! {
      use anyhow::Context;
      use reqwest::{Client, Url};
      use validator::Validate;

      #types_import

      #vis const BASE_URL: &str = #base_url;

      #client_struct

      #security

      #default_impl

      impl #client_ident {
        #constructors
        #set_security
        #(#methods)*
      }
    }
    .to_tokens(tokens);
  }
}

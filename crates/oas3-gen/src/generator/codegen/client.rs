use anyhow::Context as _;
use http::Method;
use itertools::Itertools;
use proc_macro2::{Span, TokenStream};
use quote::{ToTokens, format_ident, quote};
use syn::LitStr;

use super::Visibility;
use crate::generator::ast::{
  ApiKeyLocation, ApiKeyScheme, ClientRootNode, ContentCategory, Documentation, FieldDef, FieldNameToken,
  MultipartFieldInfo, OperationBody, OperationInfo, OperationKind, OperationResponse, OperationSecurity,
  ParameterLocation, ParsedPath, StructToken, tokens::ConstToken,
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
        let m = format_ident!("reqwest::Method::{}", self.method.as_str());
        quote! { self.client.request(#m, url) }
      }
    };
    tokens.extend(ts);
  }
}

#[derive(Clone, Debug)]
pub(crate) struct MultipartFieldFragment {
  field: MultipartFieldInfo,
}

impl MultipartFieldFragment {
  pub(crate) fn new(field: MultipartFieldInfo) -> Self {
    Self { field }
  }

  fn to_part(&self, value_expr: &TokenStream) -> TokenStream {
    if self.field.is_bytes {
      quote! { reqwest::multipart::Part::bytes(std::borrow::Cow::from(#value_expr.clone())) }
    } else if self.field.requires_json {
      quote! { reqwest::multipart::Part::text(serde_json::to_string(&#value_expr)?) }
    } else {
      quote! { reqwest::multipart::Part::text(#value_expr.to_string()) }
    }
  }
}

impl ToTokens for MultipartFieldFragment {
  fn to_tokens(&self, tokens: &mut TokenStream) {
    let ident = &self.field.name;
    let name = self.field.name.as_str();

    let ts = if self.field.nullable {
      let part = self.to_part(&quote! { val });
      quote! { if let Some(val) = &body.#ident { form = form.part(#name, #part); } }
    } else {
      let part = self.to_part(&quote! { body.#ident });
      quote! { form = form.part(#name, #part); }
    };

    tokens.extend(ts);
  }
}

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
      tokens.extend(quote! { .query(&request.query) });
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
pub(crate) struct MultipartFallbackFragment;

impl MultipartFallbackFragment {
  pub(crate) fn new() -> Self {
    Self
  }
}

impl ToTokens for MultipartFallbackFragment {
  fn to_tokens(&self, tokens: &mut TokenStream) {
    let ts = quote! {
      let json_value = serde_json::to_value(body)?;
      let mut form = reqwest::multipart::Form::new();
      if let serde_json::Value::Object(map) = json_value {
        for (key, value) in map {
          let text_value = match value {
            serde_json::Value::String(s) => s,
            serde_json::Value::Number(n) => n.to_string(),
            serde_json::Value::Bool(b) => b.to_string(),
            serde_json::Value::Null => continue,
            other => serde_json::to_string(&other)?,
          };
          form = form.text(key, text_value);
        }
      }
      req_builder = req_builder.multipart(form);
    };

    tokens.extend(ts);
  }
}

#[derive(Clone, Debug)]
pub(crate) struct MultipartStrictFragment {
  fields: Vec<MultipartFieldFragment>,
}

impl MultipartStrictFragment {
  pub(crate) fn new(fields: Vec<MultipartFieldInfo>) -> Self {
    let fragments = fields.into_iter().map(MultipartFieldFragment::new).collect();
    Self { fields: fragments }
  }
}

impl ToTokens for MultipartStrictFragment {
  fn to_tokens(&self, tokens: &mut TokenStream) {
    let parts = &self.fields;

    let ts = quote! {
      let mut form = reqwest::multipart::Form::new();
      #(#parts)*
      req_builder = req_builder.multipart(form);
    };

    tokens.extend(ts);
  }
}

#[derive(Clone, Debug)]
pub(crate) struct MultipartFormFragment {
  body: OperationBody,
}

impl MultipartFormFragment {
  pub(crate) fn new(body: OperationBody) -> Self {
    Self { body }
  }

  fn inner_logic(&self) -> TokenStream {
    self.body.multipart_fields.as_ref().map_or_else(
      || MultipartFallbackFragment::new().into_token_stream(),
      |f| MultipartStrictFragment::new(f.clone()).into_token_stream(),
    )
  }
}

impl ToTokens for MultipartFormFragment {
  fn to_tokens(&self, tokens: &mut TokenStream) {
    let logic = self.inner_logic();
    let field = &self.body.field_name;

    let ts = if self.body.optional {
      quote! {
        if let Some(body) = request.#field.as_ref() {
          #logic
        }
      }
    } else {
      quote! {
        let body = &request.#field;
        #logic
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
}

impl SimpleBodyFragment {
  fn json(field: FieldNameToken, optional: bool) -> Self {
    Self {
      field,
      optional,
      kind: BodyChainKind::Json,
    }
  }

  fn form(field: FieldNameToken, optional: bool) -> Self {
    Self {
      field,
      optional,
      kind: BodyChainKind::Form,
    }
  }

  fn text(field: FieldNameToken, optional: bool) -> Self {
    Self {
      field,
      optional,
      kind: BodyChainKind::Text,
    }
  }

  fn binary(field: FieldNameToken, optional: bool) -> Self {
    Self {
      field,
      optional,
      kind: BodyChainKind::Binary,
    }
  }

  fn make_chain(&self, expr: &TokenStream) -> TokenStream {
    match self.kind {
      BodyChainKind::Json => quote! { .json(#expr) },
      BodyChainKind::Form => quote! { .form(#expr) },
      BodyChainKind::Text => quote! { .body((#expr).to_string()) },
      BodyChainKind::Binary => quote! { .body((#expr).clone()) },
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

    match body.content_category {
      ContentCategory::Json => Self::Simple(SimpleBodyFragment::json(field, optional)),
      ContentCategory::FormUrlEncoded => Self::Simple(SimpleBodyFragment::form(field, optional)),
      ContentCategory::Text | ContentCategory::EventStream => Self::Simple(SimpleBodyFragment::text(field, optional)),
      ContentCategory::Binary => Self::Simple(SimpleBodyFragment::binary(field, optional)),
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

/// Attaches the header and query API keys configured on the client to an operation
/// that accepts them, as statements on `req_builder`. Cookie keys live in the client's
/// cookie store, which `reqwest` reads on every request.
///
/// A key that isn't configured is left off, and the server decides.
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
    for scheme in &self.security.schemes {
      let field = &scheme.field;
      let name = &scheme.parameter_name;
      let attach = match scheme.location {
        ApiKeyLocation::Header => {
          let header = ConstToken::from_raw(name);
          quote! { req_builder.header(#header, secrecy::ExposeSecret::expose_secret(api_key)) }
        }
        ApiKeyLocation::Query => quote! {
          req_builder.query(&[(#name, secrecy::ExposeSecret::expose_secret(api_key))])
        },
        ApiKeyLocation::Cookie => continue,
      };
      tokens.extend(quote! {
        if let Some(api_key) = &self.#field {
          req_builder = #attach;
        }
      });
    }
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
        ContentCategory::EventStream => quote! { oas3_gen_support::EventStream<#resp_type> },
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
        ContentCategory::EventStream => quote! { Ok(oas3_gen_support::EventStream::from_response(response)) },
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
}

impl ClientMethodFragment {
  pub(crate) fn new(op: OperationInfo, visibility: Visibility) -> Self {
    Self { op, visibility }
  }

  pub(crate) fn generate(&self) -> anyhow::Result<TokenStream> {
    let Some(request_ident) = self.op.request_type.as_ref().map(|r| format_ident!("{r}")) else {
      anyhow::bail!("operation `{}` is missing request type", self.op.operation_id);
    };

    let method_name = format_ident!("{}", self.op.stable_id);
    let doc_attrs = &self.op.documentation;

    let http_init = HttpInitFragment::new(self.op.method.clone());
    let url_construction = UrlConstructionFragment::new(self.op.path.clone());
    let query_chain = QueryParamsFragment::new(&self.op.parameters);
    let header_chain = HeaderParamsFragment::new(&self.op.parameters);
    let body_fragment = RequestBodyFragment::new(self.op.body.as_ref());
    let credentials = self
      .op
      .security
      .as_ref()
      .filter(|security| {
        security
          .schemes
          .iter()
          .any(|scheme| scheme.location != ApiKeyLocation::Cookie)
      })
      .map(CredentialsFragment::new);
    let response_fragment = ResponseParsingFragment::new(&self.op);

    let vis = self.visibility.to_tokens();
    let return_type = response_fragment.success_type();
    let parse_block = response_fragment.parse_body();

    let request_chain = if body_fragment.needs_conditional() || credentials.is_some() {
      let (body_chain, body_statements) = if body_fragment.needs_conditional() {
        (None, Some(&body_fragment))
      } else {
        (Some(&body_fragment), None)
      };
      quote! {
        let mut req_builder = #http_init #query_chain #header_chain #body_chain;
        #credentials
        #body_statements
        let response = req_builder.send().await?;
      }
    } else {
      quote! {
        let response = #http_init #query_chain #header_chain #body_fragment
          .send()
          .await?;
      }
    };

    Ok(quote! {
      #doc_attrs
      #vis async fn #method_name(&self, request: #request_ident) -> anyhow::Result<#return_type> {
        request.validate().context("parameter validation")?;
        #url_construction
        #request_chain
        #parse_block
      }
    })
  }
}

#[derive(Clone, Debug)]
pub(crate) struct ClientStructFragment<'a> {
  name: StructToken,
  api_keys: &'a [&'a ApiKeyScheme],
  visibility: Visibility,
}

impl<'a> ClientStructFragment<'a> {
  pub(crate) fn new(name: StructToken, api_keys: &'a [&'a ApiKeyScheme], visibility: Visibility) -> Self {
    Self {
      name,
      api_keys,
      visibility,
    }
  }
}

impl ToTokens for ClientStructFragment<'_> {
  fn to_tokens(&self, tokens: &mut TokenStream) {
    let name = &self.name;
    let vis = self.visibility.to_tokens();
    let api_keys = self
      .api_keys
      .iter()
      .filter(|scheme| scheme.location != ApiKeyLocation::Cookie)
      .collect::<Vec<_>>();
    let api_key_fields = api_keys.iter().map(|scheme| {
      let field = &scheme.field;
      let docs = Documentation::from_lines([format!(
        "`{}` API key, sent in {}.",
        scheme.scheme_name,
        scheme.whereabouts()
      )]);
      quote! {
        #docs
        #vis #field: Option<secrecy::SecretString>,
      }
    });

    let has_cookies = api_keys.len() < self.api_keys.len();
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
      let fields = api_keys.iter().map(|scheme| {
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
        #(#api_key_fields)*
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
  api_keys: &'a [&'a ApiKeyScheme],
  visibility: Visibility,
}

impl<'a> ClientConstructorsFragment<'a> {
  pub(crate) fn new(api_keys: &'a [&'a ApiKeyScheme], visibility: Visibility) -> Self {
    Self { api_keys, visibility }
  }
}

impl ToTokens for ClientConstructorsFragment<'_> {
  fn to_tokens(&self, tokens: &mut TokenStream) {
    let vis = self.visibility.to_tokens();
    let unset = self
      .api_keys
      .iter()
      .filter(|scheme| scheme.location != ApiKeyLocation::Cookie)
      .map(|scheme| {
        let field = &scheme.field;
        quote! { #field: None, }
      });
    let unset = quote! { #(#unset)* };
    let setters = self.api_keys.iter().map(|scheme| {
      let field = &scheme.field;
      let setter = format_ident!("with_{}", field.as_str().trim_start_matches("r#"));
      if scheme.location == ApiKeyLocation::Cookie {
        let name = &scheme.parameter_name;
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
            let cookie = reqwest_cookie_store::RawCookie::new(#name, api_key.into());
            let _ = self
              .cookies
              .lock()
              .unwrap_or_else(std::sync::PoisonError::into_inner)
              .insert_raw(&cookie, &self.base_url);
            self
          }
        };
      }
      let docs = Documentation::from_lines([format!(
        "Set the `{}` API key, sent in {} to operations that accept it.",
        scheme.scheme_name,
        scheme.whereabouts()
      )]);
      quote! {
        #docs
        #[must_use]
        #vis fn #setter(mut self, api_key: impl Into<String>) -> Self {
          self.#field = Some(secrecy::SecretString::from(api_key.into()));
          self
        }
      }
    });

    let has_cookies = self
      .api_keys
      .iter()
      .any(|scheme| scheme.location == ApiKeyLocation::Cookie);
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

    let methods = self
      .operations
      .iter()
      .filter(|op| op.kind == OperationKind::Http)
      .filter_map(|op| ClientMethodFragment::new(op.clone(), self.visibility).generate().ok());

    let types_import = if self.use_types_import {
      quote! { use super::types::*; }
    } else {
      quote! {}
    };

    let api_keys = self
      .operations
      .iter()
      .filter(|op| op.kind == OperationKind::Http)
      .filter_map(|op| op.security.as_ref())
      .flat_map(|security| &security.schemes)
      .unique_by(|scheme| &scheme.field)
      .collect::<Vec<_>>();

    let client_struct = ClientStructFragment::new(client_ident.clone(), &api_keys, self.visibility);
    let default_impl = ClientDefaultImplFragment::new(client_ident.clone());
    let constructors = ClientConstructorsFragment::new(&api_keys, self.visibility);

    quote! {
      use anyhow::Context;
      use reqwest::{Client, Url};
      use validator::Validate;

      #types_import

      #vis const BASE_URL: &str = #base_url;

      #client_struct

      #default_impl

      impl #client_ident {
        #constructors
        #(#methods)*
      }
    }
    .to_tokens(tokens);
  }
}

use anyhow::Context as _;
use http::Method;
use proc_macro2::{Span, TokenStream};
use quote::{ToTokens, format_ident, quote};
use syn::LitStr;

use super::Visibility;
use crate::generator::ast::{
  ClientDef, ContentCategory, FieldNameToken, MultipartFieldInfo, OperationBody, OperationInfo, OperationKind,
  ParameterLocation, StructToken, TypeRef,
};

pub(super) struct BodyResult {
  pub tokens: TokenStream,
  pub needs_conditional: bool,
}

struct ResponseHandling {
  success_type: TokenStream,
  parse_body: TokenStream,
}

pub struct ClientGenerator<'a> {
  def: &'a ClientDef,
  operations: &'a [OperationInfo],
  visibility: Visibility,
  use_types_import: bool,
}

impl<'a> ClientGenerator<'a> {
  pub fn new(def: &'a ClientDef, operations: &'a [OperationInfo], visibility: Visibility) -> Self {
    Self {
      def,
      operations,
      visibility,
      use_types_import: false,
    }
  }

  pub fn with_types_import(mut self) -> Self {
    self.use_types_import = true;
    self
  }

  fn client_struct(&self, client_ident: &StructToken) -> TokenStream {
    let vis = self.visibility.to_tokens();
    quote! {
      #[derive(Debug, Clone)]
      #vis struct #client_ident {
        #vis client: Client,
        #vis base_url: Url,
      }
    }
  }

  fn constructors(&self) -> TokenStream {
    let vis = self.visibility.to_tokens();
    quote! {
      /// Create a client using the OpenAPI `servers[0]` URL.
      #[must_use]
      #[track_caller]
      #vis fn new() -> Self {
        Self {
          client: Client::builder().build().expect("client"),
          base_url: Url::parse(BASE_URL).expect("valid base url"),
        }
      }

      /// Create a client with a custom base URL.
      #vis fn with_base_url(base_url: impl AsRef<str>) -> anyhow::Result<Self> {
        Ok(Self {
          client: Client::builder().build().context("building reqwest client")?,
          base_url: Url::parse(base_url.as_ref()).context("parsing base url")?,
        })
      }

      /// Create a client from an existing `reqwest::Client`.
      #vis fn with_client(base_url: impl AsRef<str>, client: Client) -> anyhow::Result<Self> {
        let url = Url::parse(base_url.as_ref()).context("parsing base url")?;
        Ok(Self { client, base_url: url })
      }
    }
  }
}

impl ToTokens for ClientGenerator<'_> {
  fn to_tokens(&self, tokens: &mut TokenStream) {
    let client_ident = &self.def.name;
    let vis = self.visibility.to_tokens();
    let base_url = LitStr::new(&self.def.base_url, Span::call_site());

    let methods = self
      .operations
      .iter()
      .filter(|op| op.kind == OperationKind::Http)
      .filter_map(|op| generate_method(op, self.visibility).ok());

    let types_import = if self.use_types_import {
      quote! { use super::types::*; }
    } else {
      quote! {}
    };

    let client_struct = self.client_struct(client_ident);
    let constructors = self.constructors();

    quote! {
      use anyhow::Context;
      use reqwest::{Client, Url};
      use validator::Validate;

      #types_import

      #vis const BASE_URL: &str = #base_url;

      #client_struct

      impl Default for #client_ident {
        fn default() -> Self {
          Self::new()
        }
      }

      impl #client_ident {
        #constructors
        #(#methods)*
      }
    }
    .to_tokens(tokens);
  }
}

pub(crate) fn generate_method(op: &OperationInfo, visibility: Visibility) -> anyhow::Result<TokenStream> {
  let Some(request_ident) = op.request_type.as_ref().map(|r| format_ident!("{r}")) else {
    anyhow::bail!("operation `{}` is missing request type", op.operation_id);
  };

  let method_name = format_ident!("{}", op.stable_id);
  let doc_attrs = &op.documentation;
  let builder_init = generate_http_init(&op.method);
  let url_construction = generate_url_construction(op);
  let query_chain = generate_query_params(op);
  let header_chain = generate_header_params(op);
  let body_result = generate_body(op);
  let response_logic = generate_response(op);

  let vis = visibility.to_tokens();
  let return_type = &response_logic.success_type;
  let parse_block = &response_logic.parse_body;

  let request_chain = if body_result.needs_conditional {
    let body_logic = &body_result.tokens;
    quote! {
      let mut req_builder = #builder_init #query_chain #header_chain;
      #body_logic
      let response = req_builder.send().await?;
    }
  } else {
    let body_chain = &body_result.tokens;
    quote! {
      let response = #builder_init #query_chain #header_chain #body_chain
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

fn generate_http_init(method: &Method) -> TokenStream {
  match *method {
    Method::GET => quote! { self.client.get(url) },
    Method::POST => quote! { self.client.post(url) },
    Method::PUT => quote! { self.client.put(url) },
    Method::DELETE => quote! { self.client.delete(url) },
    Method::PATCH => quote! { self.client.patch(url) },
    Method::HEAD => quote! { self.client.head(url) },
    _ => {
      let m = format_ident!("reqwest::Method::{}", method.as_str());
      quote! { self.client.request(#m, url) }
    }
  }
}

fn generate_url_construction(op: &OperationInfo) -> TokenStream {
  let segments = &op.path.0;
  quote! {
    let mut url = self.base_url.clone();
    url.path_segments_mut()
       .map_err(|()| anyhow::anyhow!("URL cannot be a base"))?
       #(#segments)*;
  }
}

fn generate_query_params(op: &OperationInfo) -> TokenStream {
  let has_query = op
    .parameters
    .iter()
    .any(|p| matches!(p.parameter_location, Some(ParameterLocation::Query)));
  if has_query {
    quote! { .query(&request.query) }
  } else {
    quote! {}
  }
}

fn generate_header_params(op: &OperationInfo) -> TokenStream {
  let has_headers = op
    .parameters
    .iter()
    .any(|p| matches!(p.parameter_location, Some(ParameterLocation::Header)));
  if has_headers {
    quote! {
      .headers(http::HeaderMap::try_from(&request.header)
        .context("building request headers")?)
    }
  } else {
    quote! {}
  }
}

fn generate_body(op: &OperationInfo) -> BodyResult {
  let Some(body) = &op.body else {
    return BodyResult {
      tokens: quote! {},
      needs_conditional: false,
    };
  };
  let field = &body.field_name;

  match body.content_category {
    ContentCategory::Json => chain_or_conditional(field, body.optional, |e| quote! { .json(#e) }),
    ContentCategory::FormUrlEncoded => chain_or_conditional(field, body.optional, |e| quote! { .form(#e) }),
    ContentCategory::Text | ContentCategory::EventStream => {
      chain_or_conditional(field, body.optional, |e| quote! { .body((#e).to_string()) })
    }
    ContentCategory::Binary => chain_or_conditional(field, body.optional, |e| quote! { .body((#e).clone()) }),
    ContentCategory::Xml => generate_xml_body(field, body.optional),
    ContentCategory::Multipart => generate_multipart(body),
  }
}

fn chain_or_conditional<F>(field: &FieldNameToken, optional: bool, make_chain: F) -> BodyResult
where
  F: FnOnce(TokenStream) -> TokenStream,
{
  if optional {
    let chain = make_chain(quote! { body });
    BodyResult {
      tokens: quote! {
        if let Some(body) = request.#field.as_ref() {
          req_builder = req_builder #chain;
        }
      },
      needs_conditional: true,
    }
  } else {
    BodyResult {
      tokens: make_chain(quote! { &request.#field }),
      needs_conditional: false,
    }
  }
}

fn generate_xml_body(field: &FieldNameToken, optional: bool) -> BodyResult {
  if optional {
    BodyResult {
      tokens: quote! {
        if let Some(body) = request.#field.as_ref() {
          let xml_string = body.to_string();
          req_builder = req_builder.header("Content-Type", "application/xml").body(xml_string);
        }
      },
      needs_conditional: true,
    }
  } else {
    BodyResult {
      tokens: quote! {
        .header("Content-Type", "application/xml")
        .body(request.#field.to_string())
      },
      needs_conditional: false,
    }
  }
}

pub(super) fn generate_multipart(body: &OperationBody) -> BodyResult {
  let logic = body
    .multipart_fields
    .as_ref()
    .map_or_else(|| multipart_fallback(body.body_type.as_ref()), |f| multipart_strict(f));
  let field = &body.field_name;

  let tokens = if body.optional {
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

  BodyResult {
    tokens,
    needs_conditional: true,
  }
}

fn multipart_strict(fields: &[MultipartFieldInfo]) -> TokenStream {
  let parts = fields.iter().map(|f| {
    let ident = &f.name;
    let name = f.name.as_str();

    let to_part = |v: TokenStream| {
      if f.is_bytes {
        quote! { reqwest::multipart::Part::bytes(std::borrow::Cow::from(#v.clone())) }
      } else if f.requires_json {
        quote! { reqwest::multipart::Part::text(serde_json::to_string(&#v)?) }
      } else {
        quote! { reqwest::multipart::Part::text(#v.to_string()) }
      }
    };

    if f.nullable {
      let part = to_part(quote! { val });
      quote! { if let Some(val) = &body.#ident { form = form.part(#name, #part); } }
    } else {
      let part = to_part(quote! { body.#ident });
      quote! { form = form.part(#name, #part); }
    }
  });

  quote! {
    let mut form = reqwest::multipart::Form::new();
    #(#parts)*
    req_builder = req_builder.multipart(form);
  }
}

fn multipart_fallback(body_type: Option<&TypeRef>) -> TokenStream {
  let type_annotation = body_type.map_or_else(
    || quote! {},
    |ty| {
      let ty_tokens = ty.to_token_stream();
      quote! { ::<#ty_tokens> }
    },
  );

  quote! {
    let json_value = serde_json::to_value #type_annotation (body)?;
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
  }
}

fn generate_response(op: &OperationInfo) -> ResponseHandling {
  if let Some(enum_token) = &op.response_enum {
    let req_ident = format_ident!("{}", op.request_type.as_ref().unwrap());
    return ResponseHandling {
      success_type: quote! { #enum_token },
      parse_body: quote! { Ok(#req_ident::parse_response(response).await?) },
    };
  }

  let Some(resp_type_str) = &op.response_type else {
    return raw_response();
  };

  let Ok(resp_ty) = syn::parse_str::<syn::Type>(resp_type_str).context("parsing response type") else {
    return raw_response();
  };

  let category = op
    .response_media_types
    .first()
    .map_or(ContentCategory::Json, |m| m.category);

  match category {
    ContentCategory::Json => ResponseHandling {
      success_type: quote! { #resp_ty },
      parse_body: quote! { Ok(response.json::<#resp_ty>().await?) },
    },
    ContentCategory::Text => ResponseHandling {
      success_type: quote! { String },
      parse_body: quote! { Ok(response.text().await?) },
    },
    ContentCategory::EventStream => ResponseHandling {
      success_type: quote! { oas3_gen_support::EventStream<#resp_ty> },
      parse_body: quote! { Ok(oas3_gen_support::EventStream::from_response(response)) },
    },
    _ => raw_response(),
  }
}

fn raw_response() -> ResponseHandling {
  ResponseHandling {
    success_type: quote! { reqwest::Response },
    parse_body: quote! { Ok(response) },
  }
}

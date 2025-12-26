use std::collections::BTreeSet;

use http::Method;
use proc_macro2::{Span, TokenStream};
use quote::{ToTokens, format_ident, quote};
use syn::LitStr;

use super::{Visibility, metadata::CodeMetadata};
use crate::generator::{
  ast::{
    ContentCategory, Documentation, FieldNameToken, OperationInfo, OperationKind, ParameterLocation, RustPrimitive,
    RustType, StructDef, StructToken, TypeRef,
    tokens::{ConstToken, HeaderToken},
  },
  codegen::{constants, parse_type},
  naming::identifiers::to_rust_type_name,
};

/// Generates the API Client struct and its methods.
pub struct ClientGenerator<'a> {
  metadata: &'a CodeMetadata,
  operations: &'a [OperationInfo],
  rust_types: &'a [RustType],
  visibility: Visibility,
  use_types_import: bool,
}

impl<'a> ClientGenerator<'a> {
  pub fn new(
    metadata: &'a CodeMetadata,
    operations: &'a [OperationInfo],
    rust_types: &'a [RustType],
    visibility: Visibility,
  ) -> Self {
    Self {
      metadata,
      operations,
      rust_types,
      visibility,
      use_types_import: false,
    }
  }

  pub fn with_types_import(mut self) -> Self {
    self.use_types_import = true;
    self
  }

  pub fn client_ident(&self) -> syn::Ident {
    let client_name = if self.metadata.title.is_empty() {
      "Api".to_string()
    } else {
      to_rust_type_name(&self.metadata.title)
    };
    format_ident!("{client_name}Client")
  }

  fn generate_client_struct(&self, client_ident: &syn::Ident) -> TokenStream {
    let vis = self.visibility.to_tokens();
    quote! {
      #[derive(Debug, Clone)]
      #vis struct #client_ident {
        #vis client: Client,
        #vis base_url: Url,
      }
    }
  }

  fn generate_constructors(&self) -> TokenStream {
    let vis = self.visibility.to_tokens();
    quote! {
      /// Create a client using the OpenAPI `servers[0]` URL.
      #[must_use]
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
    let client_ident = self.client_ident();
    let vis = self.visibility.to_tokens();
    let base_url = LitStr::new(&self.metadata.base_url, Span::call_site());

    let header_consts = generate_header_constants(self.operations);
    let methods = self
      .operations
      .iter()
      .filter(|op| op.kind == OperationKind::Http)
      .filter_map(|op| ClientOperationMethod::generate(op, self.rust_types, self.visibility).ok());

    let types_import = if self.use_types_import {
      quote! { use super::types::*; }
    } else {
      quote! {}
    };

    let client_struct = self.generate_client_struct(&client_ident);
    let constructors = self.generate_constructors();

    quote! {
      use anyhow::Context;
      use reqwest::{Client, Url};
      #[allow(unused_imports)]
      use reqwest::multipart::{Form, Part};
      #[allow(unused_imports)]
      use reqwest::header::HeaderValue;
      use validator::Validate;

      #types_import

      #vis const BASE_URL: &str = #base_url;

      #header_consts

      #client_struct

      impl #client_ident {
        #constructors
        #(#methods)*
      }
    }
    .to_tokens(tokens);
  }
}

fn generate_header_constants(operations: &[OperationInfo]) -> TokenStream {
  let headers: Vec<HeaderToken> = operations
    .iter()
    .filter(|op| op.kind == OperationKind::Http)
    .flat_map(|op| &op.parameters)
    .filter(|param| matches!(param.location, ParameterLocation::Header))
    .map(|param| HeaderToken::from(param.original_name.as_str()))
    .collect::<BTreeSet<_>>()
    .into_iter()
    .collect();

  constants::generate_header_constants(&headers)
}

pub(crate) struct ClientOperationMethod;

impl ClientOperationMethod {
  pub(crate) fn generate(
    op: &OperationInfo,
    rust_types: &[RustType],
    visibility: Visibility,
  ) -> anyhow::Result<TokenStream> {
    let Some(request_ident) = op.request_type.as_ref().map(|r| format_ident!("{r}")) else {
      anyhow::bail!("operation `{}` is missing request type", op.operation_id);
    };

    let method_name = format_ident!("{}", op.stable_id);
    let doc_attrs = build_doc_attributes(op);
    let builder_init = build_http_method_init(&op.method);
    let url_construction = build_url_construction(op);

    // Body and Parameter Logic
    let query_params = build_query_params(op);
    let header_params = build_header_params(op);
    let body_logic = build_body_statement(op, rust_types);

    let param_logic = quote! {
      #query_params
      #header_params
      #body_logic
    };

    // Response Handling
    let response_logic = build_response_handling(op);

    let vis = visibility.to_tokens();

    // Determine return type from response logic
    let return_type = &response_logic.success_type;
    let parse_block = &response_logic.parse_body;

    Ok(quote! {
      #doc_attrs
      #vis async fn #method_name(&self, request: #request_ident) -> anyhow::Result<#return_type> {
        request.validate().context("parameter validation")?;
        #url_construction
        let mut req_builder = #builder_init;
        #param_logic
        let response = req_builder.send().await?;
        #parse_block
      }
    })
  }
}

pub(crate) fn build_doc_attributes(op: &OperationInfo) -> Documentation {
  let mut docs = Documentation::default();

  if let Some(summary) = &op.summary {
    for line in summary.lines().filter(|l| !l.trim().is_empty()) {
      docs.push(line.trim().to_string());
    }
  }

  if let Some(desc) = &op.description {
    if op.summary.is_some() {
      docs.push(String::new());
    }
    for line in desc.lines() {
      docs.push(line.trim().to_string());
    }
  }

  if op.summary.is_some() || op.description.is_some() {
    docs.push(String::new());
  }

  docs.push(format!("{} {}", op.method.as_str(), op.path_template));
  docs
}

fn build_http_method_init(method: &Method) -> TokenStream {
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

fn build_url_construction(op: &OperationInfo) -> TokenStream {
  let segments = &op.path.0;
  quote! {
    let mut url = self.base_url.clone();
    url.path_segments_mut()
       .map_err(|()| anyhow::anyhow!("URL cannot be a base"))?
       #(#segments)*;
  }
}

fn build_query_params(op: &OperationInfo) -> TokenStream {
  if op
    .parameters
    .iter()
    .any(|p| matches!(p.location, ParameterLocation::Query))
  {
    quote! { req_builder = req_builder.query(&request.query); }
  } else {
    quote! {}
  }
}

fn build_header_params(op: &OperationInfo) -> TokenStream {
  let statements = op
    .parameters
    .iter()
    .filter(|p| matches!(p.location, ParameterLocation::Header))
    .map(|p| {
      let header_name = ConstToken::from_raw(p.original_name.as_str());
      let field = &p.rust_field;

      let conversion = build_header_value_conversion(&p.rust_type, field, p.required);

      if p.required {
        quote! {
          {
            #conversion
            req_builder = req_builder.header(#header_name, header_value);
          }
        }
      } else {
        quote! {
          if let Some(value) = request.header.#field.as_ref() {
            #conversion
            req_builder = req_builder.header(#header_name, header_value);
          }
        }
      }
    });

  quote! { #(#statements)* }
}

fn build_header_value_conversion(ty: &TypeRef, field: &FieldNameToken, required: bool) -> TokenStream {
  let val = if required {
    quote! { request.header.#field }
  } else {
    quote! { value }
  };

  if ty.is_string_like() {
    quote! { let header_value = HeaderValue::from_str(#val.as_str())?; }
  } else if ty.is_primitive_type() {
    quote! { let header_value = HeaderValue::from_str(&(#val).to_string())?; }
  } else {
    quote! { let header_value = HeaderValue::from_str(&serde_plain::to_string(&#val)?)?; }
  }
}

fn build_body_statement(op: &OperationInfo, rust_types: &[RustType]) -> TokenStream {
  let Some(body) = &op.body else {
    return quote! {};
  };
  let field = &body.field_name;

  match body.content_category {
    ContentCategory::Json => wrap_body(field, body.optional, |e| quote! { req_builder = req_builder.json(#e); }),
    ContentCategory::FormUrlEncoded => {
      wrap_body(field, body.optional, |e| quote! { req_builder = req_builder.form(#e); })
    }
    ContentCategory::Text | ContentCategory::EventStream => wrap_body(
      field,
      body.optional,
      |e| quote! { req_builder = req_builder.body((#e).to_string()); },
    ),
    ContentCategory::Binary => wrap_body(
      field,
      body.optional,
      |e| quote! { req_builder = req_builder.body((#e).clone()); },
    ),
    ContentCategory::Xml => wrap_body(field, body.optional, |e| {
      quote! {
        let xml_string = (#e).to_string();
        req_builder = req_builder.header("Content-Type", "application/xml").body(xml_string);
      }
    }),
    ContentCategory::Multipart => build_multipart_body(field, body.optional, op, rust_types),
  }
}

fn wrap_body<F>(field: &FieldNameToken, optional: bool, f: F) -> TokenStream
where
  F: FnOnce(TokenStream) -> TokenStream,
{
  if optional {
    let stmt = f(quote! { body });
    quote! {
      if let Some(body) = request.#field.as_ref() {
        #stmt
      }
    }
  } else {
    f(quote! { &request.#field })
  }
}

// --- Multipart ---

pub(crate) fn build_multipart_body(
  field: &FieldNameToken,
  optional: bool,
  op: &OperationInfo,
  rust_types: &[RustType],
) -> TokenStream {
  let logic =
    resolve_multipart_struct(op, rust_types, field).map_or_else(generate_fallback_multipart, generate_strict_multipart);

  if optional {
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
  }
}

fn resolve_multipart_struct<'a>(
  op: &OperationInfo,
  types: &'a [RustType],
  field: &FieldNameToken,
) -> Option<&'a StructDef> {
  let req_type = op.request_type.as_ref()?;
  let req_struct = find_struct(req_type, types)?;
  let field_def = req_struct.fields.iter().find(|f| *field == f.name.as_str())?;

  if let RustPrimitive::Custom(name) = &field_def.rust_type.base_type {
    find_struct(&StructToken::from(name.clone()), types)
  } else {
    None
  }
}

fn find_struct<'a>(name: &StructToken, types: &'a [RustType]) -> Option<&'a StructDef> {
  types.iter().find_map(|t| match t {
    RustType::Struct(s) if &s.name == name => Some(s),
    _ => None,
  })
}

fn generate_strict_multipart(def: &StructDef) -> TokenStream {
  let parts = def.fields.iter().map(|f| {
    let ident = format_ident!("{}", f.name);
    let name = f.name.as_str();
    let is_bytes = matches!(f.rust_type.base_type, RustPrimitive::Bytes);

    let to_part = |v: TokenStream| {
      if is_bytes {
        quote! { Part::bytes(std::borrow::Cow::from(#v.clone())) }
      } else {
        quote! { Part::text(#v.to_string()) }
      }
    };

    if f.rust_type.nullable {
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

fn generate_fallback_multipart() -> TokenStream {
  quote! {
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
  }
}

pub(crate) struct ResponseHandling {
  pub(crate) success_type: TokenStream,
  pub(crate) parse_body: TokenStream,
}

fn build_response_handling(op: &OperationInfo) -> ResponseHandling {
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

  let Ok(resp_ty) = parse_type(resp_type_str) else {
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

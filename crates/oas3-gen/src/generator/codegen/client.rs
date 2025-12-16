use std::collections::BTreeSet;

use http::Method;
use proc_macro2::{Span, TokenStream};
use quote::{ToTokens, format_ident, quote};
use syn::LitStr;

use super::{Visibility, attributes::generate_docs, metadata::CodeMetadata};
use crate::generator::{
  ast::{
    ContentCategory, EnumToken, FieldDef, FieldNameToken, OperationBody, OperationInfo, OperationKind,
    ParameterLocation, RustPrimitive, RustType, StructDef, StructToken, TypeRef,
    tokens::{ConstToken, HeaderToken},
  },
  codegen::{constants, parse_type},
  naming::identifiers::to_rust_type_name,
};

pub struct ClientGenerator<'a> {
  metadata: &'a CodeMetadata,
  operations: &'a [OperationInfo],
  rust_types: &'a [RustType],
  visibility: Visibility,
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
    }
  }

  fn client_ident(&self) -> syn::Ident {
    let client_name = if self.metadata.title.is_empty() {
      "Api".to_string()
    } else {
      to_rust_type_name(&self.metadata.title)
    };
    format_ident!("{client_name}Client")
  }

  fn base_url_lit(&self) -> LitStr {
    LitStr::new(&self.metadata.base_url, Span::call_site())
  }

  fn header_consts(&self) -> TokenStream {
    let headers: Vec<HeaderToken> = extract_header_names(self.operations, OperationKind::Http)
      .into_iter()
      .collect();
    constants::generate_header_constants(&headers)
  }

  fn method_tokens(&self) -> anyhow::Result<Vec<TokenStream>> {
    self
      .operations
      .iter()
      .filter(|op| op.kind == OperationKind::Http)
      .map(|op| {
        ClientOperationMethod::try_from_operation(op, self.rust_types, self.visibility)
          .map(quote::ToTokens::into_token_stream)
      })
      .collect()
  }
}

impl ToTokens for ClientGenerator<'_> {
  fn to_tokens(&self, tokens: &mut TokenStream) {
    let client_ident = self.client_ident();
    let base_url_lit = self.base_url_lit();
    let header_consts = self.header_consts();
    let vis = self.visibility.to_tokens();

    let Ok(method_tokens) = self.method_tokens() else {
      return;
    };

    quote! {
      use anyhow::Context;
      use reqwest::{Client, Url};
      #[allow(unused_imports)]
      use reqwest::multipart::{Form, Part};
      #[allow(unused_imports)]
      use reqwest::header::HeaderValue;
      use validator::Validate;

      #vis const BASE_URL: &str = #base_url_lit;

      #header_consts

      #[derive(Debug, Clone)]
      #vis struct #client_ident {
        #vis client: Client,
        #vis base_url: Url,
      }

      impl #client_ident {
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

        #(#method_tokens)*
      }
    }
    .to_tokens(tokens);
  }
}

fn extract_header_names(operations: &[OperationInfo], filter_kind: OperationKind) -> BTreeSet<HeaderToken> {
  operations
    .iter()
    .filter(|op| op.kind == filter_kind)
    .flat_map(|op| &op.parameters)
    .filter(|param| matches!(param.location, ParameterLocation::Header))
    .map(|param| HeaderToken::from(param.original_name.as_str()))
    .collect()
}

pub(crate) struct ResponseHandling {
  pub(crate) success_type: TokenStream,
  pub(crate) parse_body: TokenStream,
}

pub(crate) struct ClientOperationMethod {
  pub(crate) method_name: syn::Ident,
  pub(crate) request_ident: syn::Ident,
  pub(crate) doc_attrs: TokenStream,
  pub(crate) builder_init: TokenStream,
  pub(crate) header_statements: Vec<TokenStream>,
  pub(crate) body_statement: TokenStream,
  pub(crate) response_handling: ResponseHandling,
  pub(crate) visibility: Visibility,
}

impl ClientOperationMethod {
  pub(crate) fn try_from_operation(
    operation: &OperationInfo,
    rust_types: &[RustType],
    visibility: Visibility,
  ) -> anyhow::Result<Self> {
    let Some(request_ident) = operation.request_type.as_ref().map(|r| format_ident!("{r}")) else {
      anyhow::bail!(
        "operation `{}` is missing request type information",
        operation.operation_id
      );
    };

    let response_type = operation.response_type.as_ref().map(|t| parse_type(t)).transpose()?;

    let response_handling = Self::build_response_handling(
      &request_ident,
      operation.response_enum.as_ref(),
      response_type.as_ref(),
      operation.response_content_category,
    );

    Ok(Self {
      method_name: format_ident!("{}", operation.stable_id),
      request_ident,
      doc_attrs: Self::build_doc_attributes(operation),
      builder_init: Self::build_http_method_init(&operation.method),
      header_statements: Self::build_header_statements(operation),
      body_statement: Self::build_body_statement(operation, rust_types),
      response_handling,
      visibility,
    })
  }

  pub(crate) fn build_doc_attributes(operation: &OperationInfo) -> TokenStream {
    let mut docs: Vec<String> = vec![];

    if let Some(summary) = &operation.summary {
      for line in summary.lines() {
        let trimmed = line.trim();
        if !trimmed.is_empty() {
          docs.push(trimmed.to_string());
        }
      }
    }

    if let Some(description) = &operation.description {
      if operation.summary.is_some() {
        docs.push(String::new());
      }
      for line in description.lines() {
        docs.push(line.trim().to_string());
      }
    }

    if operation.summary.is_some() || operation.description.is_some() {
      docs.push(String::new());
    }

    docs.push(format!("{} {}", operation.method.as_str(), operation.path));

    generate_docs(&docs)
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
        let method = format_ident!("reqwest::Method::{}", method.as_str());
        quote! {
          self.client.request(#method, url)
        }
      }
    }
  }

  fn build_header_statements(operation: &OperationInfo) -> Vec<TokenStream> {
    operation
      .parameters
      .iter()
      .filter(|param| matches!(param.location, ParameterLocation::Header))
      .map(|param| {
        let const_token = ConstToken::from_raw(param.original_name.as_str());
        let field_ident = &param.rust_field;

        let value_conversion = Self::build_header_value_conversion(&param.rust_type, field_ident, param.required);

        if param.required {
          quote! {
            {
              #value_conversion
              req_builder = req_builder.header(#const_token, header_value);
            }
          }
        } else {
          quote! {
            if let Some(value) = request.#field_ident.as_ref() {
              #value_conversion
              req_builder = req_builder.header(#const_token, header_value);
            }
          }
        }
      })
      .collect()
  }

  fn build_header_value_conversion(rust_type: &TypeRef, field_ident: &FieldNameToken, required: bool) -> TokenStream {
    let value_expr = if required {
      quote! { request.#field_ident }
    } else {
      quote! { value }
    };

    if rust_type.is_string_like() {
      quote! {
        let header_value = HeaderValue::from_str(#value_expr.as_str())?;
      }
    } else if rust_type.is_primitive_type() {
      quote! {
        let header_value = HeaderValue::from_str(&(#value_expr).to_string())?;
      }
    } else {
      quote! {
        let header_value = HeaderValue::from_str(&serde_plain::to_string(&#value_expr)?)?;
      }
    }
  }

  fn build_body_statement(operation: &OperationInfo, rust_types: &[RustType]) -> TokenStream {
    operation
      .body
      .as_ref()
      .map(|body| Self::build_body_for_content_type(body, operation, rust_types))
      .unwrap_or_default()
  }

  fn build_body_for_content_type(
    body: &OperationBody,
    operation: &OperationInfo,
    rust_types: &[RustType],
  ) -> TokenStream {
    let field_ident = &body.field_name;

    match body.content_category {
      ContentCategory::Json => Self::build_json_body(field_ident, body.optional),
      ContentCategory::FormUrlEncoded => Self::build_form_body(field_ident, body.optional),
      ContentCategory::Multipart => Self::build_multipart_body(field_ident, body.optional, operation, rust_types),
      ContentCategory::Text => Self::build_text_body(field_ident, body.optional),
      ContentCategory::Binary => Self::build_binary_body(field_ident, body.optional),
      ContentCategory::Xml => Self::build_xml_body(field_ident, body.optional),
    }
  }

  fn wrap_optional_body<F>(field_ident: &FieldNameToken, optional: bool, make_statement: F) -> TokenStream
  where
    F: FnOnce(TokenStream) -> TokenStream,
  {
    if optional {
      let body_expr = quote! { body };
      let statement = make_statement(body_expr);
      quote! {
        if let Some(body) = request.#field_ident.as_ref() {
          #statement
        }
      }
    } else {
      let body_expr = quote! { &request.#field_ident };
      make_statement(body_expr)
    }
  }

  fn build_json_body(field_ident: &FieldNameToken, optional: bool) -> TokenStream {
    Self::wrap_optional_body(field_ident, optional, |body_expr| {
      quote! { req_builder = req_builder.json(#body_expr); }
    })
  }

  fn build_form_body(field_ident: &FieldNameToken, optional: bool) -> TokenStream {
    Self::wrap_optional_body(field_ident, optional, |body_expr| {
      quote! { req_builder = req_builder.form(#body_expr); }
    })
  }

  pub(crate) fn build_multipart_body(
    field_ident: &FieldNameToken,
    optional: bool,
    operation: &OperationInfo,
    rust_types: &[RustType],
  ) -> TokenStream {
    let multipart_logic = Self::resolve_multipart_struct(operation, rust_types, field_ident)
      .map_or_else(Self::generate_fallback_multipart, Self::generate_strict_multipart);

    if optional {
      quote! {
        if let Some(body) = request.#field_ident.as_ref() {
          #multipart_logic
        }
      }
    } else {
      quote! {
        let body = &request.#field_ident;
        #multipart_logic
      }
    }
  }

  fn resolve_multipart_struct<'a>(
    operation: &OperationInfo,
    rust_types: &'a [RustType],
    field_ident: &FieldNameToken,
  ) -> Option<&'a StructDef> {
    let req_type = operation.request_type.as_ref()?;
    let req_struct = Self::find_struct_by_name(req_type, rust_types)?;
    let field_def = req_struct.fields.iter().find(|f| *field_ident == f.name.as_str())?;
    if let RustPrimitive::Custom(name) = &field_def.rust_type.base_type {
      let field_token = StructToken::from(name.clone());
      Self::find_struct_by_name(&field_token, rust_types)
    } else {
      None
    }
  }

  fn find_struct_by_name<'a>(name: &StructToken, types: &'a [RustType]) -> Option<&'a StructDef> {
    types.iter().find_map(|t| match t {
      RustType::Struct(s) if &s.name == name => Some(s),
      _ => None,
    })
  }

  fn generate_strict_multipart(body_struct: &StructDef) -> TokenStream {
    let parts = body_struct.fields.iter().map(Self::generate_multipart_part);
    quote! {
      let mut form = reqwest::multipart::Form::new();
      #(#parts)*
      req_builder = req_builder.multipart(form);
    }
  }

  fn generate_multipart_part(field: &FieldDef) -> TokenStream {
    let ident = format_ident!("{}", field.name);
    let name = field.name.as_str();
    let is_bytes = matches!(field.rust_type.base_type, RustPrimitive::Bytes);

    let value_to_part = |val: TokenStream| {
      if is_bytes {
        quote! { Part::bytes(std::borrow::Cow::from(#val.clone())) }
      } else {
        quote! { Part::text(#val.to_string()) }
      }
    };

    if field.rust_type.nullable {
      let part = value_to_part(quote! { val });
      quote! {
        if let Some(val) = &body.#ident {
          form = form.part(#name, #part);
        }
      }
    } else {
      let part = value_to_part(quote! { body.#ident });
      quote! { form = form.part(#name, #part); }
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

  fn build_text_body(field_ident: &FieldNameToken, optional: bool) -> TokenStream {
    Self::wrap_optional_body(field_ident, optional, |body_expr| {
      quote! { req_builder = req_builder.body((#body_expr).to_string()); }
    })
  }

  fn build_binary_body(field_ident: &FieldNameToken, optional: bool) -> TokenStream {
    Self::wrap_optional_body(field_ident, optional, |body_expr| {
      quote! { req_builder = req_builder.body((#body_expr).clone()); }
    })
  }

  fn build_xml_body(field_ident: &FieldNameToken, optional: bool) -> TokenStream {
    Self::wrap_optional_body(field_ident, optional, |body_expr| {
      quote! {
        let xml_string = (#body_expr).to_string();
        req_builder = req_builder
          .header("Content-Type", "application/xml")
          .body(xml_string);
      }
    })
  }

  fn build_response_handling(
    request_ident: &syn::Ident,
    response_enum: Option<&EnumToken>,
    response_type: Option<&syn::Type>,
    response_content_category: ContentCategory,
  ) -> ResponseHandling {
    let raw_response = || ResponseHandling {
      success_type: quote! { reqwest::Response },
      parse_body: quote! { Ok(response) },
    };

    if let Some(response_enum) = response_enum {
      return ResponseHandling {
        success_type: quote! { #response_enum },
        parse_body: quote! {
          let parsed = #request_ident::parse_response(response).await?;
          Ok(parsed)
        },
      };
    }

    let Some(response_ty) = response_type else {
      return raw_response();
    };

    match response_content_category {
      ContentCategory::Json => ResponseHandling {
        success_type: quote! { #response_ty },
        parse_body: quote! {
          let parsed = response.json::<#response_ty>().await?;
          Ok(parsed)
        },
      },
      ContentCategory::Text => ResponseHandling {
        success_type: quote! { String },
        parse_body: quote! {
          let text = response.text().await?;
          Ok(text)
        },
      },
      ContentCategory::Binary | ContentCategory::Xml | ContentCategory::FormUrlEncoded | ContentCategory::Multipart => {
        raw_response()
      }
    }
  }
}

impl ToTokens for ClientOperationMethod {
  fn to_tokens(&self, tokens: &mut TokenStream) {
    let method_name = &self.method_name;
    let request_ident = &self.request_ident;
    let doc_attrs = &self.doc_attrs;
    let builder_init = &self.builder_init;
    let header_statements = &self.header_statements;
    let body_statement = &self.body_statement;
    let success_type = &self.response_handling.success_type;
    let parse_body = &self.response_handling.parse_body;
    let vis = self.visibility.to_tokens();

    quote! {
      #doc_attrs
      #vis async fn #method_name(&self, request: #request_ident) -> anyhow::Result<#success_type> {
        request.validate().context("parameter validation")?;
        let url = self
          .base_url
          .join(&request.render_path()?)
          .context("constructing request url")?;
        let mut req_builder = #builder_init;
        #(#header_statements)*
        #body_statement
        let response = req_builder.send().await?;
        #parse_body
      }
    }
    .to_tokens(tokens);
  }
}

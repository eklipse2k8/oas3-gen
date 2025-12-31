use http::Method;
use proc_macro2::{Span, TokenStream};
use quote::{ToTokens, format_ident, quote};
use syn::LitStr;

use super::Visibility;
use crate::generator::{
  ast::{
    ClientDef, ContentCategory, Documentation, FieldNameToken, OperationInfo, OperationKind, ParameterLocation,
    RustPrimitive, RustType, RustTypeCollection, StructDef, StructToken,
  },
  codegen::parse_type,
  naming::identifiers::to_rust_type_name,
};

pub struct ClientGenerator<'a> {
  def: &'a ClientDef,
  operations: &'a [OperationInfo],
  rust_types: &'a [RustType],
  visibility: Visibility,
  use_types_import: bool,
}

impl<'a> ClientGenerator<'a> {
  pub fn new(
    def: &'a ClientDef,
    operations: &'a [OperationInfo],
    rust_types: &'a [RustType],
    visibility: Visibility,
  ) -> Self {
    Self {
      def,
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
    let client_name = if self.def.title.is_empty() {
      "Api".to_string()
    } else {
      to_rust_type_name(&self.def.title)
    };
    format_ident!("{client_name}Client")
  }

  fn client_struct(&self, client_ident: &syn::Ident) -> TokenStream {
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
    let client_ident = self.client_ident();
    let vis = self.visibility.to_tokens();
    let base_url = LitStr::new(&self.def.base_url, Span::call_site());

    let methods = self
      .operations
      .iter()
      .filter(|op| op.kind == OperationKind::Http)
      .filter_map(|op| {
        method::MethodGenerator::new(op, self.rust_types, self.visibility)
          .emit()
          .ok()
      });

    let types_import = if self.use_types_import {
      quote! { use super::types::*; }
    } else {
      quote! {}
    };

    let client_struct = self.client_struct(&client_ident);
    let constructors = self.constructors();

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

pub(crate) mod method {
  use super::*;

  pub(crate) struct MethodGenerator<'a> {
    op: &'a OperationInfo,
    rust_types: &'a [RustType],
    visibility: Visibility,
  }

  impl<'a> MethodGenerator<'a> {
    pub(crate) fn new(op: &'a OperationInfo, rust_types: &'a [RustType], visibility: Visibility) -> Self {
      Self {
        op,
        rust_types,
        visibility,
      }
    }

    pub(crate) fn emit(&self) -> anyhow::Result<TokenStream> {
      let Some(request_ident) = self.op.request_type.as_ref().map(|r| format_ident!("{r}")) else {
        anyhow::bail!("operation `{}` is missing request type", self.op.operation_id);
      };

      let method_name = format_ident!("{}", self.op.stable_id);
      let doc_attrs = doc_attributes(self.op);
      let builder_init = http_init(&self.op.method);
      let url_construction = url_construction(self.op);

      let query_chain = params::query(self.op);
      let header_chain = params::headers(self.op);
      let body_result = body::BodyGenerator::new(self.op, self.rust_types).emit();

      let response_logic = response::build(self.op);

      let vis = self.visibility.to_tokens();
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
  }

  pub(crate) fn doc_attributes(op: &OperationInfo) -> Documentation {
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

  fn http_init(method: &Method) -> TokenStream {
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

  fn url_construction(op: &OperationInfo) -> TokenStream {
    let segments = &op.path.0;
    quote! {
      let mut url = self.base_url.clone();
      url.path_segments_mut()
         .map_err(|()| anyhow::anyhow!("URL cannot be a base"))?
         #(#segments)*;
    }
  }

  pub(crate) mod params {
    use super::*;

    pub(crate) fn query(op: &OperationInfo) -> TokenStream {
      if op
        .parameters
        .iter()
        .any(|p| matches!(p.parameter_location, Some(ParameterLocation::Query)))
      {
        quote! { .query(&request.query) }
      } else {
        quote! {}
      }
    }

    pub(crate) fn headers(op: &OperationInfo) -> TokenStream {
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
  }

  pub(crate) mod body {
    use super::*;

    pub(crate) struct BodyResult {
      pub(crate) tokens: TokenStream,
      pub(crate) needs_conditional: bool,
    }

    pub(crate) struct BodyGenerator<'a> {
      op: &'a OperationInfo,
      rust_types: &'a [RustType],
    }

    impl<'a> BodyGenerator<'a> {
      pub(crate) fn new(op: &'a OperationInfo, rust_types: &'a [RustType]) -> Self {
        Self { op, rust_types }
      }

      pub(crate) fn emit(&self) -> BodyResult {
        let Some(body) = &self.op.body else {
          return BodyResult {
            tokens: quote! {},
            needs_conditional: false,
          };
        };
        let field = &body.field_name;

        match body.content_category {
          ContentCategory::Json => Self::chain_or_conditional(field, body.optional, |e| quote! { .json(#e) }),
          ContentCategory::FormUrlEncoded => Self::chain_or_conditional(field, body.optional, |e| quote! { .form(#e) }),
          ContentCategory::Text | ContentCategory::EventStream => {
            Self::chain_or_conditional(field, body.optional, |e| quote! { .body((#e).to_string()) })
          }
          ContentCategory::Binary => {
            Self::chain_or_conditional(field, body.optional, |e| quote! { .body((#e).clone()) })
          }
          ContentCategory::Xml => {
            if body.optional {
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
          ContentCategory::Multipart => {
            multipart::MultipartGenerator::new(self.op, self.rust_types, field, body.optional).emit()
          }
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
    }

    pub(crate) mod multipart {
      use super::*;
      use crate::generator::ast::FieldCollection;

      pub(crate) struct MultipartGenerator<'a> {
        op: &'a OperationInfo,
        rust_types: &'a [RustType],
        field: &'a FieldNameToken,
        optional: bool,
      }

      impl<'a> MultipartGenerator<'a> {
        pub(crate) fn new(
          op: &'a OperationInfo,
          rust_types: &'a [RustType],
          field: &'a FieldNameToken,
          optional: bool,
        ) -> Self {
          Self {
            op,
            rust_types,
            field,
            optional,
          }
        }

        pub(crate) fn emit(&self) -> BodyResult {
          let logic = self.resolve_struct().map_or_else(fallback, strict);
          let field = self.field;

          let tokens = if self.optional {
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

        fn resolve_struct(&self) -> Option<&'a StructDef> {
          let req_type = self.op.request_type.as_ref()?;
          let req_struct = self.rust_types.find_struct(req_type)?;
          let field_def = req_struct.fields.find_name(self.field)?;

          if let RustPrimitive::Custom(name) = &field_def.rust_type.base_type {
            self.rust_types.find_struct(&StructToken::from(name.clone()))
          } else {
            None
          }
        }
      }

      fn strict(def: &StructDef) -> TokenStream {
        let parts = def.fields.iter().map(|f| {
          let ident = &f.name;
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

      fn fallback() -> TokenStream {
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
    }
  }

  pub(crate) mod response {
    use super::*;

    pub(crate) struct ResponseHandling {
      pub(crate) success_type: TokenStream,
      pub(crate) parse_body: TokenStream,
    }

    pub(crate) fn build(op: &OperationInfo) -> ResponseHandling {
      if let Some(enum_token) = &op.response_enum {
        let req_ident = format_ident!("{}", op.request_type.as_ref().unwrap());
        return ResponseHandling {
          success_type: quote! { #enum_token },
          parse_body: quote! { Ok(#req_ident::parse_response(response).await?) },
        };
      }

      let Some(resp_type_str) = &op.response_type else {
        return raw();
      };

      let Ok(resp_ty) = parse_type(resp_type_str) else {
        return raw();
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
        _ => raw(),
      }
    }

    fn raw() -> ResponseHandling {
      ResponseHandling {
        success_type: quote! { reqwest::Response },
        parse_body: quote! { Ok(response) },
      }
    }
  }
}

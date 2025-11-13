use anyhow::anyhow;
use proc_macro2::TokenStream;
use quote::{format_ident, quote};

use crate::generator::ast::{OperationInfo, ParameterLocation};

pub(super) fn build_method_tokens(operation: &OperationInfo) -> anyhow::Result<TokenStream> {
  let request_type_name = operation.request_type.as_ref().ok_or_else(|| {
    anyhow!(
      "operation `{}` is missing request type information",
      operation.operation_id
    )
  })?;
  let request_ident = format_ident!("{}", request_type_name);

  let response_enum_type = operation
    .response_enum
    .as_ref()
    .map(|name| parse_type(name))
    .transpose()?;
  let response_type = operation
    .response_type
    .as_ref()
    .map(|name| parse_type(name))
    .transpose()?;

  let method_name = format_ident!("{}", operation.stable_id);
  let method_lower = operation.method.to_ascii_lowercase();
  let builder_init = match method_lower.as_str() {
    "get" => quote! { self.client.get(url) },
    "post" => quote! { self.client.post(url) },
    "put" => quote! { self.client.put(url) },
    "delete" => quote! { self.client.delete(url) },
    "patch" => quote! { self.client.patch(url) },
    "head" => quote! { self.client.head(url) },
    _ => {
      let method_upper = syn::Ident::new(&method_lower.to_ascii_uppercase(), proc_macro2::Span::call_site());
      quote! { self.client.request(reqwest::Method::#method_upper, url) }
    }
  };

  let mut doc_attrs: Vec<TokenStream> = Vec::new();
  if let Some(summary) = &operation.summary {
    for line in summary.lines() {
      let trimmed = line.trim();
      if !trimmed.is_empty() {
        let lit = syn::LitStr::new(trimmed, proc_macro2::Span::call_site());
        doc_attrs.push(quote! { #[doc = #lit] });
      }
    }
  }
  let signature_doc = format!("{} {}", operation.method.to_uppercase(), operation.path);
  let signature_lit = syn::LitStr::new(&signature_doc, proc_macro2::Span::call_site());
  doc_attrs.push(quote! { #[doc = #signature_lit] });

  let header_statements: Vec<TokenStream> = operation
    .parameters
    .iter()
    .filter(|param| matches!(param.location, ParameterLocation::Header))
    .map(|param| {
      let header_name = param.original_name.clone();
      let field_ident = format_ident!("{}", param.rust_field);
      if param.required {
        quote! {
          {
            let header_value = HeaderValue::from_str(&request.#field_ident.to_string())?;
            req_builder = req_builder.header(#header_name, header_value);
          }
        }
      } else {
        quote! {
          if let Some(value) = request.#field_ident.as_ref() {
            let header_value = HeaderValue::from_str(&value.to_string())?;
            req_builder = req_builder.header(#header_name, header_value);
          }
        }
      }
    })
    .collect();

  let body_statement = operation.body.as_ref().map(|body| {
    let field_ident = format_ident!("{}", body.field_name);
    let content_type = body
      .content_type
      .as_deref()
      .unwrap_or("application/json")
      .to_ascii_lowercase();
    if content_type.contains("json") {
      if body.optional {
        quote! {
          if let Some(body) = request.#field_ident.as_ref() {
            req_builder = req_builder.json(body);
          }
        }
      } else {
        quote! {
          req_builder = req_builder.json(&request.#field_ident);
        }
      }
    } else if content_type.contains("form") {
      if body.optional {
        quote! {
          if let Some(body) = request.#field_ident.as_ref() {
            req_builder = req_builder.form(body);
          }
        }
      } else {
        quote! {
          req_builder = req_builder.form(&request.#field_ident);
        }
      }
    } else if content_type.contains("multipart") {
      quote! {
        /* TODO: build multipart/form-data payload using `Form` and `Part`. */
      }
    } else {
      quote! {
        /* TODO: handle request body for unsupported content types. */
      }
    }
  });
  let body_statement = body_statement.unwrap_or_default();

  let (return_ty, response_handling) = if let Some(response_enum) = response_enum_type {
    (
      quote! { #response_enum },
      quote! {
        let parsed = #request_ident::parse_response(response).await?;
        Ok(parsed)
      },
    )
  } else if let Some(response_ty) = response_type {
    (
      quote! { #response_ty },
      quote! {
        let parsed = response.json::<#response_ty>().await?;
        Ok(parsed)
      },
    )
  } else {
    (quote! { reqwest::Response }, quote! { Ok(response) })
  };

  Ok(quote! {
    #(#doc_attrs)*
    pub async fn #method_name(&self, request: #request_ident) -> anyhow::Result<#return_ty> {
      request.validate().context("parameter validation")?;
      let url = self
        .base_url
        .join(&request.render_path())
        .context("constructing request url")?;
      let mut req_builder = #builder_init;
      #(#header_statements)*
      #body_statement
      let response = req_builder.send().await?;
      #response_handling
    }
  })
}

fn parse_type(type_name: &str) -> anyhow::Result<syn::Type> {
  syn::parse_str(type_name).map_err(|err| anyhow!("failed to parse type `{type_name}`: {err}"))
}

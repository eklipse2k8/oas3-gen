use anyhow::anyhow;
use proc_macro2::TokenStream;
use quote::{format_ident, quote};

use crate::generator::ast::{OperationBody, OperationInfo, ParameterLocation};

struct TypeInfo {
  request_ident: syn::Ident,
  response_enum: Option<syn::Type>,
  response_type: Option<syn::Type>,
}

struct MethodComponents {
  doc_attrs: Vec<TokenStream>,
  builder_init: TokenStream,
  header_statements: Vec<TokenStream>,
  body_statement: TokenStream,
  return_ty: TokenStream,
  response_handling: TokenStream,
}

pub(super) fn build_method_tokens(operation: &OperationInfo) -> anyhow::Result<TokenStream> {
  let type_info = extract_type_info(operation)?;
  let method_name = format_ident!("{}", operation.stable_id);

  let (return_ty, response_handling) = build_response_handling(&type_info);
  let components = MethodComponents {
    doc_attrs: build_doc_attributes(operation),
    builder_init: build_http_method_init(&operation.method),
    header_statements: build_header_statements(operation),
    body_statement: build_body_statement(operation),
    return_ty,
    response_handling,
  };

  Ok(assemble_method_tokens(
    &method_name,
    &type_info.request_ident,
    &components,
  ))
}

fn extract_type_info(operation: &OperationInfo) -> anyhow::Result<TypeInfo> {
  let request_type_name = operation.request_type.as_ref().ok_or_else(|| {
    anyhow!(
      "operation `{}` is missing request type information",
      operation.operation_id
    )
  })?;
  let request_ident = format_ident!("{}", request_type_name);

  let response_enum = operation
    .response_enum
    .as_ref()
    .map(|name| parse_type(name))
    .transpose()?;
  let response_type = operation
    .response_type
    .as_ref()
    .map(|name| parse_type(name))
    .transpose()?;

  Ok(TypeInfo {
    request_ident,
    response_enum,
    response_type,
  })
}

fn build_http_method_init(method: &str) -> TokenStream {
  let method_lower = method.to_ascii_lowercase();
  match method_lower.as_str() {
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
  }
}

fn build_doc_attributes(operation: &OperationInfo) -> Vec<TokenStream> {
  let mut doc_attrs = Vec::new();
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

  doc_attrs
}

fn build_header_statements(operation: &OperationInfo) -> Vec<TokenStream> {
  operation
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
    .collect()
}

fn build_body_statement(operation: &OperationInfo) -> TokenStream {
  operation
    .body
    .as_ref()
    .map(build_body_for_content_type)
    .unwrap_or_default()
}

fn build_body_for_content_type(body: &OperationBody) -> TokenStream {
  let field_ident = format_ident!("{}", body.field_name);
  let content_type = body
    .content_type
    .as_deref()
    .unwrap_or("application/json")
    .to_ascii_lowercase();

  if content_type.contains("json") {
    build_json_body(&field_ident, body.optional)
  } else if content_type.contains("x-www-form-urlencoded") {
    build_form_body(&field_ident, body.optional)
  } else if content_type.contains("multipart") {
    build_multipart_body(&field_ident, body.optional)
  } else if content_type.contains("text/plain") || content_type.contains("text/html") {
    build_text_body(&field_ident, body.optional)
  } else if content_type.contains("octet-stream")
    || content_type.starts_with("application/") && !content_type.contains("json")
  {
    build_binary_body(&field_ident, body.optional)
  } else if content_type.contains("xml") {
    build_xml_body(&field_ident, body.optional)
  } else {
    build_fallback_body(&field_ident, body.optional)
  }
}

fn build_json_body(field_ident: &syn::Ident, optional: bool) -> TokenStream {
  if optional {
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
}

fn build_form_body(field_ident: &syn::Ident, optional: bool) -> TokenStream {
  if optional {
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
}

fn build_multipart_body(field_ident: &syn::Ident, optional: bool) -> TokenStream {
  let multipart_logic = quote! {
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

fn build_text_body(field_ident: &syn::Ident, optional: bool) -> TokenStream {
  if optional {
    quote! {
      if let Some(body) = request.#field_ident.as_ref() {
        req_builder = req_builder.body(body.to_string());
      }
    }
  } else {
    quote! {
      req_builder = req_builder.body(request.#field_ident.to_string());
    }
  }
}

fn build_binary_body(field_ident: &syn::Ident, optional: bool) -> TokenStream {
  if optional {
    quote! {
      if let Some(body) = request.#field_ident.as_ref() {
        req_builder = req_builder.body(body.clone());
      }
    }
  } else {
    quote! {
      req_builder = req_builder.body(request.#field_ident.clone());
    }
  }
}

fn build_xml_body(field_ident: &syn::Ident, optional: bool) -> TokenStream {
  if optional {
    quote! {
      if let Some(body) = request.#field_ident.as_ref() {
        let xml_string = body.to_string();
        req_builder = req_builder
          .header("Content-Type", "application/xml")
          .body(xml_string);
      }
    }
  } else {
    quote! {
      let xml_string = request.#field_ident.to_string();
      req_builder = req_builder
        .header("Content-Type", "application/xml")
        .body(xml_string);
    }
  }
}

fn build_fallback_body(field_ident: &syn::Ident, optional: bool) -> TokenStream {
  if optional {
    quote! {
      if let Some(body) = request.#field_ident.as_ref() {
        req_builder = req_builder.body(serde_json::to_vec(body)?);
      }
    }
  } else {
    quote! {
      req_builder = req_builder.body(serde_json::to_vec(&request.#field_ident)?);
    }
  }
}

fn build_response_handling(type_info: &TypeInfo) -> (TokenStream, TokenStream) {
  if let Some(response_enum) = &type_info.response_enum {
    let request_ident = &type_info.request_ident;
    return (
      quote! { #response_enum },
      quote! {
        let parsed = #request_ident::parse_response(response).await?;
        Ok(parsed)
      },
    );
  }

  if let Some(response_ty) = &type_info.response_type {
    return (
      quote! { #response_ty },
      quote! {
        let parsed = response.json::<#response_ty>().await?;
        Ok(parsed)
      },
    );
  }

  (quote! { reqwest::Response }, quote! { Ok(response) })
}

fn assemble_method_tokens(
  method_name: &syn::Ident,
  request_ident: &syn::Ident,
  components: &MethodComponents,
) -> TokenStream {
  let doc_attrs = &components.doc_attrs;
  let builder_init = &components.builder_init;
  let header_statements = &components.header_statements;
  let body_statement = &components.body_statement;
  let return_ty = &components.return_ty;
  let response_handling = &components.response_handling;

  quote! {
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
  }
}

fn parse_type(type_name: &str) -> anyhow::Result<syn::Type> {
  syn::parse_str(type_name).map_err(|err| anyhow!("failed to parse type `{type_name}`: {err}"))
}

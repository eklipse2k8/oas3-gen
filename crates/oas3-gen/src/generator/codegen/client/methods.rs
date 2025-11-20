use anyhow::anyhow;
use http::Method;
use proc_macro2::TokenStream;
use quote::{format_ident, quote};

use crate::{
  generator::ast::{
    FieldDef, OperationBody, OperationInfo, ParameterLocation, RustPrimitive, RustType, StructDef, TypeRef,
  },
  reserved::header_const_name,
};

struct TypeInfo {
  request_ident: syn::Ident,
  response_enum: Option<syn::Type>,
  response_type: Option<syn::Type>,
  response_content_type: Option<String>,
}

struct MethodComponents {
  doc_attrs: Vec<TokenStream>,
  builder_init: TokenStream,
  header_statements: Vec<TokenStream>,
  body_statement: TokenStream,
  return_ty: TokenStream,
  response_handling: TokenStream,
}

pub(super) fn build_method_tokens(operation: &OperationInfo, rust_types: &[RustType]) -> anyhow::Result<TokenStream> {
  let type_info = extract_type_info(operation)?;
  let method_name = format_ident!("{}", operation.stable_id);

  let (return_ty, response_handling) = build_response_handling(&type_info);
  let components = MethodComponents {
    doc_attrs: build_doc_attributes(operation),
    builder_init: build_http_method_init(&operation.method),
    header_statements: build_header_statements(operation),
    body_statement: build_body_statement(operation, rust_types),
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
  let request_ident = format_ident!("{request_type_name}");

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
    response_content_type: operation.response_content_type.clone(),
  })
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
        // Using request for uncommon HTTP method
        self.client.request(#method, url)
      }
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

  if let Some(description) = &operation.description {
    if operation.summary.is_some() {
      doc_attrs.push(quote! { #[doc = ""] });
    }
    for line in description.lines() {
      let trimmed = line.trim();
      let lit = syn::LitStr::new(trimmed, proc_macro2::Span::call_site());
      doc_attrs.push(quote! { #[doc = #lit] });
    }
  }

  if operation.summary.is_some() || operation.description.is_some() {
    doc_attrs.push(quote! { #[doc = ""] });
  }

  let signature_doc = format!("{} {}", operation.method.as_str(), operation.path);
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
      let const_name = header_const_name(&param.original_name.to_ascii_lowercase());
      let const_ident = format_ident!("{const_name}");
      let field_ident = format_ident!("{}", param.rust_field);

      let value_conversion = build_header_value_conversion(&param.rust_type, &field_ident, param.required);

      if param.required {
        quote! {
          {
            #value_conversion
            req_builder = req_builder.header(#const_ident, header_value);
          }
        }
      } else {
        quote! {
          if let Some(value) = request.#field_ident.as_ref() {
            #value_conversion
            req_builder = req_builder.header(#const_ident, header_value);
          }
        }
      }
    })
    .collect()
}

fn build_header_value_conversion(rust_type: &TypeRef, field_ident: &syn::Ident, required: bool) -> TokenStream {
  let value_expr = if required {
    quote! { request.#field_ident }
  } else {
    quote! { value }
  };

  if is_string_type(rust_type) {
    quote! {
      let header_value = HeaderValue::from_str(#value_expr.as_str())?;
    }
  } else if is_primitive_type(rust_type) {
    quote! {
      let header_value = HeaderValue::from_str(&(#value_expr).to_string())?;
    }
  } else {
    quote! {
      let header_value = HeaderValue::from_str(&serde_plain::to_string(&#value_expr)?)?;
    }
  }
}

fn is_string_type(type_ref: &TypeRef) -> bool {
  matches!(type_ref.base_type, RustPrimitive::String) && !type_ref.is_array
}

fn is_primitive_type(type_ref: &TypeRef) -> bool {
  matches!(
    type_ref.base_type,
    RustPrimitive::I8
      | RustPrimitive::I16
      | RustPrimitive::I32
      | RustPrimitive::I64
      | RustPrimitive::I128
      | RustPrimitive::Isize
      | RustPrimitive::U8
      | RustPrimitive::U16
      | RustPrimitive::U32
      | RustPrimitive::U64
      | RustPrimitive::U128
      | RustPrimitive::Usize
      | RustPrimitive::F32
      | RustPrimitive::F64
      | RustPrimitive::Bool
  ) && !type_ref.is_array
}

fn build_body_statement(operation: &OperationInfo, rust_types: &[RustType]) -> TokenStream {
  operation
    .body
    .as_ref()
    .map(|body| build_body_for_content_type(body, operation, rust_types))
    .unwrap_or_default()
}

fn build_body_for_content_type(
  body: &OperationBody,
  operation: &OperationInfo,
  rust_types: &[RustType],
) -> TokenStream {
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
    build_multipart_body(&field_ident, body.optional, operation, rust_types)
  } else if content_type.contains("text/plain") || content_type.contains("text/html") {
    build_text_body(&field_ident, body.optional)
  } else if content_type.contains("octet-stream")
    || (content_type.starts_with("application/") && !content_type.contains("json"))
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

fn build_multipart_body(
  field_ident: &syn::Ident,
  optional: bool,
  operation: &OperationInfo,
  rust_types: &[RustType],
) -> TokenStream {
  let multipart_logic = resolve_multipart_struct(operation, rust_types, field_ident)
    .map_or_else(generate_fallback_multipart, generate_strict_multipart);

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
  field_ident: &syn::Ident,
) -> Option<&'a StructDef> {
  let req_type = operation.request_type.as_ref()?;
  let req_struct = find_struct_by_name(req_type, rust_types)?;
  let field_def = req_struct.fields.iter().find(|f| *field_ident == f.name)?;
  find_struct_by_name(&field_def.rust_type.base_type.to_string(), rust_types)
}

fn find_struct_by_name<'a>(name: &str, types: &'a [RustType]) -> Option<&'a StructDef> {
  types.iter().find_map(|t| match t {
    RustType::Struct(s) if s.name == name => Some(s),
    _ => None,
  })
}

fn generate_strict_multipart(body_struct: &StructDef) -> TokenStream {
  let parts = body_struct.fields.iter().map(generate_multipart_part);
  quote! {
    let mut form = reqwest::multipart::Form::new();
    #(#parts)*
    req_builder = req_builder.multipart(form);
  }
}

fn generate_multipart_part(field: &FieldDef) -> TokenStream {
  let ident = format_ident!("{}", field.name);
  let name = &field.name;
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
    let content_type = type_info
      .response_content_type
      .as_deref()
      .unwrap_or("application/json")
      .to_ascii_lowercase();

    if content_type.contains("json") {
      return (
        quote! { #response_ty },
        quote! {
          let parsed = response.json::<#response_ty>().await?;
          Ok(parsed)
        },
      );
    } else if content_type.contains("text/plain") || content_type.contains("text/html") {
      return (
        quote! { String },
        quote! {
          let text = response.text().await?;
          Ok(text)
        },
      );
    } else if content_type.starts_with("image/")
      || content_type.starts_with("video/")
      || content_type.starts_with("audio/")
      || content_type == "application/octet-stream"
      || content_type.starts_with("application/pdf")
    {
      return (quote! { reqwest::Response }, quote! { Ok(response) });
    }
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
        .join(&request.render_path()?)
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

#[cfg(test)]
mod tests {
  use std::collections::BTreeSet;

  use super::*;
  use crate::generator::ast::{FieldDef, RustPrimitive, RustType, StructDef, StructKind, TypeRef};

  fn create_test_operation(summary: Option<&str>, description: Option<&str>) -> OperationInfo {
    OperationInfo {
      stable_id: "test_operation".to_string(),
      operation_id: "testOperation".to_string(),
      method: Method::GET,
      path: "/test".to_string(),
      summary: summary.map(String::from),
      description: description.map(String::from),
      request_type: Some("TestRequest".to_string()),
      response_type: Some("TestResponse".to_string()),
      response_enum: None,
      response_content_type: None,
      request_body_types: vec![],
      success_response_types: vec![],
      error_response_types: vec![],
      warnings: vec![],
      parameters: vec![],
      body: None,
    }
  }

  #[test]
  fn test_build_doc_attributes_with_summary_only() {
    let operation = create_test_operation(Some("Test summary"), None);
    let doc_attrs = build_doc_attributes(&operation);

    let output = quote! { #(#doc_attrs)* }.to_string();

    assert!(output.contains("Test summary"));
    assert!(output.contains("GET /test"));
    assert!(!output.contains("Test description"));
  }

  #[test]
  fn test_build_doc_attributes_with_description_only() {
    let operation = create_test_operation(None, Some("Test description"));
    let doc_attrs = build_doc_attributes(&operation);

    let output = quote! { #(#doc_attrs)* }.to_string();

    assert!(output.contains("Test description"));
    assert!(output.contains("GET /test"));
  }

  #[test]
  fn test_build_doc_attributes_with_both_summary_and_description() {
    let operation = create_test_operation(Some("Test summary"), Some("Test description"));
    let doc_attrs = build_doc_attributes(&operation);

    let output = quote! { #(#doc_attrs)* }.to_string();

    assert!(output.contains("Test summary"));
    assert!(output.contains("Test description"));
    assert!(output.contains("GET /test"));

    let summary_pos = output.find("Test summary").unwrap();
    let description_pos = output.find("Test description").unwrap();
    let signature_pos = output.find("GET /test").unwrap();

    assert!(summary_pos < description_pos);
    assert!(description_pos < signature_pos);
  }

  #[test]
  fn test_build_doc_attributes_with_multiline_description() {
    let operation = create_test_operation(Some("Test summary"), Some("Line 1\nLine 2\nLine 3"));
    let doc_attrs = build_doc_attributes(&operation);

    let output = quote! { #(#doc_attrs)* }.to_string();

    assert!(output.contains("Line 1"));
    assert!(output.contains("Line 2"));
    assert!(output.contains("Line 3"));
  }

  #[test]
  fn test_build_doc_attributes_with_neither_summary_nor_description() {
    let operation = create_test_operation(None, None);
    let doc_attrs = build_doc_attributes(&operation);

    let output = quote! { #(#doc_attrs)* }.to_string();

    assert!(output.contains("GET /test"));
    assert_eq!(doc_attrs.len(), 1);
  }

  #[test]
  fn test_response_handling_with_json_content_type() {
    let type_info = TypeInfo {
      request_ident: format_ident!("TestRequest"),
      response_enum: None,
      response_type: Some(syn::parse_str("TestResponse").unwrap()),
      response_content_type: Some("application/json".to_string()),
    };

    let (return_ty, response_handling) = build_response_handling(&type_info);

    let return_ty_str = return_ty.to_string();
    let response_str = response_handling.to_string();

    assert!(return_ty_str.contains("TestResponse"));
    assert!(response_str.contains("json"));
    assert!(response_str.contains("TestResponse"));
  }

  #[test]
  fn test_response_handling_with_text_content_type() {
    let type_info = TypeInfo {
      request_ident: format_ident!("TestRequest"),
      response_enum: None,
      response_type: Some(syn::parse_str("TestResponse").unwrap()),
      response_content_type: Some("text/plain".to_string()),
    };

    let (return_ty, response_handling) = build_response_handling(&type_info);

    let return_ty_str = return_ty.to_string();
    let response_str = response_handling.to_string();

    assert_eq!(return_ty_str, "String");
    assert!(response_str.contains("text"));
  }

  #[test]
  fn test_response_handling_with_binary_content_type() {
    let type_info = TypeInfo {
      request_ident: format_ident!("TestRequest"),
      response_enum: None,
      response_type: Some(syn::parse_str("TestResponse").unwrap()),
      response_content_type: Some("application/octet-stream".to_string()),
    };

    let (return_ty, response_handling) = build_response_handling(&type_info);

    let return_ty_str = return_ty.to_string();
    let response_str = response_handling.to_string();

    assert_eq!(return_ty_str, "reqwest :: Response");
    assert!(response_str.contains("Ok (response)"));
  }

  #[test]
  fn test_response_handling_no_content_type_defaults_to_json() {
    let type_info = TypeInfo {
      request_ident: format_ident!("TestRequest"),
      response_enum: None,
      response_type: Some(syn::parse_str("TestResponse").unwrap()),
      response_content_type: None,
    };

    let (return_ty, response_handling) = build_response_handling(&type_info);

    let return_ty_str = return_ty.to_string();
    let response_str = response_handling.to_string();

    assert!(return_ty_str.contains("TestResponse"));
    assert!(response_str.contains("json"));
  }

  #[test]
  fn test_response_handling_with_response_enum() {
    let type_info = TypeInfo {
      request_ident: format_ident!("TestRequest"),
      response_enum: Some(syn::parse_str("TestResponseEnum").unwrap()),
      response_type: Some(syn::parse_str("TestResponse").unwrap()),
      response_content_type: Some("application/json".to_string()),
    };

    let (return_ty, response_handling) = build_response_handling(&type_info);

    let return_ty_str = return_ty.to_string();
    let response_str = response_handling.to_string();

    assert!(return_ty_str.contains("TestResponseEnum"));
    assert!(response_str.contains("parse_response"));
  }

  #[test]
  fn test_response_handling_with_image_png() {
    let type_info = TypeInfo {
      request_ident: format_ident!("TestRequest"),
      response_enum: None,
      response_type: Some(syn::parse_str("TestResponse").unwrap()),
      response_content_type: Some("image/png".to_string()),
    };

    let (return_ty, response_handling) = build_response_handling(&type_info);

    let return_ty_str = return_ty.to_string();
    let response_str = response_handling.to_string();

    assert_eq!(return_ty_str, "reqwest :: Response");
    assert!(response_str.contains("Ok (response)"));
  }

  #[test]
  fn test_response_handling_with_image_jpeg() {
    let type_info = TypeInfo {
      request_ident: format_ident!("TestRequest"),
      response_enum: None,
      response_type: Some(syn::parse_str("TestResponse").unwrap()),
      response_content_type: Some("image/jpeg".to_string()),
    };

    let (return_ty, response_handling) = build_response_handling(&type_info);

    let return_ty_str = return_ty.to_string();
    let response_str = response_handling.to_string();

    assert_eq!(return_ty_str, "reqwest :: Response");
    assert!(response_str.contains("Ok (response)"));
  }

  #[test]
  fn test_response_handling_with_video_content() {
    let type_info = TypeInfo {
      request_ident: format_ident!("TestRequest"),
      response_enum: None,
      response_type: Some(syn::parse_str("TestResponse").unwrap()),
      response_content_type: Some("video/mp4".to_string()),
    };

    let (return_ty, response_handling) = build_response_handling(&type_info);

    let return_ty_str = return_ty.to_string();
    let response_str = response_handling.to_string();

    assert_eq!(return_ty_str, "reqwest :: Response");
    assert!(response_str.contains("Ok (response)"));
  }

  #[test]
  fn test_response_handling_with_pdf() {
    let type_info = TypeInfo {
      request_ident: format_ident!("TestRequest"),
      response_enum: None,
      response_type: Some(syn::parse_str("TestResponse").unwrap()),
      response_content_type: Some("application/pdf".to_string()),
    };

    let (return_ty, response_handling) = build_response_handling(&type_info);

    let return_ty_str = return_ty.to_string();
    let response_str = response_handling.to_string();

    assert_eq!(return_ty_str, "reqwest :: Response");
    assert!(response_str.contains("Ok (response)"));
  }

  #[test]
  fn test_multipart_generation_strict_with_binary_and_text() {
    let binary_field = FieldDef {
      name: "file".to_string(),
      rust_type: TypeRef {
        base_type: RustPrimitive::Bytes,
        is_array: false,
        nullable: false,
        boxed: false,
        unique_items: false,
      },
      ..Default::default()
    };

    let text_field = FieldDef {
      name: "description".to_string(),
      rust_type: TypeRef {
        base_type: RustPrimitive::String,
        is_array: false,
        nullable: false,
        boxed: false,
        unique_items: false,
      },
      ..Default::default()
    };

    let body_struct = StructDef {
      name: "MultipartBody".to_string(),
      fields: vec![binary_field, text_field],
      docs: vec![],
      derives: BTreeSet::new(),
      serde_attrs: vec![],
      outer_attrs: vec![],
      methods: vec![],
      kind: StructKind::RequestBody,
    };

    let request_struct = StructDef {
      name: "UploadRequest".to_string(),
      fields: vec![FieldDef {
        name: "body".to_string(),
        rust_type: TypeRef {
          base_type: RustPrimitive::Custom("MultipartBody".to_string()),
          is_array: false,
          nullable: false,
          boxed: false,
          unique_items: false,
        },
        ..Default::default()
      }],
      docs: vec![],
      derives: BTreeSet::new(),
      serde_attrs: vec![],
      outer_attrs: vec![],
      methods: vec![],
      kind: StructKind::OperationRequest,
    };

    let rust_types = vec![RustType::Struct(request_struct), RustType::Struct(body_struct)];

    let operation = OperationInfo {
      stable_id: "upload".to_string(),
      operation_id: "upload".to_string(),
      method: Method::POST,
      path: "/upload".to_string(),
      summary: None,
      description: None,
      request_type: Some("UploadRequest".to_string()),
      response_type: None,
      response_enum: None,
      response_content_type: None,
      request_body_types: vec![],
      success_response_types: vec![],
      error_response_types: vec![],
      warnings: vec![],
      parameters: vec![],
      body: Some(OperationBody {
        field_name: "body".to_string(),
        optional: false,
        content_type: Some("multipart/form-data".to_string()),
      }),
    };

    let field_ident = format_ident!("body");
    let tokens = build_multipart_body(&field_ident, false, &operation, &rust_types);
    let code = tokens.to_string();

    assert!(code.contains("Part :: bytes")); // For binary field
    assert!(code.contains("Part :: text")); // For text field
    assert!(code.contains("form . part (\"file\""));
    assert!(code.contains("form . part (\"description\""));
    assert!(!code.contains("serde_json :: to_value")); // Should NOT fallback
  }

  #[test]
  fn test_multipart_generation_fallback() {
    let rust_types = vec![]; // No structs defined
    let operation = OperationInfo {
      stable_id: "upload".to_string(),
      operation_id: "upload".to_string(),
      method: Method::POST,
      path: "/upload".to_string(),
      summary: None,
      description: None,
      request_type: Some("UnknownRequest".to_string()), // Unknown type
      response_type: None,
      response_enum: None,
      response_content_type: None,
      request_body_types: vec![],
      success_response_types: vec![],
      error_response_types: vec![],
      warnings: vec![],
      parameters: vec![],
      body: Some(OperationBody {
        field_name: "body".to_string(),
        optional: false,
        content_type: Some("multipart/form-data".to_string()),
      }),
    };

    let field_ident = format_ident!("body");
    let tokens = build_multipart_body(&field_ident, false, &operation, &rust_types);
    let code = tokens.to_string();

    assert!(code.contains("serde_json :: to_value")); // Should use fallback
    assert!(code.contains("form . text"));
    assert!(!code.contains("Part :: bytes"));
  }
}

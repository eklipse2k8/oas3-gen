use std::collections::BTreeMap;

use oas3::spec::ParameterStyle;
use proc_macro2::TokenStream;
use quote::{format_ident, quote};

use super::{
  Visibility,
  attributes::{
    generate_deprecated_attr, generate_docs, generate_docs_for_field, generate_outer_attrs, generate_serde_attrs,
    generate_validation_attrs,
  },
  coercion,
};
use crate::generator::ast::{
  FieldDef, PathSegment, QueryParameter, RegexKey, ResponseVariant, RustPrimitive, StructDef, StructMethod,
  StructMethodKind, TypeRef, ValidationAttribute,
};

const UNSET_PREFIX: char = '\0';

pub(crate) fn generate_struct(
  def: &StructDef,
  regex_lookup: &BTreeMap<RegexKey, String>,
  visibility: Visibility,
) -> TokenStream {
  let name = format_ident!("{}", def.name);
  let docs = generate_docs(&def.docs);
  let vis = visibility.to_tokens();

  let derives = super::attributes::generate_derives_from_slice(&def.derives);

  let outer_attrs = generate_outer_attrs(&def.outer_attrs);
  let serde_attrs = generate_serde_attrs(&def.serde_attrs);

  let fields = generate_fields(&def.name, &def.fields, regex_lookup, visibility);

  let struct_tokens = quote! {
    #docs
    #outer_attrs
    #derives
    #serde_attrs
    #vis struct #name {
      #(#fields),*
    }
  };

  if def.methods.is_empty() {
    struct_tokens
  } else {
    let methods: Vec<TokenStream> = def
      .methods
      .iter()
      .map(|m| generate_struct_method(m, visibility))
      .collect();

    quote! {
      #struct_tokens

      impl #name {
        #(#methods)*
      }
    }
  }
}

fn generate_fields(
  type_name: &str,
  fields: &[FieldDef],
  regex_lookup: &BTreeMap<RegexKey, String>,
  visibility: Visibility,
) -> Vec<TokenStream> {
  let vis = visibility.to_tokens();
  fields
    .iter()
    .map(|field| {
      let name = format_ident!("{}", field.name);
      let docs = generate_docs_for_field(field);
      let serde_attrs = generate_serde_attrs(&field.serde_attrs);
      let extra_attrs: Vec<TokenStream> = field
        .extra_attrs
        .iter()
        .filter_map(|attr| attr.parse::<TokenStream>().ok())
        .collect();

      let validation_attrs: Vec<ValidationAttribute> = field
        .validation_attrs
        .iter()
        .map(|attr| match attr {
          ValidationAttribute::Regex(_) => {
            let key = RegexKey::for_struct(type_name, &field.name);
            regex_lookup.get(&key).map_or_else(
              || attr.clone(),
              |const_name| ValidationAttribute::Regex(const_name.clone()),
            )
          }
          _ => attr.clone(),
        })
        .collect();

      let validation_attrs = generate_validation_attrs(&validation_attrs);

      let deprecated_attr = generate_deprecated_attr(field.deprecated);

      let default_attr = if field.default_value.is_some() {
        let default_expr = coercion::json_to_rust_literal(field.default_value.as_ref().unwrap(), &field.rust_type);
        quote! { #[default(#default_expr)] }
      } else {
        quote! {}
      };

      let type_tokens = coercion::parse_type_string(&field.rust_type.to_rust_type());

      quote! {
        #(#extra_attrs)*
        #docs
        #deprecated_attr
        #serde_attrs
        #validation_attrs
        #default_attr
        #vis #name: #type_tokens
      }
    })
    .collect()
}

fn generate_struct_method(method: &StructMethod, visibility: Visibility) -> TokenStream {
  match &method.kind {
    StructMethodKind::ParseResponse {
      response_enum,
      variants,
    } => {
      let docs = generate_docs(&method.docs);
      let attrs = generate_outer_attrs(&method.attrs);
      generate_parse_response_method(response_enum, variants, &docs, &attrs, visibility)
    }
    StructMethodKind::RenderPath { segments, query_params } => {
      generate_render_path_method(method, segments, query_params, visibility)
    }
  }
}

fn generate_render_path_method(
  method: &StructMethod,
  segments: &[PathSegment],
  query_params: &[QueryParameter],
  visibility: Visibility,
) -> TokenStream {
  let name = format_ident!("{}", method.name);
  let docs = generate_docs(&method.docs);
  let attrs = generate_outer_attrs(&method.attrs);
  let vis = visibility.to_tokens();
  let body = build_render_path_body(segments, query_params);

  quote! {
    #docs
    #attrs
    #vis fn #name(&self) -> anyhow::Result<String> {
      #body
    }
  }
}

fn build_render_path_body(segments: &[PathSegment], query_params: &[QueryParameter]) -> TokenStream {
  let path_expr = build_path_expression(segments);
  if query_params.is_empty() {
    quote! { Ok(#path_expr) }
  } else {
    append_query_params(&path_expr, query_params)
  }
}

fn build_path_expression(segments: &[PathSegment]) -> TokenStream {
  let mut format_string = String::new();
  let mut fallback_string = String::new();
  let mut args = vec![];

  for (i, segment) in segments.iter().enumerate() {
    match segment {
      PathSegment::Literal(lit) => {
        // path will be joined with base URL, so skip leading slash for first segment
        let lit_str = if i == 0 && lit.starts_with('/') { &lit[1..] } else { lit };
        let escaped = lit_str.replace('{', "{{").replace('}', "}}");
        format_string.push_str(&escaped);
        fallback_string.push_str(lit_str);
      }
      PathSegment::Parameter { field } => {
        format_string.push_str("{}");
        fallback_string.push_str("{}");
        let ident = format_ident!("{field}");
        args.push(quote! {
          oas3_gen_support::percent_encode_path_segment(&oas3_gen_support::serialize_query_param(&self.#ident)?)
        });
      }
    }
  }

  if args.is_empty() {
    quote! { #fallback_string.to_string() }
  } else {
    quote! { format!(#format_string, #(#args),*) }
  }
}

fn append_query_params(path_expr: &TokenStream, query_params: &[QueryParameter]) -> TokenStream {
  let query_statements: Vec<TokenStream> = query_params.iter().map(generate_query_param_statement).collect();

  quote! {
    use std::fmt::Write as _;
    let mut path = #path_expr;
    let mut prefix = #UNSET_PREFIX;
    #(#query_statements)*
    Ok(path)
  }
}

fn generate_query_param_statement(param: &QueryParameter) -> TokenStream {
  let ident = format_ident!("{}", param.field);
  let key = &param.encoded_name;
  let param_equal = format!("{{prefix}}{key}={{}}");
  let style = param.style.unwrap_or(ParameterStyle::Form);

  // Determine separator for unexploded arrays
  let delimiter = match style {
    ParameterStyle::SpaceDelimited => "%20",
    ParameterStyle::PipeDelimited => "|",
    _ => ",", // Form default
  };

  if param.optional {
    if param.is_array {
      if param.explode {
        // Exploded array: key=val1&key=val2
        quote! {
          if let Some(values) = &self.#ident {
            for value in values {
              prefix = if prefix == #UNSET_PREFIX { '?' } else { '&' };
              write!(&mut path, #param_equal, oas3_gen_support::percent_encode_query_component(&oas3_gen_support::serialize_query_param(value)?)).unwrap();
            }
          }
        }
      } else {
        // Unexploded array: key=val1,val2 (or other delimiter)
        quote! {
          if let Some(values) = &self.#ident && !values.is_empty() {
            prefix = if prefix == #UNSET_PREFIX { '?' } else { '&' };
            let values = values.iter().map(|v| oas3_gen_support::serialize_query_param(v).map(|s| oas3_gen_support::percent_encode_query_component(&s))).collect::<Result<Vec<_>, _>>()?;
            let values = values.join(#delimiter);
            write!(&mut path, #param_equal, values).unwrap();
          }
        }
      }
    } else {
      // Optional scalar
      quote! {
        if let Some(value) = &self.#ident {
          prefix = if prefix == #UNSET_PREFIX { '?' } else { '&' };
          write!(&mut path, #param_equal, oas3_gen_support::percent_encode_query_component(&oas3_gen_support::serialize_query_param(value)?)).unwrap();
        }
      }
    }
  } else if param.is_array {
    if param.explode {
      // Exploded array (required)
      quote! {
        for value in &self.#ident {
          prefix = if prefix == #UNSET_PREFIX { '?' } else { '&' };
          write!(&mut path, #param_equal, oas3_gen_support::percent_encode_query_component(&oas3_gen_support::serialize_query_param(value)?)).unwrap();
        }
      }
    } else {
      // Unexploded array (required)
      quote! {
        if !self.#ident.is_empty() {
          prefix = if prefix == #UNSET_PREFIX { '?' } else { '&' };
          let values = self.#ident.iter().map(|v| oas3_gen_support::serialize_query_param(v).map(|s| oas3_gen_support::percent_encode_query_component(&s))).collect::<Result<Vec<_>, _>>()?;
          let values = values.join(#delimiter);
          write!(&mut path, #param_equal, values).unwrap();
        }
      }
    }
  } else {
    // Required scalar
    quote! {
      prefix = if prefix == #UNSET_PREFIX { '?' } else { '&' };
      write!(&mut path, #param_equal, oas3_gen_support::percent_encode_query_component(&oas3_gen_support::serialize_query_param(&self.#ident)?)).unwrap();
    }
  }
}

fn status_code_condition(status_code: &str) -> TokenStream {
  if status_code.starts_with('2') {
    return quote! { status.is_success() };
  }

  if status_code.to_uppercase().ends_with("XX") {
    let prefix = &status_code[0..1];
    return match prefix {
      "1" => quote! { status.is_informational() },
      "3" => quote! { status.is_redirection() },
      "4" => quote! { status.is_client_error() },
      "5" => quote! { status.is_server_error() },
      _ => quote! { false },
    };
  }

  if let Ok(code) = status_code.parse::<u16>() {
    quote! { status.as_u16() == #code }
  } else {
    quote! { false }
  }
}

fn generate_parse_response_method(
  response_enum: &str,
  variants: &[ResponseVariant],
  docs: &TokenStream,
  attrs: &TokenStream,
  visibility: Visibility,
) -> TokenStream {
  let vis = visibility.to_tokens();
  let response_enum_ident = format_ident!("{response_enum}");

  let (defaults, specifics): (Vec<_>, Vec<_>) = variants.iter().partition(|v| v.status_code == "default");
  let default_variant = defaults.first();

  let status_matches: Vec<TokenStream> = specifics
    .iter()
    .map(|variant| {
      let condition = status_code_condition(&variant.status_code);
      let block = generate_variant_block(
        &response_enum_ident,
        &variant.variant_name,
        variant.schema_type.as_ref(),
        variant.content_type.as_deref(),
        true,
      );
      quote! {
        if #condition {
          #block
        }
      }
    })
    .collect();

  let has_status_checks = !status_matches.is_empty();
  let status_decl = if has_status_checks {
    quote! { let status = req.status(); }
  } else {
    quote! {}
  };

  let fallback = if let Some(default) = default_variant {
    generate_variant_block(
      &response_enum_ident,
      &default.variant_name,
      default.schema_type.as_ref(),
      default.content_type.as_deref(),
      false,
    )
  } else {
    quote! {
      let _ = req.bytes().await?;
      Ok(#response_enum_ident::Unknown)
    }
  };

  quote! {
    #docs
    #attrs
    #vis async fn parse_response(req: reqwest::Response) -> anyhow::Result<#response_enum_ident> {
      #status_decl
      #(#status_matches)*
      #fallback
    }
  }
}

fn generate_variant_block(
  enum_name: &proc_macro2::Ident,
  variant_name: &str,
  schema: Option<&TypeRef>,
  content_type: Option<&str>,
  is_specific_variant: bool,
) -> TokenStream {
  let variant_ident = format_ident!("{variant_name}");

  let variant_expr = if schema.is_some() {
    quote! { #enum_name::#variant_ident(data) }
  } else {
    quote! { #enum_name::#variant_ident }
  };

  let result_statement = if is_specific_variant {
    quote! { return Ok(#variant_expr); }
  } else {
    quote! { Ok(#variant_expr) }
  };

  if let Some(schema_type) = schema {
    let extraction = generate_data_expression(schema_type, content_type);
    quote! {
      let data = #extraction;
      #result_statement
    }
  } else {
    quote! {
      let _ = req.bytes().await?;
      #result_statement
    }
  }
}

fn generate_data_expression(schema_type: &TypeRef, content_type: Option<&str>) -> TokenStream {
  let type_token = coercion::parse_type_string(&schema_type.to_rust_type());
  let ct = content_type.unwrap_or("application/json");

  if ct.contains("json") {
    quote! { oas3_gen_support::Diagnostics::<#type_token>::json_with_diagnostics(req).await? }
  } else if ct.starts_with("text/") {
    if schema_type.is_string_like() {
      quote! { req.text().await? }
    } else if matches!(schema_type.base_type, RustPrimitive::Custom(_)) {
      quote! { oas3_gen_support::Diagnostics::<#type_token>::json_with_diagnostics(req).await? }
    } else {
      quote! { req.text().await?.parse::<#type_token>()? }
    }
  } else if ct.starts_with("image/") || ct == "application/pdf" || schema_type.base_type == RustPrimitive::Bytes {
    quote! { req.bytes().await?.to_vec() }
  } else {
    quote! { oas3_gen_support::Diagnostics::<#type_token>::json_with_diagnostics(req).await? }
  }
}

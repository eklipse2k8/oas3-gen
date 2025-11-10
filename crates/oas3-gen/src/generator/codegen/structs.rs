use std::collections::BTreeMap;

use proc_macro2::TokenStream;
use quote::{format_ident, quote};

use super::{
  Visibility,
  attributes::{
    generate_deprecated_attr, generate_docs, generate_docs_for_field, generate_outer_attrs, generate_serde_attrs,
    generate_validation_attrs,
  },
  coercion,
  constants::RegexKey,
};
use crate::generator::ast::{
  FieldDef, PathSegment, QueryParameter, ResponseVariant, StructDef, StructMethod, StructMethodKind,
};

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

      let regex_const = if field.regex_validation.is_some() {
        let key = RegexKey::for_struct(type_name, &field.name);
        regex_lookup.get(&key).map(std::string::String::as_str)
      } else {
        None
      };

      let validation_attrs = generate_validation_attrs(regex_const, &field.validation_attrs);

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
  let docs = generate_docs(&method.docs);
  let name = format_ident!("{}", method.name);
  let attrs = generate_outer_attrs(&method.attrs);
  let vis = visibility.to_tokens();

  if let StructMethodKind::ParseResponse {
    response_enum,
    variants,
  } = &method.kind
  {
    return generate_parse_response_method(&name, response_enum, variants, &docs, &attrs, visibility);
  }

  let body = match &method.kind {
    StructMethodKind::RenderPath { segments, query_params } => {
      let mut format_string = String::new();
      let mut fallback_string = String::new();
      let mut args: Vec<TokenStream> = Vec::new();

      for segment in segments {
        match segment {
          PathSegment::Literal(lit) => {
            let escaped = lit.replace('{', "{{").replace('}', "}}");
            format_string.push_str(&escaped);
            fallback_string.push_str(lit);
          }
          PathSegment::Parameter { field } => {
            format_string.push_str("{}");
            fallback_string.push_str("{}");
            let ident = format_ident!("{}", field);
            args.push(quote! {
              oas3_gen_support::percent_encode_path_segment(&self.#ident.to_string())
            });
          }
        }
      }

      let path_expr = if args.is_empty() {
        quote! { #fallback_string.to_string() }
      } else {
        let args_tokens = args;
        quote! { format!(#format_string, #(#args_tokens),*) }
      };

      if query_params.is_empty() {
        path_expr
      } else {
        let query_statements: Vec<TokenStream> = query_params.iter().map(generate_query_param_statement).collect();

        quote! {
          use std::fmt::Write as _;
          let mut path = #path_expr;
          let mut prefix = '\0';
          #(#query_statements)*
          path
        }
      }
    }
    StructMethodKind::ParseResponse { .. } => unreachable!("ParseResponse handled above"),
  };

  quote! {
    #docs
    #attrs
    #vis fn #name(&self) -> String {
      #body
    }
  }
}

fn generate_query_param_statement(param: &QueryParameter) -> TokenStream {
  let ident = format_ident!("{}", param.field);
  let key = &param.encoded_name;
  let param_equal = format!("{{prefix}}{key}={{}}");

  if param.optional {
    if param.is_array {
      if param.explode {
        quote! {
          if let Some(values) = &self.#ident {
            for value in values {
              prefix = if prefix == '\0' { '?' } else { '&' };
              write!(&mut path, #param_equal, oas3_gen_support::percent_encode_query_component(&oas3_gen_support::serialize_query_param(value))).unwrap();
            }
          }
        }
      } else {
        quote! {
          if let Some(values) = &self.#ident && !values.is_empty() {
            prefix = if prefix == '\0' { '?' } else { '&' };
            let values = values.iter().map(|v| oas3_gen_support::percent_encode_query_component(&oas3_gen_support::serialize_query_param(v))).collect::<Vec<_>>().join(",");
            write!(&mut path, #param_equal, values).unwrap();
          }
        }
      }
    } else {
      quote! {
        if let Some(value) = &self.#ident {
          prefix = if prefix == '\0' { '?' } else { '&' };
          write!(&mut path, #param_equal, oas3_gen_support::percent_encode_query_component(&oas3_gen_support::serialize_query_param(value))).unwrap();
        }
      }
    }
  } else if param.is_array {
    if param.explode {
      quote! {
        for value in &self.#ident {
          prefix = if prefix == '\0' { '?' } else { '&' };
          write!(&mut path, #param_equal, oas3_gen_support::percent_encode_query_component(&oas3_gen_support::serialize_query_param(value))).unwrap();
        }
      }
    } else {
      quote! {
        if !self.#ident.is_empty() {
          prefix = if prefix == '\0' { '?' } else { '&' };
          let values = self.#ident.iter().map(|v| oas3_gen_support::percent_encode_query_component(&oas3_gen_support::serialize_query_param(v))).collect::<Vec<_>>().join(",");
          write!(&mut path, #param_equal, values).unwrap();
        }
      }
    }
  } else {
    quote! {
      prefix = if prefix == '\0' { '?' } else { '&' };
      write!(&mut path, #param_equal, oas3_gen_support::percent_encode_query_component(&oas3_gen_support::serialize_query_param(&self.#ident))).unwrap();
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
  _name: &proc_macro2::Ident,
  response_enum: &str,
  variants: &[ResponseVariant],
  docs: &TokenStream,
  attrs: &TokenStream,
  visibility: Visibility,
) -> TokenStream {
  let vis = visibility.to_tokens();
  let response_enum_ident = format_ident!("{}", response_enum);

  let mut status_matches: Vec<TokenStream> = Vec::new();
  let mut default_variant: Option<&ResponseVariant> = None;

  for variant in variants {
    if variant.status_code == "default" {
      default_variant = Some(variant);
      continue;
    }

    let variant_name = format_ident!("{}", variant.variant_name);
    let status_code = &variant.status_code;
    let condition = status_code_condition(status_code);

    if let Some(ref schema_type) = variant.schema_type {
      let type_token = coercion::parse_type_string(&schema_type.to_rust_type());
      status_matches.push(quote! {
        if #condition {
          let data = req.json::<#type_token>().await?;
          return Ok(#response_enum_ident::#variant_name(data));
        }
      });
    } else {
      status_matches.push(quote! {
        if #condition {
          let _ = req.bytes().await?;
          return Ok(#response_enum_ident::#variant_name);
        }
      });
    }
  }

  let has_status_checks = !status_matches.is_empty();
  let status_decl = if has_status_checks {
    quote! { let status = req.status(); }
  } else {
    quote! {}
  };

  let fallback = if let Some(default) = default_variant {
    let variant_name = format_ident!("{}", default.variant_name);
    if let Some(ref schema_type) = default.schema_type {
      let type_token = coercion::parse_type_string(&schema_type.to_rust_type());
      quote! {
        let data = req.json::<#type_token>().await?;
        Ok(#response_enum_ident::#variant_name(data))
      }
    } else {
      quote! {
        let _ = req.bytes().await?;
        Ok(#response_enum_ident::#variant_name)
      }
    }
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

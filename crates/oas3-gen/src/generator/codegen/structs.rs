use std::collections::BTreeMap;

use proc_macro2::TokenStream;
use quote::{format_ident, quote};

use super::{
  TypeUsage, Visibility,
  attributes::{
    generate_deprecated_attr, generate_docs, generate_docs_for_field, generate_outer_attrs, generate_serde_attrs,
    generate_validation_attrs,
  },
  coercion,
  constants::RegexKey,
};
use crate::generator::ast::{
  FieldDef, PathSegment, QueryParameter, StructDef, StructKind, StructMethod, StructMethodKind,
};

pub(crate) fn generate_struct(
  def: &StructDef,
  regex_lookup: &BTreeMap<RegexKey, String>,
  type_usage: &BTreeMap<String, TypeUsage>,
  visibility: Visibility,
) -> TokenStream {
  let name = format_ident!("{}", def.name);
  let docs = generate_docs(&def.docs);
  let vis = visibility.to_tokens();

  let derives = match def.kind {
    StructKind::OperationRequest => {
      let mut custom = vec!["Debug".to_string(), "Clone".to_string()];
      custom.push("validator::Validate".to_string());
      custom.push("oas3_gen_support::Default".to_string());
      super::attributes::generate_derives_from_slice(&custom)
    }
    StructKind::RequestBody => {
      let usage = type_usage.get(&def.name).unwrap_or(&TypeUsage::Bidirectional);
      let mut custom = vec!["Debug".to_string(), "Clone".to_string()];
      match usage {
        TypeUsage::RequestOnly => {
          custom.push("Serialize".to_string());
          custom.push("validator::Validate".to_string());
        }
        TypeUsage::ResponseOnly => {
          custom.push("Deserialize".to_string());
        }
        TypeUsage::Bidirectional => {
          custom.push("Serialize".to_string());
          custom.push("Deserialize".to_string());
          custom.push("validator::Validate".to_string());
        }
      }
      custom.push("oas3_gen_support::Default".to_string());
      super::attributes::generate_derives_from_slice(&custom)
    }
    StructKind::Schema => {
      let mut derives = def.derives.clone();
      if let Some(usage) = type_usage.get(&def.name) {
        match usage {
          TypeUsage::RequestOnly => {
            ensure_derive(&mut derives, "Serialize");
            ensure_derive(&mut derives, "validator::Validate");
          }
          TypeUsage::ResponseOnly => {
            ensure_derive(&mut derives, "Deserialize");
          }
          TypeUsage::Bidirectional => {
            ensure_derive(&mut derives, "Serialize");
            ensure_derive(&mut derives, "Deserialize");
            ensure_derive(&mut derives, "validator::Validate");
          }
        }
      }
      super::attributes::generate_derives_from_slice(&derives)
    }
  };

  let outer_attrs = generate_outer_attrs(&def.outer_attrs);
  let serde_attrs = generate_serde_attrs(&def.serde_attrs);

  let include_validation = match def.kind {
    StructKind::RequestBody => !matches!(type_usage.get(&def.name), Some(TypeUsage::ResponseOnly)),
    StructKind::OperationRequest | StructKind::Schema => true,
  };
  let fields = generate_fields(&def.name, &def.fields, include_validation, regex_lookup, visibility);

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

fn ensure_derive(derives: &mut Vec<String>, trait_name: &str) {
  if !derives.iter().any(|existing| existing == trait_name) {
    derives.push(trait_name.to_string());
  }
}

fn generate_fields(
  type_name: &str,
  fields: &[FieldDef],
  include_validation: bool,
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

      let regex_const = if include_validation && field.regex_validation.is_some() {
        let key = RegexKey::for_struct(type_name, &field.name);
        regex_lookup.get(&key).map(std::string::String::as_str)
      } else {
        None
      };

      let validation_attrs = if include_validation {
        generate_validation_attrs(regex_const, &field.validation_attrs)
      } else {
        quote! {}
      };

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

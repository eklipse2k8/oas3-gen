use std::collections::{BTreeMap, BTreeSet};

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
use crate::generator::{
  ast::{
    ContentCategory, DerivesProvider, FieldDef, PathSegment, QueryParameter, RegexKey, ResolvedLink, ResponseVariant,
    ResponseVariantLinks, RuntimeExpression, RustPrimitive, StatusCodeToken, StructDef, StructMethod, StructMethodKind,
    StructToken, TypeRef, ValidationAttribute,
    tokens::{ConstToken, EnumToken, EnumVariantToken, MethodNameToken},
  },
  converter::runtime_expression,
  naming::identifiers::to_rust_field_name,
};

const QUERY_PREFIX_UNSET: char = '\0';
const QUERY_PREFIX_FIRST: char = '?';
const QUERY_PREFIX_SUBSEQUENT: char = '&';

pub(crate) struct StructGenerator<'a> {
  regex_lookup: &'a BTreeMap<RegexKey, ConstToken>,
  visibility: Visibility,
}

impl<'a> StructGenerator<'a> {
  pub(crate) fn new(regex_lookup: &'a BTreeMap<RegexKey, ConstToken>, visibility: Visibility) -> Self {
    Self {
      regex_lookup,
      visibility,
    }
  }

  pub(crate) fn generate_links_struct(&self, links: &ResponseVariantLinks) -> TokenStream {
    let name = format_ident!("{}", links.links_struct_name);
    let vis = self.visibility.to_tokens();

    let fields: Vec<TokenStream> = links
      .resolved_links
      .iter()
      .map(|link| {
        let field_name = format_ident!("{}", to_rust_field_name(&link.link_def.name));
        let target_type = &link.target_request_type;
        let doc = link
          .link_def
          .description
          .as_ref()
          .map(|d| quote! { #[doc = #d] })
          .unwrap_or_default();

        quote! {
          #doc
          #vis #field_name: Option<#target_type>
        }
      })
      .collect();

    quote! {
      #[derive(Debug, Clone, Default)]
      #vis struct #name {
        #(#fields),*
      }
    }
  }

  pub(crate) fn generate(&self, def: &StructDef) -> TokenStream {
    let name = format_ident!("{}", def.name);
    let docs = generate_docs(&def.docs);
    let vis = self.visibility.to_tokens();

    let derives = super::attributes::generate_derives_from_slice(&def.derives());

    let outer_attrs = generate_outer_attrs(&def.outer_attrs);
    let serde_attrs = generate_serde_attrs(&def.serde_attrs);

    let fields = self.generate_fields(&def.name, &def.fields);

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
      let methods: Vec<TokenStream> = def.methods.iter().map(|m| self.generate_method(m)).collect();

      quote! {
        #struct_tokens

        impl #name {
          #(#methods)*
        }
      }
    }
  }

  fn generate_fields(&self, type_name: &StructToken, fields: &[FieldDef]) -> Vec<TokenStream> {
    let vis = self.visibility.to_tokens();
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
              let key = RegexKey::for_struct(type_name, field.name.as_str());
              self.regex_lookup.get(&key).map_or_else(
                || attr.clone(),
                |const_token| ValidationAttribute::Regex(const_token.to_string()),
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

        let type_tokens = &field.rust_type;

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

  fn generate_method(&self, method: &StructMethod) -> TokenStream {
    match &method.kind {
      StructMethodKind::ParseResponse {
        response_enum,
        variants,
      } => {
        let docs = generate_docs(&method.docs);
        let attrs = generate_outer_attrs(&method.attrs);
        self.generate_parse_response_method(&method.name, response_enum, variants, &docs, &attrs)
      }
      StructMethodKind::RenderPath { segments, query_params } => {
        self.generate_render_path_method(method, segments, query_params)
      }
    }
  }

  fn generate_render_path_method(
    &self,
    method: &StructMethod,
    segments: &[PathSegment],
    query_params: &[QueryParameter],
  ) -> TokenStream {
    let name = &method.name;
    let docs = generate_docs(&method.docs);
    let attrs = generate_outer_attrs(&method.attrs);
    let vis = self.visibility.to_tokens();
    let body = Self::build_render_path_body(segments, query_params);

    quote! {
      #docs
      #attrs
      #vis fn #name(&self) -> anyhow::Result<String> {
        #body
      }
    }
  }

  fn generate_parse_response_method(
    &self,
    method_name: &MethodNameToken,
    response_enum: &EnumToken,
    variants: &[ResponseVariant],
    docs: &TokenStream,
    attrs: &TokenStream,
  ) -> TokenStream {
    let vis = self.visibility.to_tokens();

    let (defaults, specifics): (Vec<_>, Vec<_>) = variants.iter().partition(|v| v.status_code.is_default());
    let default_variant = defaults.first();

    let status_matches: Vec<TokenStream> = specifics
      .iter()
      .map(|variant| {
        let condition = Self::status_code_condition(variant.status_code);
        let variant_token = &variant.variant_name;
        let block = Self::generate_variant_block(
          response_enum,
          variant_token,
          variant.schema_type.as_ref(),
          variant.content_category,
          variant.links.as_ref(),
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
      quote! { let status = response.status(); }
    } else {
      quote! {}
    };

    let fallback = if let Some(default) = default_variant {
      Self::generate_variant_block(
        response_enum,
        &default.variant_name,
        default.schema_type.as_ref(),
        default.content_category,
        default.links.as_ref(),
        false,
      )
    } else {
      let unknown_variant = EnumVariantToken::from("Unknown");
      quote! {
        let _ = response.bytes().await?;
        Ok(#response_enum::#unknown_variant)
      }
    };

    quote! {
      #docs
      #attrs
      #vis async fn #method_name(self, response: reqwest::Response) -> anyhow::Result<#response_enum> {
        #status_decl
        #(#status_matches)*
        #fallback
      }
    }
  }

  fn build_render_path_body(segments: &[PathSegment], query_params: &[QueryParameter]) -> TokenStream {
    let path_expr = Self::build_path_expression(segments);
    if query_params.is_empty() {
      quote! { Ok(#path_expr) }
    } else {
      Self::append_query_params(&path_expr, query_params)
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
          let ident = field;
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
    let query_statements: Vec<TokenStream> = query_params.iter().map(Self::generate_query_param_statement).collect();

    quote! {
      use std::fmt::Write as _;
      let mut path = #path_expr;
      let mut prefix = #QUERY_PREFIX_UNSET;
      #(#query_statements)*
      Ok(path)
    }
  }

  fn advance_query_prefix() -> TokenStream {
    quote! {
      prefix = if prefix == #QUERY_PREFIX_UNSET { #QUERY_PREFIX_FIRST } else { #QUERY_PREFIX_SUBSEQUENT };
    }
  }

  fn write_single_query_value(format_str: &str, value_expr: &TokenStream) -> TokenStream {
    let format_lit: TokenStream = format_str.parse().unwrap();
    quote! {
      write!(&mut path, #format_lit, oas3_gen_support::percent_encode_query_component(&oas3_gen_support::serialize_query_param(#value_expr)?)).unwrap();
    }
  }

  fn write_joined_query_values(format_str: &str, values_expr: &TokenStream, delimiter: &str) -> TokenStream {
    let format_lit: TokenStream = format_str.parse().unwrap();
    quote! {
      let values = #values_expr.iter().map(|v| oas3_gen_support::serialize_query_param(v).map(|s| oas3_gen_support::percent_encode_query_component(&s))).collect::<Result<Vec<_>, _>>()?;
      let values = values.join(#delimiter);
      write!(&mut path, #format_lit, values).unwrap();
    }
  }

  fn generate_query_param_statement(param: &QueryParameter) -> TokenStream {
    let ident = &param.field;
    let format_str = format!("\"{{prefix}}{}={{}}\"", param.encoded_name);
    let style = param.style.unwrap_or(ParameterStyle::Form);
    let delimiter = match style {
      ParameterStyle::SpaceDelimited => "%20",
      ParameterStyle::PipeDelimited => "|",
      _ => ",",
    };
    let advance_prefix = Self::advance_query_prefix();

    match (param.optional, param.is_array, param.explode) {
      (true, true, true) => {
        let value_expr = quote! { value };
        let write_value = Self::write_single_query_value(&format_str, &value_expr);
        quote! {
          if let Some(values) = &self.#ident {
            for value in values {
              #advance_prefix
              #write_value
            }
          }
        }
      }
      (true, true, false) => {
        let values_expr = quote! { values };
        let write_joined = Self::write_joined_query_values(&format_str, &values_expr, delimiter);
        quote! {
          if let Some(values) = &self.#ident && !values.is_empty() {
            #advance_prefix
            #write_joined
          }
        }
      }
      (true, false, _) => {
        let value_expr = quote! { value };
        let write_value = Self::write_single_query_value(&format_str, &value_expr);
        quote! {
          if let Some(value) = &self.#ident {
            #advance_prefix
            #write_value
          }
        }
      }
      (false, true, true) => {
        let value_expr = quote! { value };
        let write_value = Self::write_single_query_value(&format_str, &value_expr);
        quote! {
          for value in &self.#ident {
            #advance_prefix
            #write_value
          }
        }
      }
      (false, true, false) => {
        let values_expr = quote! { &self.#ident };
        let write_joined = Self::write_joined_query_values(&format_str, &values_expr, delimiter);
        quote! {
          if !self.#ident.is_empty() {
            #advance_prefix
            #write_joined
          }
        }
      }
      (false, false, _) => {
        let value_expr = quote! { &self.#ident };
        let write_value = Self::write_single_query_value(&format_str, &value_expr);
        quote! {
          #advance_prefix
          #write_value
        }
      }
    }
  }

  fn status_code_condition(status_code: StatusCodeToken) -> TokenStream {
    if status_code.is_success() {
      return quote! { status.is_success() };
    }
    if status_code.is_default() {
      return quote! { true };
    }
    match status_code {
      StatusCodeToken::Informational1XX => quote! { status.is_informational() },
      StatusCodeToken::Redirection3XX => quote! { status.is_redirection() },
      StatusCodeToken::ClientError4XX => quote! { status.is_client_error() },
      StatusCodeToken::ServerError5XX => quote! { status.is_server_error() },
      other => other
        .code()
        .map_or_else(|| quote! { false }, |code| quote! { status.as_u16() == #code }),
    }
  }

  fn generate_variant_block(
    enum_token: &EnumToken,
    variant_token: &EnumVariantToken,
    schema: Option<&TypeRef>,
    content_category: ContentCategory,
    links: Option<&ResponseVariantLinks>,
    is_specific_variant: bool,
  ) -> TokenStream {
    let has_links = links.is_some();
    let variant_expr = match (schema.is_some(), has_links) {
      (true, true) => quote! { #enum_token::#variant_token(data, links) },
      (true, false) => quote! { #enum_token::#variant_token(data) },
      (false, true) => quote! { #enum_token::#variant_token(links) },
      (false, false) => quote! { #enum_token::#variant_token },
    };

    let result_statement = if is_specific_variant {
      quote! { return Ok(#variant_expr); }
    } else {
      quote! { Ok(#variant_expr) }
    };

    let links_construction = if let Some(links_info) = links {
      Self::generate_links_construction(links_info)
    } else {
      quote! {}
    };

    if let Some(schema_type) = schema {
      let extraction = Self::generate_data_expression(schema_type, content_category);
      quote! {
        let data = #extraction;
        #links_construction
        #result_statement
      }
    } else if has_links {
      quote! {
        let _ = response.bytes().await?;
        #links_construction
        #result_statement
      }
    } else {
      quote! {
        let _ = response.bytes().await?;
        #result_statement
      }
    }
  }

  fn generate_data_expression(schema_type: &TypeRef, content_category: ContentCategory) -> TokenStream {
    let type_token = schema_type;

    match content_category {
      ContentCategory::Text => {
        if schema_type.is_string_like() {
          quote! { response.text().await? }
        } else if matches!(schema_type.base_type, RustPrimitive::Custom(_)) {
          quote! { oas3_gen_support::Diagnostics::<#type_token>::json_with_diagnostics(response).await? }
        } else {
          quote! { response.text().await?.parse::<#type_token>()? }
        }
      }
      ContentCategory::Binary => {
        if matches!(schema_type.base_type, RustPrimitive::Custom(_)) {
          quote! { oas3_gen_support::Diagnostics::<#type_token>::json_with_diagnostics(response).await? }
        } else {
          quote! { response.bytes().await?.to_vec() }
        }
      }
      ContentCategory::Json | ContentCategory::Xml | ContentCategory::FormUrlEncoded | ContentCategory::Multipart => {
        quote! { oas3_gen_support::Diagnostics::<#type_token>::json_with_diagnostics(response).await? }
      }
    }
  }

  fn generate_links_construction(links: &ResponseVariantLinks) -> TokenStream {
    let links_struct_name = &links.links_struct_name;
    let response_body_fields = &links.response_body_fields;

    let field_assignments: Vec<TokenStream> = links
      .resolved_links
      .iter()
      .map(|link| {
        let field_name = format_ident!("{}", to_rust_field_name(&link.link_def.name));
        let target_type = &link.target_request_type;

        let param_assignments = Self::generate_link_param_assignments(link, response_body_fields);

        if param_assignments.is_empty() {
          quote! { #field_name: None }
        } else {
          let (guards, assignments): (Vec<_>, Vec<_>) = param_assignments.into_iter().unzip();
          let combined_guard = Self::combine_option_guards(&guards);

          quote! {
            #field_name: #combined_guard.map(|_| #target_type {
              #(#assignments,)*
            })
          }
        }
      })
      .collect();

    quote! {
      let links = #links_struct_name {
        #(#field_assignments,)*
      };
    }
  }

  fn generate_link_param_assignments(
    link: &ResolvedLink,
    response_body_fields: &BTreeSet<String>,
  ) -> Vec<(TokenStream, TokenStream)> {
    link
      .link_def
      .parameters
      .iter()
      .filter_map(|(param_name, expr)| {
        let target_field = format_ident!("{}", to_rust_field_name(param_name));
        Self::generate_param_source(expr, response_body_fields)
          .map(|(guard, value)| (guard, quote! { #target_field: #value }))
      })
      .collect()
  }

  fn generate_param_source(
    expr: &RuntimeExpression,
    response_body_fields: &BTreeSet<String>,
  ) -> Option<(TokenStream, TokenStream)> {
    match expr {
      RuntimeExpression::ResponseBodyPath { json_pointer } => {
        let field_path = runtime_expression::json_pointer_to_field_path(json_pointer);
        if field_path.is_empty() {
          return None;
        }

        let first_field = &field_path[0];
        if !response_body_fields.contains(first_field) {
          return None;
        }

        let (accessor, has_array_index) = Self::build_json_pointer_accessor(&field_path, quote! { data });

        if has_array_index {
          Some((accessor.clone(), quote! { #accessor.cloned().unwrap_or_default() }))
        } else {
          Some((
            quote! { #accessor.as_ref() },
            quote! { #accessor.clone().unwrap_or_default() },
          ))
        }
      }
      RuntimeExpression::RequestPathParam { name } => {
        let field = format_ident!("{}", to_rust_field_name(name));
        Some((quote! { Some(&()) }, quote! { self.#field.clone() }))
      }
      RuntimeExpression::RequestQueryParam { name } | RuntimeExpression::RequestHeader { name } => {
        let field = format_ident!("{}", to_rust_field_name(name));
        Some((
          quote! { self.#field.as_ref() },
          quote! { self.#field.clone().unwrap_or_default() },
        ))
      }
      RuntimeExpression::RequestBody { json_pointer } => {
        if let Some(pointer) = json_pointer {
          let field_path = runtime_expression::json_pointer_to_field_path(pointer);
          if field_path.is_empty() {
            return Some((
              quote! { self.body.as_ref() },
              quote! { self.body.clone().unwrap_or_default() },
            ));
          }
          let field_idents: Vec<_> = field_path
            .iter()
            .map(|f| format_ident!("{}", to_rust_field_name(f)))
            .collect();
          let first = &field_idents[0];

          let accessor = if field_idents.len() == 1 {
            quote! { self.body.as_ref().and_then(|b| b.#first.as_ref()) }
          } else {
            let rest = &field_idents[1..];
            quote! { self.body.as_ref().and_then(|b| b.#first #(.#rest)*.as_ref()) }
          };

          Some((accessor.clone(), quote! { #accessor.cloned().unwrap_or_default() }))
        } else {
          Some((
            quote! { self.body.as_ref() },
            quote! { self.body.clone().unwrap_or_default() },
          ))
        }
      }
      RuntimeExpression::Literal { value } => Some((quote! { Some(&()) }, quote! { #value.to_string() })),
      RuntimeExpression::Unsupported => None,
    }
  }

  fn build_json_pointer_accessor(field_path: &[String], base: TokenStream) -> (TokenStream, bool) {
    let mut accessor = base;
    let mut has_array_index = false;

    for segment in field_path {
      if let Ok(index) = segment.parse::<usize>() {
        has_array_index = true;
        accessor = quote! { #accessor.get(#index) };
      } else {
        let field_ident = format_ident!("{}", to_rust_field_name(segment));
        if has_array_index {
          accessor = quote! { #accessor.and_then(|v| Some(&v.#field_ident)) };
        } else {
          accessor = quote! { #accessor.#field_ident };
        }
      }
    }

    (accessor, has_array_index)
  }

  fn combine_option_guards(guards: &[TokenStream]) -> TokenStream {
    if guards.is_empty() {
      return quote! { None::<()> };
    }
    if guards.len() == 1 {
      let guard = &guards[0];
      return quote! { #guard };
    }

    let first = &guards[0];
    let rest = &guards[1..];
    quote! { #first #(.and_then(|_| #rest))* }
  }
}

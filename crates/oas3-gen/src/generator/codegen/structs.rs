use std::collections::BTreeMap;

use proc_macro2::TokenStream;
use quote::{format_ident, quote};

use super::{
  Visibility,
  attributes::{
    generate_deprecated_attr, generate_docs_for_field, generate_outer_attrs, generate_serde_as_attr,
    generate_serde_attrs, generate_validation_attrs,
  },
  coercion,
};
use crate::generator::ast::{
  ContentCategory, DerivesProvider, Documentation, FieldDef, RegexKey, ResponseMediaType, ResponseVariant,
  RustPrimitive, StatusCodeToken, StructDef, StructMethod, StructMethodKind, StructToken, TypeRef, ValidationAttribute,
  tokens::{ConstToken, EnumToken, EnumVariantToken, MethodNameToken},
};

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

  pub(crate) fn generate(&self, def: &StructDef) -> TokenStream {
    let name = format_ident!("{}", def.name);
    let docs = &def.docs;
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
        let serde_as_attr = generate_serde_as_attr(field.serde_as_attr.as_ref());
        let serde_attrs = generate_serde_attrs(&field.serde_attrs);
        let doc_hidden_attr = if field.doc_hidden {
          quote! { #[doc(hidden)] }
        } else {
          quote! {}
        };

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
          #doc_hidden_attr
          #docs
          #deprecated_attr
          #serde_as_attr
          #serde_attrs
          #validation_attrs
          #default_attr
          #vis #name: #type_tokens
        }
      })
      .collect()
  }

  fn generate_method(&self, method: &StructMethod) -> TokenStream {
    let StructMethodKind::ParseResponse {
      response_enum,
      variants,
    } = &method.kind;
    let docs = &method.docs;
    self.generate_parse_response_method(&method.name, response_enum, variants, docs)
  }

  fn generate_parse_response_method(
    &self,
    method_name: &MethodNameToken,
    response_enum: &EnumToken,
    variants: &[ResponseVariant],
    docs: &Documentation,
  ) -> TokenStream {
    let vis = self.visibility.to_tokens();

    let (defaults, specifics): (Vec<_>, Vec<_>) = variants.iter().partition(|v| v.status_code.is_default());
    let default_variant = defaults.first();

    let grouped = Self::group_variants_by_status_code(&specifics);

    let status_matches: Vec<TokenStream> = grouped
      .iter()
      .map(|(status_code, group)| {
        let condition = Self::status_code_condition(*status_code);
        let block = Self::generate_grouped_variant_block(response_enum, group);

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
      Self::generate_variant_block_for_response(response_enum, default, false)
    } else {
      let unknown_variant = EnumVariantToken::from("Unknown");
      quote! {
        let _ = req.bytes().await?;
        Ok(#response_enum::#unknown_variant)
      }
    };

    quote! {
      #docs
      #vis async fn #method_name(req: reqwest::Response) -> anyhow::Result<#response_enum> {
        #status_decl
        #(#status_matches)*
        #fallback
      }
    }
  }

  fn group_variants_by_status_code<'b>(
    variants: &[&'b ResponseVariant],
  ) -> Vec<(StatusCodeToken, Vec<&'b ResponseVariant>)> {
    use indexmap::IndexMap;
    let mut grouped: IndexMap<StatusCodeToken, Vec<&'b ResponseVariant>> = IndexMap::new();
    for variant in variants {
      grouped.entry(variant.status_code).or_default().push(variant);
    }
    grouped.into_iter().collect()
  }

  fn generate_grouped_variant_block(response_enum: &EnumToken, variants: &[&ResponseVariant]) -> TokenStream {
    if variants.len() == 1 {
      return Self::generate_variant_block_for_response(response_enum, variants[0], true);
    }

    let event_stream_variant = variants
      .iter()
      .find(|v| ResponseMediaType::has_event_stream(&v.media_types));
    let non_stream_variant = variants
      .iter()
      .find(|v| !ResponseMediaType::has_event_stream(&v.media_types));

    match (non_stream_variant, event_stream_variant) {
      (Some(json_var), Some(stream_var)) => {
        let json_variant_name = &json_var.variant_name;
        let stream_variant_name = &stream_var.variant_name;

        let json_schema = json_var.schema_type.as_ref();
        let stream_schema = stream_var.schema_type.as_ref();

        let json_block = if let Some(schema) = json_schema {
          let content_category = ResponseMediaType::primary_category(&json_var.media_types);
          let data_expr = Self::generate_data_expression(schema, content_category);
          quote! {
            let data = #data_expr;
            return Ok(#response_enum::#json_variant_name(data));
          }
        } else {
          quote! {
            let _ = req.bytes().await?;
            return Ok(#response_enum::#json_variant_name);
          }
        };

        let stream_block = if let Some(_schema) = stream_schema {
          quote! {
            let data = oas3_gen_support::EventStream::from_response(req);
            return Ok(#response_enum::#stream_variant_name(data));
          }
        } else {
          quote! {
            let _ = req.bytes().await?;
            return Ok(#response_enum::#stream_variant_name);
          }
        };

        quote! {
          let content_type_str = req.headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("application/json");
          if content_type_str.contains("event-stream") {
            #stream_block
          }
          #json_block
        }
      }
      _ => Self::generate_variant_block_for_response(response_enum, variants[0], true),
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

  fn generate_variant_block_for_response(
    response_enum: &EnumToken,
    variant: &ResponseVariant,
    is_specific_variant: bool,
  ) -> TokenStream {
    let variant_token = &variant.variant_name;

    let Some(schema_type) = &variant.schema_type else {
      return Self::generate_simple_variant_block(
        response_enum,
        variant_token,
        None,
        ContentCategory::Json,
        is_specific_variant,
      );
    };

    let has_event_stream = ResponseMediaType::has_event_stream(&variant.media_types);
    let has_multiple_content_types = Self::has_multiple_parse_strategies(&variant.media_types);

    if has_event_stream {
      Self::generate_event_stream_block(response_enum, variant_token, schema_type, is_specific_variant)
    } else if has_multiple_content_types {
      Self::generate_content_type_dispatch(
        response_enum,
        variant_token,
        schema_type,
        &variant.media_types,
        is_specific_variant,
      )
    } else {
      let content_category = ResponseMediaType::primary_category(&variant.media_types);
      Self::generate_simple_variant_block(
        response_enum,
        variant_token,
        Some(schema_type),
        content_category,
        is_specific_variant,
      )
    }
  }

  fn has_multiple_parse_strategies(media_types: &[ResponseMediaType]) -> bool {
    if media_types.len() <= 1 {
      return false;
    }
    let categories: std::collections::HashSet<_> = media_types
      .iter()
      .filter(|m| m.schema_type.is_some())
      .filter(|m| m.category != ContentCategory::EventStream)
      .map(|m| m.category)
      .collect();
    categories.len() > 1
  }

  fn generate_event_stream_block(
    enum_token: &EnumToken,
    variant_token: &EnumVariantToken,
    _schema_type: &TypeRef,
    is_specific_variant: bool,
  ) -> TokenStream {
    let variant_expr = quote! { #enum_token::#variant_token(data) };
    let result_statement = if is_specific_variant {
      quote! { return Ok(#variant_expr); }
    } else {
      quote! { Ok(#variant_expr) }
    };

    quote! {
      let data = oas3_gen_support::EventStream::from_response(req);
      #result_statement
    }
  }

  fn generate_simple_variant_block(
    enum_token: &EnumToken,
    variant_token: &EnumVariantToken,
    schema: Option<&TypeRef>,
    content_category: ContentCategory,
    is_specific_variant: bool,
  ) -> TokenStream {
    let variant_expr = if schema.is_some() {
      quote! { #enum_token::#variant_token(data) }
    } else {
      quote! { #enum_token::#variant_token }
    };

    let result_statement = if is_specific_variant {
      quote! { return Ok(#variant_expr); }
    } else {
      quote! { Ok(#variant_expr) }
    };

    if let Some(schema_type) = schema {
      let extraction = Self::generate_data_expression(schema_type, content_category);
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

  fn generate_content_type_dispatch(
    response_enum: &EnumToken,
    variant_token: &EnumVariantToken,
    schema_type: &TypeRef,
    media_types: &[ResponseMediaType],
    is_specific_variant: bool,
  ) -> TokenStream {
    let variant_expr = quote! { #response_enum::#variant_token(data) };
    let result_statement = if is_specific_variant {
      quote! { return Ok(#variant_expr); }
    } else {
      quote! { Ok(#variant_expr) }
    };

    let typed_media_types: Vec<_> = media_types
      .iter()
      .filter(|m| m.schema_type.is_some())
      .filter(|m| m.category != ContentCategory::EventStream)
      .collect();

    if typed_media_types.is_empty() {
      return quote! {
        let _ = req.bytes().await?;
        #result_statement
      };
    }

    let mut branches: Vec<TokenStream> = vec![];

    for (idx, media_type) in typed_media_types.iter().enumerate() {
      let category = media_type.category;
      let check = Self::generate_content_type_check(category);
      let data_expr = Self::generate_data_expression(schema_type, category);

      let extraction = quote! {
        let data = #data_expr;
        #result_statement
      };

      if idx == 0 {
        branches.push(quote! {
          if #check {
            #extraction
          }
        });
      } else {
        branches.push(quote! {
          else if #check {
            #extraction
          }
        });
      }
    }

    let primary_category = typed_media_types.first().map_or(ContentCategory::Json, |m| m.category);
    let data_expr = Self::generate_data_expression(schema_type, primary_category);
    let trailing = quote! {
      let data = #data_expr;
      #result_statement
    };

    quote! {
      let content_type_str = req.headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("application/json");
      #(#branches)*
      #trailing
    }
  }

  fn generate_content_type_check(category: ContentCategory) -> TokenStream {
    match category {
      ContentCategory::Json => quote! { content_type_str.contains("json") },
      ContentCategory::Xml => quote! { content_type_str.contains("xml") },
      ContentCategory::Text => quote! { content_type_str.starts_with("text/") && !content_type_str.contains("xml") },
      ContentCategory::Binary => {
        quote! { content_type_str.starts_with("application/octet-stream") || content_type_str.starts_with("image/") || content_type_str.starts_with("audio/") || content_type_str.starts_with("video/") }
      }
      ContentCategory::EventStream => quote! { content_type_str.contains("event-stream") },
      ContentCategory::FormUrlEncoded => quote! { content_type_str.contains("x-www-form-urlencoded") },
      ContentCategory::Multipart => quote! { content_type_str.contains("multipart") },
    }
  }

  fn generate_data_expression(schema_type: &TypeRef, content_category: ContentCategory) -> TokenStream {
    let type_token = schema_type;

    match content_category {
      ContentCategory::Text => {
        if schema_type.is_string_like() {
          quote! { req.text().await? }
        } else if matches!(schema_type.base_type, RustPrimitive::Custom(_)) {
          quote! { oas3_gen_support::Diagnostics::<#type_token>::json_with_diagnostics(req).await? }
        } else {
          quote! { req.text().await?.parse::<#type_token>()? }
        }
      }
      ContentCategory::Binary => {
        if matches!(schema_type.base_type, RustPrimitive::Custom(_)) {
          quote! { oas3_gen_support::Diagnostics::<#type_token>::json_with_diagnostics(req).await? }
        } else {
          quote! { req.bytes().await?.to_vec() }
        }
      }
      ContentCategory::EventStream => {
        quote! { oas3_gen_support::EventStream::<#type_token>::from_response(req) }
      }
      ContentCategory::Xml => {
        quote! { oas3_gen_support::Diagnostics::<#type_token>::xml_with_diagnostics(req).await? }
      }
      ContentCategory::Json | ContentCategory::FormUrlEncoded | ContentCategory::Multipart => {
        quote! { oas3_gen_support::Diagnostics::<#type_token>::json_with_diagnostics(req).await? }
      }
    }
  }
}

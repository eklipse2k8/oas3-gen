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
  ContentCategory, DerivesProvider, Documentation, FieldDef, RegexKey, ResponseVariant, RustPrimitive, StatusCodeToken,
  StructDef, StructMethod, StructMethodKind, StructToken, TypeRef, ValidationAttribute,
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
      Self::generate_variant_block(
        response_enum,
        &default.variant_name,
        default.schema_type.as_ref(),
        default.content_category,
        false,
      )
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
      ContentCategory::Json | ContentCategory::Xml | ContentCategory::FormUrlEncoded | ContentCategory::Multipart => {
        quote! { oas3_gen_support::Diagnostics::<#type_token>::json_with_diagnostics(req).await? }
      }
    }
  }
}

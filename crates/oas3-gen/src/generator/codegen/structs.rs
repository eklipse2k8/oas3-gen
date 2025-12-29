use std::collections::{BTreeMap, BTreeSet};

use indexmap::IndexMap;
use proc_macro2::TokenStream;
use quote::{ToTokens, format_ident, quote};

use super::{
  Visibility,
  attributes::{
    generate_deprecated_attr, generate_doc_hidden_attr, generate_docs_for_field, generate_field_default_attr,
    generate_outer_attrs, generate_serde_as_attr, generate_serde_attrs, generate_validation_attrs,
  },
};
use crate::generator::ast::{
  BuilderField, BuilderNestedStruct, ContentCategory, DerivesProvider, FieldDef, RegexKey, ResponseMediaType,
  ResponseVariant, RustPrimitive, StatusCodeToken, StructDef, StructMethod, StructMethodKind, StructToken, TypeRef,
  ValidationAttribute,
  tokens::{ConstToken, EnumToken, EnumVariantToken},
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
    let struct_def = self.generate_struct_definition(def);
    let impl_block = self.generate_impl_block(def);

    quote! {
      #struct_def
      #impl_block
    }
  }

  fn generate_struct_definition(&self, def: &StructDef) -> TokenStream {
    let name = format_ident!("{}", def.name);
    let docs = &def.docs;
    let vis = self.visibility.to_tokens();

    let derives = super::attributes::generate_derives_from_slice(&def.derives());
    let outer_attrs = generate_outer_attrs(&def.outer_attrs);
    let serde_attrs = generate_serde_attrs(&def.serde_attrs);
    let fields = self.generate_fields(&def.name, &def.fields);

    quote! {
      #docs
      #outer_attrs
      #derives
      #serde_attrs
      #vis struct #name {
        #(#fields),*
      }
    }
  }

  fn generate_fields(&self, struct_name: &StructToken, fields: &[FieldDef]) -> Vec<TokenStream> {
    fields
      .iter()
      .map(|field| self.generate_single_field(struct_name, field))
      .collect()
  }

  fn generate_single_field(&self, struct_name: &StructToken, field: &FieldDef) -> TokenStream {
    let name = format_ident!("{}", field.name);
    let docs = generate_docs_for_field(field);
    let vis = self.visibility.to_tokens();
    let type_tokens = &field.rust_type;

    let serde_as = generate_serde_as_attr(field.serde_as_attr.as_ref());
    let serde_attrs = generate_serde_attrs(&field.serde_attrs);
    let validation = self.resolve_validation_attrs(struct_name, field);
    let deprecated = generate_deprecated_attr(field.deprecated);
    let default_val = generate_field_default_attr(field);
    let doc_hidden = generate_doc_hidden_attr(field.doc_hidden);

    quote! {
      #doc_hidden
      #docs
      #deprecated
      #serde_as
      #serde_attrs
      #validation
      #default_val
      #vis #name: #type_tokens
    }
  }

  fn resolve_validation_attrs(&self, struct_name: &StructToken, field: &FieldDef) -> TokenStream {
    let attrs: Vec<ValidationAttribute> = field
      .validation_attrs
      .iter()
      .map(|attr| match attr {
        ValidationAttribute::Regex(_) => {
          let key = RegexKey::for_struct(struct_name, field.name.as_str());
          self.regex_lookup.get(&key).map_or_else(
            || attr.clone(),
            |const_token| ValidationAttribute::Regex(const_token.to_string()),
          )
        }
        _ => attr.clone(),
      })
      .collect();

    generate_validation_attrs(&attrs)
  }

  fn generate_impl_block(&self, def: &StructDef) -> TokenStream {
    if def.methods.is_empty() {
      return quote! {};
    }

    let name = &def.name;

    let (builder_methods, other_methods): (Vec<_>, Vec<_>) = def
      .methods
      .iter()
      .partition(|m| matches!(m.kind, StructMethodKind::Builder { .. }));

    let mut result = quote! {};

    if !builder_methods.is_empty() {
      let methods: Vec<TokenStream> = builder_methods.iter().map(|m| self.generate_method(m)).collect();
      result = quote! {
        #result
        #[bon::bon]
        impl #name {
          #(#methods)*
        }
      };
    }

    if !other_methods.is_empty() {
      let methods: Vec<TokenStream> = other_methods.iter().map(|m| self.generate_method(m)).collect();
      result = quote! {
        #result
        impl #name {
          #(#methods)*
        }
      };
    }

    result
  }

  fn generate_method(&self, method: &StructMethod) -> TokenStream {
    match &method.kind {
      StructMethodKind::ParseResponse {
        response_enum,
        variants,
      } => generate_parse_response_method(self.visibility, &method.name, &method.docs, response_enum, variants),
      StructMethodKind::Builder { fields, nested_structs } => {
        generate_builder_method(self.visibility, &method.docs, fields, nested_structs)
      }
    }
  }
}

fn generate_parse_response_method(
  vis: Visibility,
  method_name: impl ToTokens,
  docs: impl ToTokens,
  response_enum: &EnumToken,
  variants: &[ResponseVariant],
) -> TokenStream {
  let vis = vis.to_tokens();

  let (defaults, specifics) = partition_variants(variants);
  let grouped_specifics = group_by_status_code(&specifics);

  let status_checks: Vec<TokenStream> = grouped_specifics
    .iter()
    .map(|(code, variants)| generate_status_code_block(response_enum, *code, variants))
    .collect();

  let fallback = generate_fallback_block(response_enum, defaults.first());
  let status_decl = if status_checks.is_empty() {
    quote! {}
  } else {
    quote! { let status = req.status(); }
  };

  quote! {
    #docs
    #vis async fn #method_name(req: reqwest::Response) -> anyhow::Result<#response_enum> {
      #status_decl
      #(#status_checks)*
      #fallback
    }
  }
}

fn partition_variants(variants: &[ResponseVariant]) -> (Vec<&ResponseVariant>, Vec<&ResponseVariant>) {
  variants.iter().partition(|v| v.status_code.is_default())
}

fn group_by_status_code<'b>(variants: &[&'b ResponseVariant]) -> IndexMap<StatusCodeToken, Vec<&'b ResponseVariant>> {
  let mut grouped: IndexMap<StatusCodeToken, Vec<&'b ResponseVariant>> = IndexMap::new();
  for variant in variants {
    grouped.entry(variant.status_code).or_default().push(variant);
  }
  grouped
}

fn generate_status_code_block(
  response_enum: &EnumToken,
  status_code: StatusCodeToken,
  variants: &[&ResponseVariant],
) -> TokenStream {
  let condition = status_code_condition(status_code);
  let body = generate_dispatch_body(response_enum, variants);

  quote! {
    if #condition {
      #body
    }
  }
}

fn generate_dispatch_body(response_enum: &EnumToken, variants: &[&ResponseVariant]) -> TokenStream {
  if variants.len() == 1 {
    let variant = variants[0];
    let distinct_categories: BTreeSet<_> = variant.media_types.iter().map(|m| m.category).collect();
    if distinct_categories.len() <= 1 {
      return generate_single_variant_logic(response_enum, variant);
    }
  }

  generate_content_type_dispatch(response_enum, variants)
}

fn generate_content_type_dispatch(response_enum: &EnumToken, variants: &[&ResponseVariant]) -> TokenStream {
  let content_type_header = quote! {
    let content_type_str = req.headers()
      .get(reqwest::header::CONTENT_TYPE)
      .and_then(|v| v.to_str().ok())
      .unwrap_or("application/json");
  };

  let mut cases = Vec::new();
  for variant in variants {
    if variant.media_types.is_empty() {
      cases.push((ResponseMediaType::primary_category(&[]), *variant));
    } else {
      for media_type in &variant.media_types {
        cases.push((media_type.category, *variant));
      }
    }
  }

  let (streams, others): (Vec<_>, Vec<_>) = cases
    .into_iter()
    .partition(|(cat, _)| *cat == ContentCategory::EventStream);

  let stream_checks: Vec<TokenStream> = streams
    .iter()
    .map(|(cat, v)| {
      let block = generate_single_variant_logic_for_category(response_enum, v, *cat);
      quote! {
         if content_type_str.contains("event-stream") {
           #block
         }
      }
    })
    .collect();

  let other_checks: Vec<TokenStream> = others
    .iter()
    .map(|(cat, v)| {
      let check_expr = content_type_check_expr(*cat);
      let block = generate_single_variant_logic_for_category(response_enum, v, *cat);
      quote! {
        if #check_expr {
           #block
        }
      }
    })
    .collect();

  quote! {
    #content_type_header
    #(#stream_checks)*
    #(#other_checks)*
  }
}

fn generate_single_variant_logic(response_enum: &EnumToken, variant: &ResponseVariant) -> TokenStream {
  let category = ResponseMediaType::primary_category(&variant.media_types);
  generate_single_variant_logic_for_category(response_enum, variant, category)
}

fn generate_single_variant_logic_for_category(
  response_enum: &EnumToken,
  variant: &ResponseVariant,
  category: ContentCategory,
) -> TokenStream {
  let variant_name = &variant.variant_name;
  let schema_type = variant.schema_type.as_ref();

  match schema_type {
    Some(ty) => {
      let extraction = generate_data_extraction(ty, category);
      quote! {
        let data = #extraction;
        return Ok(#response_enum::#variant_name(data));
      }
    }
    None => {
      quote! {
        let _ = req.bytes().await?;
        return Ok(#response_enum::#variant_name);
      }
    }
  }
}

fn generate_fallback_block(response_enum: &EnumToken, default_variant: Option<&&ResponseVariant>) -> TokenStream {
  if let Some(variant) = default_variant {
    generate_single_variant_logic(response_enum, variant)
  } else {
    let unknown_variant = EnumVariantToken::from("Unknown");
    quote! {
      let _ = req.bytes().await?;
      Ok(#response_enum::#unknown_variant)
    }
  }
}

fn status_code_condition(status_code: StatusCodeToken) -> TokenStream {
  match status_code {
    code if code.is_success() => quote! { status.is_success() },
    code if code.is_default() => quote! { true },
    StatusCodeToken::Informational1XX => quote! { status.is_informational() },
    StatusCodeToken::Redirection3XX => quote! { status.is_redirection() },
    StatusCodeToken::ClientError4XX => quote! { status.is_client_error() },
    StatusCodeToken::ServerError5XX => quote! { status.is_server_error() },
    other => other
      .code()
      .map_or_else(|| quote! { false }, |code| quote! { status.as_u16() == #code }),
  }
}

fn content_type_check_expr(category: ContentCategory) -> TokenStream {
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

fn generate_data_extraction(schema_type: &TypeRef, category: ContentCategory) -> TokenStream {
  match category {
    ContentCategory::Text => {
      if schema_type.is_string_like() {
        quote! { req.text().await? }
      } else if matches!(schema_type.base_type, RustPrimitive::Custom(_)) {
        quote! { oas3_gen_support::Diagnostics::<#schema_type>::json_with_diagnostics(req).await? }
      } else {
        quote! { req.text().await?.parse::<#schema_type>()? }
      }
    }
    ContentCategory::Binary => {
      if matches!(schema_type.base_type, RustPrimitive::Custom(_)) {
        quote! { oas3_gen_support::Diagnostics::<#schema_type>::json_with_diagnostics(req).await? }
      } else {
        quote! { req.bytes().await?.to_vec() }
      }
    }
    ContentCategory::EventStream => {
      quote! { <#schema_type>::from_response(req) }
    }
    ContentCategory::Xml => {
      quote! { oas3_gen_support::Diagnostics::<#schema_type>::xml_with_diagnostics(req).await? }
    }
    _ => quote! { oas3_gen_support::Diagnostics::<#schema_type>::json_with_diagnostics(req).await? },
  }
}

fn generate_builder_method(
  vis: Visibility,
  docs: impl ToTokens,
  fields: &[BuilderField],
  nested_structs: &[BuilderNestedStruct],
) -> TokenStream {
  let vis = vis.to_tokens();

  let params: Vec<TokenStream> = fields
    .iter()
    .map(|f| {
      let name = &f.name;
      let ty = &f.rust_type;
      quote! { #name: #ty }
    })
    .collect();

  let validations = generate_builder_validations(fields);
  let struct_construction = generate_struct_construction(fields, nested_structs);

  quote! {
    #docs
    #[builder]
    #vis fn new(#(#params),*) -> Result<Self, anyhow::Error> {
      #validations
      let request = #struct_construction;
      request.validate()?;
      Ok(request)
    }
  }
}

fn generate_builder_validations(fields: &[BuilderField]) -> TokenStream {
  let checks: Vec<TokenStream> = fields
    .iter()
    .filter_map(|f| {
      if !f.required {
        return None;
      }

      let has_min_length = f
        .validation_attrs
        .iter()
        .any(|attr| matches!(attr, ValidationAttribute::Length { min: Some(_), .. }));

      if !has_min_length {
        return None;
      }

      if !f.rust_type.is_string_like() {
        return None;
      }

      let name = &f.name;
      let name_str = f.name.to_string();

      Some(quote! {
        if #name.is_empty() {
          return Err(anyhow::anyhow!("Empty {} is disallowed", #name_str));
        }
      })
    })
    .collect();

  quote! { #(#checks)* }
}

fn generate_struct_construction(fields: &[BuilderField], nested_structs: &[BuilderNestedStruct]) -> TokenStream {
  let nested_map: BTreeMap<&str, &BuilderNestedStruct> =
    nested_structs.iter().map(|ns| (ns.field_name.as_str(), ns)).collect();

  let field_assignments: Vec<TokenStream> = if nested_structs.is_empty() {
    fields.iter().map(|f| f.name.to_token_stream()).collect()
  } else {
    let mut processed_nested: BTreeSet<&str> = BTreeSet::new();
    let mut assignments = Vec::new();

    for field in fields {
      let nested_field_name = field.nested_struct.as_str();

      if let Some(nested) = nested_map.get(nested_field_name) {
        if processed_nested.insert(nested_field_name) {
          let field_name = &nested.field_name;
          let struct_name = &nested.struct_name;
          let inner_fields: Vec<TokenStream> = nested
            .field_names
            .iter()
            .map(quote::ToTokens::to_token_stream)
            .collect();

          assignments.push(quote! {
            #field_name: #struct_name { #(#inner_fields),* }
          });
        }
      } else {
        assignments.push(field.name.to_token_stream());
      }
    }

    assignments
  };

  quote! {
    Self { #(#field_assignments),* }
  }
}

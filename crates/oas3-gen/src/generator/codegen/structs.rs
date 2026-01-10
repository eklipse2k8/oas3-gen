use std::collections::{BTreeMap, BTreeSet};

use proc_macro2::TokenStream;
use quote::{ToTokens, quote};

use super::{
  Visibility,
  attributes::{
    generate_deprecated_attr, generate_doc_hidden_attr, generate_docs_for_field, generate_field_default_attr,
    generate_outer_attrs, generate_serde_as_attr, generate_serde_attrs, generate_validation_attrs,
  },
};
use crate::generator::{
  ast::{
    BuilderField, BuilderNestedStruct, ContentCategory, DerivesProvider, FieldDef, RegexKey, ResponseStatusCategory,
    ResponseVariantCategory, RustPrimitive, StatusCodeToken, StatusHandler, StructDef, StructKind, StructMethod,
    StructMethodKind, TypeRef, ValidationAttribute,
    tokens::{ConstToken, EnumToken, EnumVariantToken},
  },
  codegen::{attributes::generate_derives_from_slice, headers::HeaderMapGenerator},
};

pub(crate) struct StructGenerator<'a> {
  def: &'a StructDef,
  regex_lookup: &'a BTreeMap<RegexKey, ConstToken>,
  vis: TokenStream,
}

impl<'a> StructGenerator<'a> {
  pub(crate) fn new(
    def: &'a StructDef,
    regex_lookup: &'a BTreeMap<RegexKey, ConstToken>,
    visibility: Visibility,
  ) -> Self {
    Self {
      def,
      regex_lookup,
      vis: visibility.to_tokens(),
    }
  }

  pub(crate) fn emit(&self) -> TokenStream {
    let definition = self.definition().emit();
    let impl_block = self.impl_block().emit();
    let header_map_impl = HeaderMapGenerator::new(self.def).emit();

    quote! {
      #definition
      #impl_block
      #header_map_impl
    }
  }

  fn definition(&self) -> Definition<'_> {
    Definition {
      def: self.def,
      regex_lookup: self.regex_lookup,
      vis: &self.vis,
    }
  }

  fn impl_block(&self) -> ImplBlock<'_> {
    ImplBlock {
      def: self.def,
      vis: &self.vis,
    }
  }
}

struct Definition<'a> {
  def: &'a StructDef,
  regex_lookup: &'a BTreeMap<RegexKey, ConstToken>,
  vis: &'a TokenStream,
}

impl Definition<'_> {
  fn emit(&self) -> TokenStream {
    let name = &self.def.name;
    let docs = &self.def.docs;
    let vis = self.vis;

    let derives = generate_derives_from_slice(&self.def.derives());
    let outer_attrs = generate_outer_attrs(&self.def.outer_attrs);
    let serde_attrs = generate_serde_attrs(&self.def.serde_attrs);
    let fields: Vec<TokenStream> = self.def.fields.iter().map(|f| self.field(f)).collect();

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

  fn field(&self, field: &FieldDef) -> TokenStream {
    let name = &field.name;
    let docs = generate_docs_for_field(field);
    let vis = self.vis;
    let type_tokens = &field.rust_type;

    // Only generate serde attributes for structs that derive Serialize or Deserialize
    let (serde_as, serde_attrs) = if matches!(self.def.kind, StructKind::HeaderParams | StructKind::PathParams) {
      (quote! {}, quote! {})
    } else {
      (
        generate_serde_as_attr(field.serde_as_attr.as_ref()),
        generate_serde_attrs(&field.serde_attrs),
      )
    };
    let validation = self.validation(field);
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

  fn validation(&self, field: &FieldDef) -> TokenStream {
    let attrs: Vec<ValidationAttribute> = field
      .validation_attrs
      .iter()
      .map(|attr| match attr {
        ValidationAttribute::Regex(_) => {
          let key = RegexKey::for_struct(&self.def.name, field.name.as_str());
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
}

struct ImplBlock<'a> {
  def: &'a StructDef,
  vis: &'a TokenStream,
}

impl ImplBlock<'_> {
  fn emit(&self) -> TokenStream {
    if self.def.methods.is_empty() {
      return quote! {};
    }

    let name = &self.def.name;
    let (builder_methods, other_methods): (Vec<_>, Vec<_>) = self
      .def
      .methods
      .iter()
      .partition(|m| matches!(m.kind, StructMethodKind::Builder { .. }));

    let mut result = quote! {};

    if !builder_methods.is_empty() {
      let methods: Vec<TokenStream> = builder_methods.iter().map(|m| self.method(m)).collect();
      result = quote! {
        #result
        #[bon::bon]
        impl #name {
          #(#methods)*
        }
      };
    }

    if !other_methods.is_empty() {
      let methods: Vec<TokenStream> = other_methods.iter().map(|m| self.method(m)).collect();
      result = quote! {
        #result
        impl #name {
          #(#methods)*
        }
      };
    }

    result
  }

  fn method(&self, method: &StructMethod) -> TokenStream {
    match &method.kind {
      StructMethodKind::ParseResponse {
        response_enum,
        status_handlers,
        default_handler,
      } => parse_response::Generator::new(
        response_enum,
        status_handlers,
        default_handler.as_ref(),
        self.vis,
        &method.name,
        &method.docs,
      )
      .emit(),
      StructMethodKind::IntoAxumResponse {
        response_enum,
        status_handlers,
        default_handler,
      } => parse_response::Generator::new(
        response_enum,
        status_handlers,
        default_handler.as_ref(),
        self.vis,
        &method.name,
        &method.docs,
      )
      .emit(),
      StructMethodKind::Builder { fields, nested_structs } => {
        builder::Generator::new(fields, nested_structs, self.vis, &method.docs).emit()
      }
    }
  }
}

mod parse_response {
  use super::*;

  pub(super) struct Generator<'a> {
    response_enum: &'a EnumToken,
    status_handlers: &'a [StatusHandler],
    default_handler: Option<&'a ResponseVariantCategory>,
    vis: &'a TokenStream,
    method_name: &'a dyn ToTokens,
    docs: &'a dyn ToTokens,
  }

  impl<'a> Generator<'a> {
    pub(super) fn new(
      response_enum: &'a EnumToken,
      status_handlers: &'a [StatusHandler],
      default_handler: Option<&'a ResponseVariantCategory>,
      vis: &'a TokenStream,
      method_name: &'a impl ToTokens,
      docs: &'a impl ToTokens,
    ) -> Self {
      Self {
        response_enum,
        status_handlers,
        default_handler,
        vis,
        method_name,
        docs,
      }
    }

    pub(super) fn emit(&self) -> TokenStream {
      let vis = self.vis;
      let method_name = self.method_name;
      let docs = self.docs;
      let response_enum = self.response_enum;

      let status_checks: Vec<TokenStream> = self.status_handlers.iter().map(|h| self.status_block(h)).collect();

      let fallback = self.fallback();
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

    fn status_block(&self, handler: &StatusHandler) -> TokenStream {
      let cond = condition(handler.status_code);
      let body = self.dispatch(&handler.dispatch);

      quote! {
        if #cond {
          #body
        }
      }
    }

    fn dispatch(&self, dispatch: &ResponseStatusCategory) -> TokenStream {
      match dispatch {
        ResponseStatusCategory::Single(case) => self.dispatch_case(case),
        ResponseStatusCategory::ContentDispatch {
          streams: event_streams,
          variants: others,
        } => self.content_dispatch(event_streams, others),
      }
    }

    fn content_dispatch(
      &self,
      event_streams: &[ResponseVariantCategory],
      others: &[ResponseVariantCategory],
    ) -> TokenStream {
      let content_type_header = quote! {
        let content_type_str = req.headers()
          .get(reqwest::header::CONTENT_TYPE)
          .and_then(|v| v.to_str().ok())
          .unwrap_or("application/json");
      };

      let stream_checks: Vec<TokenStream> = event_streams
        .iter()
        .map(|case| {
          let block = self.dispatch_case(case);
          quote! {
            if content_type_str.contains("event-stream") {
              #block
            }
          }
        })
        .collect();

      let other_checks: Vec<TokenStream> = others
        .iter()
        .map(|case| {
          let check = content_check(case.category);
          let block = self.dispatch_case(case);
          quote! {
            if #check {
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

    fn dispatch_case(&self, case: &ResponseVariantCategory) -> TokenStream {
      let response_enum = self.response_enum;
      let variant_name = &case.variant.variant_name;

      match case.variant.schema_type.as_ref() {
        Some(ty) => {
          let data = extraction(ty, case.category);
          quote! {
            let data = #data;
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

    fn fallback(&self) -> TokenStream {
      if let Some(case) = self.default_handler {
        self.dispatch_case(case)
      } else {
        let response_enum = self.response_enum;
        let unknown_variant = EnumVariantToken::from("Unknown");
        quote! {
          let _ = req.bytes().await?;
          Ok(#response_enum::#unknown_variant)
        }
      }
    }
  }

  fn condition(code: StatusCodeToken) -> TokenStream {
    match code {
      c if c.is_success() => quote! { status.is_success() },
      c if c.is_default() => quote! { true },
      StatusCodeToken::Informational1XX => quote! { status.is_informational() },
      StatusCodeToken::Redirection3XX => quote! { status.is_redirection() },
      StatusCodeToken::ClientError4XX => quote! { status.is_client_error() },
      StatusCodeToken::ServerError5XX => quote! { status.is_server_error() },
      other => other
        .code()
        .map_or_else(|| quote! { false }, |code| quote! { status.as_u16() == #code }),
    }
  }

  fn content_check(category: ContentCategory) -> TokenStream {
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

  fn extraction(schema_type: &TypeRef, category: ContentCategory) -> TokenStream {
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
        quote! { req.bytes().await?.to_vec() }
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
}

mod builder {
  use super::*;

  pub(super) struct Generator<'a> {
    fields: &'a [BuilderField],
    nested_structs: &'a [BuilderNestedStruct],
    vis: &'a TokenStream,
    docs: &'a dyn ToTokens,
  }

  impl<'a> Generator<'a> {
    pub(super) fn new(
      fields: &'a [BuilderField],
      nested_structs: &'a [BuilderNestedStruct],
      vis: &'a TokenStream,
      docs: &'a impl ToTokens,
    ) -> Self {
      Self {
        fields,
        nested_structs,
        vis,
        docs,
      }
    }

    pub(super) fn emit(&self) -> TokenStream {
      let vis = self.vis;
      let docs = self.docs;

      let params: Vec<TokenStream> = self
        .fields
        .iter()
        .map(|f| {
          let name = &f.name;
          let ty = &f.rust_type;
          quote! { #name: #ty }
        })
        .collect();

      let construction = self.construction();

      quote! {
        #docs
        #[builder]
        #vis fn new(#(#params),*) -> anyhow::Result<Self> {
          let request = #construction;
          request.validate()?;
          Ok(request)
        }
      }
    }

    fn construction(&self) -> TokenStream {
      let nested_map = self
        .nested_structs
        .iter()
        .map(|ns| (ns.field_name.as_str(), ns))
        .collect::<BTreeMap<_, _>>();

      let mut processed_nested = BTreeSet::new();
      let mut assignments = vec![];

      for field in self.fields {
        match &field.owner_field {
          Some(owner) => {
            let owner_name = owner.as_str();
            if let Some(nested) = nested_map.get(owner_name)
              && processed_nested.insert(owner_name)
            {
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
          }
          None => {
            assignments.push(field.name.to_token_stream());
          }
        }
      }

      quote! {
        Self { #(#assignments),* }
      }
    }
  }
}

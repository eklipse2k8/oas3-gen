use std::collections::{BTreeMap, BTreeSet};

use proc_macro2::TokenStream;
use quote::{ToTokens, quote};

use super::{
  Visibility,
  attributes::{
    generate_builder_attrs, generate_deprecated_attr, generate_doc_hidden_attr, generate_docs_for_field,
    generate_field_default_attr, generate_outer_attrs, generate_serde_as_attr, generate_serde_attrs,
    generate_validation_attrs,
  },
};
use crate::generator::{
  ast::{
    BuilderField, BuilderNestedStruct, ContentCategory, DerivesProvider, Documentation, FieldDef, MethodKind,
    MethodNameToken, OperationResponse, RegexKey, ResponseStatusCategory, ResponseVariantCategory, RustPrimitive,
    StatusCodeToken, StatusHandler, StructDef, StructKind, StructMethod, TypeRef, ValidationAttribute,
    tokens::{ConstToken, EnumVariantToken},
  },
  codegen::{
    attributes::generate_derives_from_slice,
    headers::{HeaderFromMapFragment, HeaderMapFragment},
    http::HttpStatusCode,
  },
  converter::GenerationTarget,
  naming::constants::OTHER_RESPONSE_VARIANT,
};

#[derive(Clone, Copy, Debug)]
pub(crate) struct StructFragment<'a> {
  def: &'a StructDef,
  regex_lookup: &'a BTreeMap<RegexKey, ConstToken>,
  visibility: Visibility,
  target: GenerationTarget,
}

impl<'a> StructFragment<'a> {
  pub(crate) fn new(
    def: &'a StructDef,
    regex_lookup: &'a BTreeMap<RegexKey, ConstToken>,
    visibility: Visibility,
    target: GenerationTarget,
  ) -> Self {
    Self {
      def,
      regex_lookup,
      visibility,
      target,
    }
  }
}

impl ToTokens for StructFragment<'_> {
  fn to_tokens(&self, tokens: &mut TokenStream) {
    let definition = StructDefinitionFragment::new(self.def, self.regex_lookup, self.visibility);
    let impl_block = StructImplBlockFragment::new(self.def, self.visibility);
    let header_map = HeaderMapFragment::new(self.def);

    tokens.extend(quote! {
      #definition

      #impl_block

      #header_map

    });

    if self.target == GenerationTarget::Server {
      let header_from_map = HeaderFromMapFragment::new(self.def);
      tokens.extend(quote! {
        #header_from_map

      });
    }
  }
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct StructDefinitionFragment<'a> {
  def: &'a StructDef,
  regex_lookup: &'a BTreeMap<RegexKey, ConstToken>,
  visibility: Visibility,
}

impl<'a> StructDefinitionFragment<'a> {
  pub(crate) fn new(
    def: &'a StructDef,
    regex_lookup: &'a BTreeMap<RegexKey, ConstToken>,
    visibility: Visibility,
  ) -> Self {
    Self {
      def,
      regex_lookup,
      visibility,
    }
  }
}

impl ToTokens for StructDefinitionFragment<'_> {
  fn to_tokens(&self, tokens: &mut TokenStream) {
    let name = &self.def.name;
    let docs = &self.def.docs;
    let vis = &self.visibility;

    let derives = generate_derives_from_slice(&self.def.derives());
    let outer_attrs = generate_outer_attrs(&self.def.outer_attrs);
    let serde_attrs = generate_serde_attrs(&self.def.serde_attrs);

    let fields: Vec<StructFieldFragment<'_>> = self
      .def
      .fields
      .iter()
      .map(|f| StructFieldFragment::new(f, self.def, self.regex_lookup, self.visibility))
      .collect();

    tokens.extend(quote! {
      #docs
      #outer_attrs
      #derives
      #serde_attrs
      #vis struct #name {
        #(#fields),*
      }
    });
  }
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct StructFieldFragment<'a> {
  field: &'a FieldDef,
  struct_def: &'a StructDef,
  regex_lookup: &'a BTreeMap<RegexKey, ConstToken>,
  visibility: Visibility,
}

impl<'a> StructFieldFragment<'a> {
  pub(crate) fn new(
    field: &'a FieldDef,
    struct_def: &'a StructDef,
    regex_lookup: &'a BTreeMap<RegexKey, ConstToken>,
    visibility: Visibility,
  ) -> Self {
    Self {
      field,
      struct_def,
      regex_lookup,
      visibility,
    }
  }

  fn validation_attrs(&self) -> TokenStream {
    let attrs: Vec<ValidationAttribute> = self
      .field
      .validation_attrs
      .iter()
      .map(|attr| match attr {
        ValidationAttribute::Regex(_) => {
          let key = RegexKey::for_struct(&self.struct_def.name, self.field.name.as_str());
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

impl ToTokens for StructFieldFragment<'_> {
  fn to_tokens(&self, tokens: &mut TokenStream) {
    let name = &self.field.name;
    let field_docs = generate_docs_for_field(self.field);
    let docs = field_docs.as_ref();
    let vis = &self.visibility;
    let type_tokens = &self.field.rust_type;

    let (serde_as, serde_attrs) = if matches!(self.struct_def.kind, StructKind::HeaderParams | StructKind::PathParams) {
      (quote! {}, quote! {})
    } else {
      (
        generate_serde_as_attr(self.field.serde_as_attr.as_ref()),
        generate_serde_attrs(&self.field.serde_attrs),
      )
    };

    let validation = self.validation_attrs();
    let deprecated = generate_deprecated_attr(self.field.deprecated);
    let default_val = generate_field_default_attr(self.field);
    let builder_attr = generate_builder_attrs(&self.field.builder_attrs);
    let doc_hidden = generate_doc_hidden_attr(self.field.doc_hidden);

    tokens.extend(quote! {
      #doc_hidden
      #docs
      #deprecated
      #serde_as
      #serde_attrs
      #validation
      #default_val
      #builder_attr
      #vis #name: #type_tokens
    });
  }
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct StructImplBlockFragment<'a> {
  def: &'a StructDef,
  visibility: Visibility,
}

impl<'a> StructImplBlockFragment<'a> {
  pub(crate) fn new(def: &'a StructDef, visibility: Visibility) -> Self {
    Self { def, visibility }
  }
}

impl ToTokens for StructImplBlockFragment<'_> {
  fn to_tokens(&self, tokens: &mut TokenStream) {
    if self.def.methods.is_empty() {
      return;
    }

    let name = &self.def.name;
    let (builder_methods, other_methods): (Vec<_>, Vec<_>) = self
      .def
      .methods
      .iter()
      .partition(|m| matches!(m.kind, MethodKind::Builder { .. }));

    if !builder_methods.is_empty() {
      let methods: Vec<TokenStream> = builder_methods
        .into_iter()
        .map(|m| StructMethodFragment::new(m, self.visibility).into_token_stream())
        .collect();

      tokens.extend(quote! {
        #[bon::bon]
        impl #name {
          #(#methods)*
        }
      });
    }

    if !other_methods.is_empty() {
      let methods: Vec<TokenStream> = other_methods
        .into_iter()
        .map(|m| StructMethodFragment::new(m, self.visibility).into_token_stream())
        .collect();

      tokens.extend(quote! {
        impl #name {
          #(#methods)*
        }
      });
    }
  }
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct StructMethodFragment<'a> {
  method: &'a StructMethod,
  visibility: Visibility,
}

impl<'a> StructMethodFragment<'a> {
  pub(crate) fn new(method: &'a StructMethod, visibility: Visibility) -> Self {
    Self { method, visibility }
  }
}

impl ToTokens for StructMethodFragment<'_> {
  fn to_tokens(&self, tokens: &mut TokenStream) {
    let ts = match &self.method.kind {
      MethodKind::ParseResponse {
        response,
        status_handlers,
        default_handler,
      } => ParseResponseMethodFragment::new(
        response,
        status_handlers,
        default_handler.as_ref(),
        self.visibility,
        &self.method.name,
        &self.method.docs,
      )
      .into_token_stream(),
      MethodKind::Builder { fields, nested_structs } => {
        BuilderMethodFragment::new(fields, nested_structs, self.visibility, &self.method.docs).into_token_stream()
      }
    };

    tokens.extend(ts);
  }
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct ParseResponseMethodFragment<'a> {
  response: &'a OperationResponse,
  status_handlers: &'a [StatusHandler],
  default_handler: Option<&'a ResponseVariantCategory>,
  visibility: Visibility,
  method_name: &'a MethodNameToken,
  docs: &'a Documentation,
}

impl<'a> ParseResponseMethodFragment<'a> {
  pub(crate) fn new(
    response: &'a OperationResponse,
    status_handlers: &'a [StatusHandler],
    default_handler: Option<&'a ResponseVariantCategory>,
    visibility: Visibility,
    method_name: &'a MethodNameToken,
    docs: &'a Documentation,
  ) -> Self {
    Self {
      response,
      status_handlers,
      default_handler,
      visibility,
      method_name,
      docs,
    }
  }
}

impl ToTokens for ParseResponseMethodFragment<'_> {
  fn to_tokens(&self, tokens: &mut TokenStream) {
    let status_checks = self
      .status_handlers
      .iter()
      .map(|h| StatusCheckFragment::new(h, self.response))
      .collect::<Vec<_>>();

    let fallback = FallbackFragment::new(self.response, self.default_handler);

    let vis = &self.visibility;
    let method_name = self.method_name;
    let docs = self.docs;
    let response = self.response;

    tokens.extend(quote! {
      #docs
      #vis async fn #method_name(req: reqwest::Response) -> anyhow::Result<#response> {
        let status = req.status();
        #(#status_checks)*
        #fallback
      }
    });
  }
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct StatusCheckFragment<'a> {
  handler: &'a StatusHandler,
  response: &'a OperationResponse,
}

impl<'a> StatusCheckFragment<'a> {
  pub(crate) fn new(handler: &'a StatusHandler, response: &'a OperationResponse) -> Self {
    Self { handler, response }
  }
}

impl ToTokens for StatusCheckFragment<'_> {
  fn to_tokens(&self, tokens: &mut TokenStream) {
    let cond = StatusConditionFragment::new(self.handler.status_code);
    let body = ResponseDispatchFragment::new(&self.handler.dispatch, self.response);

    tokens.extend(quote! {
      if #cond {
        #body
      }
    });
  }
}

#[derive(Clone, Debug)]
pub(crate) struct StatusConditionFragment {
  status_code: StatusCodeToken,
}

impl StatusConditionFragment {
  pub(crate) fn new(status_code: StatusCodeToken) -> Self {
    Self { status_code }
  }
}

impl ToTokens for StatusConditionFragment {
  fn to_tokens(&self, tokens: &mut TokenStream) {
    let ts = match self.status_code {
      c if c.is_default() => quote! { true },
      StatusCodeToken::Informational1XX => quote! { status.is_informational() },
      StatusCodeToken::Success2XX => quote! { status.is_success() },
      StatusCodeToken::Redirection3XX => quote! { status.is_redirection() },
      StatusCodeToken::ClientError4XX => quote! { status.is_client_error() },
      StatusCodeToken::ServerError5XX => quote! { status.is_server_error() },
      StatusCodeToken::Default => quote! { false },
      status_code => {
        let code = HttpStatusCode::new(status_code);
        quote! { status == #code }
      }
    };

    tokens.extend(ts);
  }
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct ResponseDispatchFragment<'a> {
  dispatch: &'a ResponseStatusCategory,
  response: &'a OperationResponse,
}

impl<'a> ResponseDispatchFragment<'a> {
  pub(crate) fn new(dispatch: &'a ResponseStatusCategory, response: &'a OperationResponse) -> Self {
    Self { dispatch, response }
  }
}

impl ToTokens for ResponseDispatchFragment<'_> {
  fn to_tokens(&self, tokens: &mut TokenStream) {
    let ts = match self.dispatch {
      ResponseStatusCategory::Single(case) => ResponseCaseFragment::new(case, self.response).into_token_stream(),
      ResponseStatusCategory::ContentDispatch { streams, variants } => {
        ContentDispatchFragment::new(streams, variants, self.response).into_token_stream()
      }
    };

    tokens.extend(ts);
  }
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct ContentDispatchFragment<'a> {
  event_streams: &'a [ResponseVariantCategory],
  others: &'a [ResponseVariantCategory],
  response: &'a OperationResponse,
}

impl<'a> ContentDispatchFragment<'a> {
  pub(crate) fn new(
    event_streams: &'a [ResponseVariantCategory],
    others: &'a [ResponseVariantCategory],
    response: &'a OperationResponse,
  ) -> Self {
    Self {
      event_streams,
      others,
      response,
    }
  }
}

impl ToTokens for ContentDispatchFragment<'_> {
  fn to_tokens(&self, tokens: &mut TokenStream) {
    let content_type_header = quote! {
      let content_type_str = req.headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("application/json");
    };

    let stream_checks: Vec<TokenStream> = self
      .event_streams
      .iter()
      .map(|case| {
        let block = ResponseCaseFragment::new(case, self.response);
        quote! {
          if content_type_str.contains("event-stream") {
            #block
          }
        }
      })
      .collect();

    let other_checks: Vec<TokenStream> = self
      .others
      .iter()
      .map(|case| {
        let check = ContentCheckFragment::new(case.category);
        let block = ResponseCaseFragment::new(case, self.response);
        quote! {
          if #check {
            #block
          }
        }
      })
      .collect();

    tokens.extend(quote! {
      #content_type_header
      #(#stream_checks)*
      #(#other_checks)*
    });
  }
}

#[derive(Clone, Debug)]
pub(crate) struct ContentCheckFragment {
  category: ContentCategory,
}

impl ContentCheckFragment {
  pub(crate) fn new(category: ContentCategory) -> Self {
    Self { category }
  }
}

impl ToTokens for ContentCheckFragment {
  fn to_tokens(&self, tokens: &mut TokenStream) {
    let ts = match self.category {
      ContentCategory::Json => quote! { content_type_str.contains("json") },
      ContentCategory::Xml => quote! { content_type_str.contains("xml") },
      ContentCategory::Text => quote! { content_type_str.starts_with("text/") && !content_type_str.contains("xml") },
      ContentCategory::Binary => {
        quote! { content_type_str.starts_with("application/octet-stream") || content_type_str.starts_with("image/") || content_type_str.starts_with("audio/") || content_type_str.starts_with("video/") }
      }
      ContentCategory::EventStream => quote! { content_type_str.contains("event-stream") },
      ContentCategory::FormUrlEncoded => quote! { content_type_str.contains("x-www-form-urlencoded") },
      ContentCategory::Multipart => quote! { content_type_str.contains("multipart") },
    };

    tokens.extend(ts);
  }
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct ResponseCaseFragment<'a> {
  case: &'a ResponseVariantCategory,
  response: &'a OperationResponse,
}

impl<'a> ResponseCaseFragment<'a> {
  pub(crate) fn new(case: &'a ResponseVariantCategory, response: &'a OperationResponse) -> Self {
    Self { case, response }
  }
}

impl ToTokens for ResponseCaseFragment<'_> {
  fn to_tokens(&self, tokens: &mut TokenStream) {
    let variant = &self.case.variant;
    let variant_name = variant.variant_name();
    let response_enum = &self.response.name;
    let mapping = &variant.mapping;
    let carries_status = variant.status_code.carries_status();
    let status_arg = carries_status.then(|| quote! { status, });
    let param = self.response.param(mapping.payload);
    let optional = param.is_some_and(|p| p.nullable);

    let ts = match (variant.schema_type.as_ref(), param) {
      (_, None) => {
        let fields = carries_status.then(|| quote! { (status) });
        quote! {
          let _ = req.bytes().await?;
          return Ok(#response_enum::#variant_name #fields);
        }
      }
      (None, Some(_)) => {
        let empty = if optional {
          quote! { None }
        } else {
          quote! { () }
        };
        quote! {
          let _ = req.bytes().await?;
          return Ok(#response_enum::#variant_name(#status_arg #empty));
        }
      }
      (Some(schema_type), Some(param)) => {
        let data = ResponseExtractionFragment::new(schema_type, self.case.category);
        let mut value = quote! { data };
        if let Some(union_variant) = &mapping.union_variant {
          let union = &param.base_type;
          value = quote! { #union::#union_variant(#value) };
        }
        if optional {
          value = quote! { Some(#value) };
        }
        quote! {
          let data = #data;
          return Ok(#response_enum::#variant_name(#status_arg #value));
        }
      }
    };

    tokens.extend(ts);
  }
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct ResponseExtractionFragment<'a> {
  schema_type: &'a TypeRef,
  category: ContentCategory,
}

impl<'a> ResponseExtractionFragment<'a> {
  pub(crate) fn new(schema_type: &'a TypeRef, category: ContentCategory) -> Self {
    Self { schema_type, category }
  }
}

impl ToTokens for ResponseExtractionFragment<'_> {
  fn to_tokens(&self, tokens: &mut TokenStream) {
    let schema_type = self.schema_type;

    let ts = match self.category {
      ContentCategory::Text | ContentCategory::Binary if matches!(self.schema_type.base_type, RustPrimitive::Bytes) => {
        quote! { req.bytes().await?.to_vec() }
      }
      ContentCategory::Text | ContentCategory::Binary if self.schema_type.is_string_like() => {
        quote! { req.text().await? }
      }
      ContentCategory::Text => {
        if matches!(self.schema_type.base_type, RustPrimitive::Custom(_)) {
          quote! { oas3_gen_support::Diagnostics::<#schema_type>::json_with_diagnostics(req).await? }
        } else {
          quote! { req.text().await?.parse::<#schema_type>()? }
        }
      }
      ContentCategory::EventStream => {
        quote! { <#schema_type>::from_response(req) }
      }
      ContentCategory::Xml => {
        quote! { oas3_gen_support::Diagnostics::<#schema_type>::xml_with_diagnostics(req).await? }
      }
      _ => quote! { oas3_gen_support::Diagnostics::<#schema_type>::json_with_diagnostics(req).await? },
    };

    tokens.extend(ts);
  }
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct FallbackFragment<'a> {
  response: &'a OperationResponse,
  default_handler: Option<&'a ResponseVariantCategory>,
}

impl<'a> FallbackFragment<'a> {
  pub(crate) fn new(response: &'a OperationResponse, default_handler: Option<&'a ResponseVariantCategory>) -> Self {
    Self {
      response,
      default_handler,
    }
  }
}

impl ToTokens for FallbackFragment<'_> {
  fn to_tokens(&self, tokens: &mut TokenStream) {
    let ts = if let Some(case) = self.default_handler {
      ResponseCaseFragment::new(case, self.response).into_token_stream()
    } else {
      let response_enum = &self.response.name;
      let unexpected = EnumVariantToken::from_raw(OTHER_RESPONSE_VARIANT);
      quote! {
        Ok(#response_enum::#unexpected(status, req.bytes().await?.to_vec()))
      }
    };

    tokens.extend(ts);
  }
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct BuilderMethodFragment<'a> {
  fields: &'a [BuilderField],
  nested_structs: &'a [BuilderNestedStruct],
  visibility: Visibility,
  docs: &'a Documentation,
}

impl<'a> BuilderMethodFragment<'a> {
  pub(crate) fn new(
    fields: &'a [BuilderField],
    nested_structs: &'a [BuilderNestedStruct],
    visibility: Visibility,
    docs: &'a Documentation,
  ) -> Self {
    Self {
      fields,
      nested_structs,
      visibility,
      docs,
    }
  }
}

impl ToTokens for BuilderMethodFragment<'_> {
  fn to_tokens(&self, tokens: &mut TokenStream) {
    let params: Vec<TokenStream> = self
      .fields
      .iter()
      .map(|f| {
        let name = &f.name;
        let ty = &f.rust_type;
        quote! { #name: #ty }
      })
      .collect();

    let construction = BuilderConstructionFragment::new(self.fields, self.nested_structs);
    let vis = &self.visibility;
    let docs = self.docs;

    tokens.extend(quote! {
      #docs
      #[builder]
      #vis fn new(#(#params),*) -> anyhow::Result<Self> {
        let request = #construction;
        request.validate()?;
        Ok(request)
      }
    });
  }
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct BuilderConstructionFragment<'a> {
  fields: &'a [BuilderField],
  nested_structs: &'a [BuilderNestedStruct],
}

impl<'a> BuilderConstructionFragment<'a> {
  pub(crate) fn new(fields: &'a [BuilderField], nested_structs: &'a [BuilderNestedStruct]) -> Self {
    Self { fields, nested_structs }
  }
}

impl ToTokens for BuilderConstructionFragment<'_> {
  fn to_tokens(&self, tokens: &mut TokenStream) {
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

    tokens.extend(quote! {
      Self { #(#assignments),* }
    });
  }
}

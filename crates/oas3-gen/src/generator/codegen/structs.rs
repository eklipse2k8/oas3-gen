use std::collections::{BTreeMap, BTreeSet};

use itertools::Itertools as _;
use mediatype::{MediaType, names};
use proc_macro2::TokenStream;
use quote::{ToTokens, format_ident, quote};

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
    BuilderField, BuilderNestedStruct, ContentCategory, DerivesProvider, Documentation, EventStreamFormat, FieldDef,
    FrameData, MethodKind, MethodNameToken, OperationResponse, RegexKey, ResponseMediaType, ResponseStatusCategory,
    ResponseVariantCategory, RustPrimitive, StatusCodeToken, StatusHandler, StreamItem, StructDef, StructKind,
    StructMethod, TypeRef, ValidationAttribute,
    tokens::{ConstToken, EnumVariantToken},
  },
  codegen::{
    attributes::generate_derives_from_slice,
    cookies::CookieParamsFragment,
    headers::{HeaderFromMapFragment, HeaderMapFragment},
    http::HttpStatusCode,
  },
  converter::{GenerationTarget, requests::category_media_type},
  naming::{constants::OTHER_RESPONSE_VARIANT, identifiers::builder_setter_name},
};

#[derive(Clone, Copy, Debug)]
pub struct StructFragment<'a> {
  def: &'a StructDef,
  regex_lookup: &'a BTreeMap<RegexKey, ConstToken>,
  visibility: Visibility,
  target: GenerationTarget,
}

impl<'a> StructFragment<'a> {
  pub fn new(
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
    let header_from_map = HeaderFromMapFragment::new(self.def, self.target);
    let cookies = CookieParamsFragment::new(self.def, self.target);

    tokens.extend(quote! {
      #definition

      #impl_block

      #header_map

      #header_from_map

      #cookies
    });
  }
}

#[derive(Clone, Copy, Debug)]
pub struct StructDefinitionFragment<'a> {
  def: &'a StructDef,
  regex_lookup: &'a BTreeMap<RegexKey, ConstToken>,
  visibility: Visibility,
}

impl<'a> StructDefinitionFragment<'a> {
  pub fn new(def: &'a StructDef, regex_lookup: &'a BTreeMap<RegexKey, ConstToken>, visibility: Visibility) -> Self {
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
pub struct StructFieldFragment<'a> {
  field: &'a FieldDef,
  struct_def: &'a StructDef,
  regex_lookup: &'a BTreeMap<RegexKey, ConstToken>,
  visibility: Visibility,
}

impl<'a> StructFieldFragment<'a> {
  pub fn new(
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

    let (serde_as, serde_attrs) = if self.struct_def.kind.is_header_struct()
      || matches!(self.struct_def.kind, StructKind::PathParams | StructKind::CookieParams)
    {
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
    let builder_attr = generate_builder_attrs(self.field);
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
pub struct StructImplBlockFragment<'a> {
  def: &'a StructDef,
  visibility: Visibility,
}

impl<'a> StructImplBlockFragment<'a> {
  pub fn new(def: &'a StructDef, visibility: Visibility) -> Self {
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
pub struct StructMethodFragment<'a> {
  method: &'a StructMethod,
  visibility: Visibility,
}

impl<'a> StructMethodFragment<'a> {
  pub fn new(method: &'a StructMethod, visibility: Visibility) -> Self {
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
pub struct ParseResponseMethodFragment<'a> {
  response: &'a OperationResponse,
  status_handlers: &'a [StatusHandler],
  default_handler: Option<&'a ResponseStatusCategory>,
  visibility: Visibility,
  method_name: &'a MethodNameToken,
  docs: &'a Documentation,
}

impl<'a> ParseResponseMethodFragment<'a> {
  pub fn new(
    response: &'a OperationResponse,
    status_handlers: &'a [StatusHandler],
    default_handler: Option<&'a ResponseStatusCategory>,
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
    let response = self.response;
    let status_checks = self
      .status_handlers
      .iter()
      .map(|handler| {
        (
          StatusConditionFragment::new(handler.status_code),
          ContentDispatchFragment::new(handler.dispatch.cases(), response, false),
        )
      })
      .collect::<Vec<_>>();
    let fallback = ContentDispatchFragment::new(
      self.default_handler.into_iter().flat_map(ResponseStatusCategory::cases),
      response,
      true,
    );

    let content_type = status_checks
      .iter()
      .map(|(_, dispatch)| dispatch)
      .chain([&fallback])
      .flat_map(|dispatch| &dispatch.cases)
      .any(|(_, check)| check.is_some())
      .then(|| {
        quote! {
          let content_type = req
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.split(';').next())
            .unwrap_or_default()
            .trim()
            .to_ascii_lowercase();
        }
      });
    let status_checks = status_checks.iter().map(|(condition, dispatch)| {
      quote! {
        if #condition {
          #dispatch
        }
      }
    });

    let vis = &self.visibility;
    let method_name = self.method_name;
    let docs = self.docs;

    tokens.extend(quote! {
      #docs
      #vis async fn #method_name(req: reqwest::Response) -> anyhow::Result<#response> {
        let status = req.status();
        #content_type
        #(#status_checks)*
        #fallback
      }
    });
  }
}

#[derive(Clone, Debug)]
pub struct StatusConditionFragment {
  status_code: StatusCodeToken,
}

impl StatusConditionFragment {
  pub fn new(status_code: StatusCodeToken) -> Self {
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

/// Reads a status-matched response as the first case whose media type it carries, or returns it
/// unread as `Other`.
///
/// A case that reads no body, or declares a media type that can't be parsed, reads any
/// response, so the cases after it are dropped. When a case declares a JSON type, no other case
/// falls back to JSON.
#[derive(Clone, Debug)]
pub struct ContentDispatchFragment<'a> {
  cases: Vec<(&'a ResponseVariantCategory, Option<ContentCheckFragment>)>,
  response: &'a OperationResponse,
  ends_method: bool,
}

impl<'a> ContentDispatchFragment<'a> {
  /// With `ends_method`, the unread response is the method's final expression rather than a
  /// `return`.
  pub fn new(
    cases: impl IntoIterator<Item = &'a ResponseVariantCategory>,
    response: &'a OperationResponse,
    ends_method: bool,
  ) -> Self {
    let mut cases = cases
      .into_iter()
      .map(|case| (case, ContentCheckFragment::new(case, response)))
      .take_while_inclusive(|(_, check)| check.is_some())
      .collect_vec();

    let declares_json = cases
      .iter()
      .filter_map(|(_, check)| check.as_ref())
      .any(|check| check.accepted.contains(&AcceptedMediaType::JSON));
    if declares_json {
      for check in cases.iter_mut().filter_map(|(_, check)| check.as_mut()) {
        check.json_fallback = false;
      }
    }

    Self {
      cases,
      response,
      ends_method,
    }
  }
}

impl ToTokens for ContentDispatchFragment<'_> {
  fn to_tokens(&self, tokens: &mut TokenStream) {
    for (case, check) in &self.cases {
      let block = ResponseCaseFragment::new(case, self.response);
      tokens.extend(match check {
        Some(check) => quote! {
          if #check {
            #block
          }
        },
        None => block.into_token_stream(),
      });
    }

    if self.cases.last().is_none_or(|(_, check)| check.is_some()) {
      let response_enum = &self.response.name;
      let other = EnumVariantToken::from_raw(OTHER_RESPONSE_VARIANT);
      let unread = quote! { Ok(#response_enum::#other(Box::new(req))) };
      tokens.extend(if self.ends_method {
        unread
      } else {
        quote! { return #unread; }
      });
    }
  }
}

/// Tests the response's lowercased, parameterless `content_type` against the media types a case
/// declares.
///
/// A binary type whose body decodes as JSON also accepts any JSON type, since a generated server
/// sends that body as JSON unless the status also declares a text type.
#[derive(Clone, Debug)]
pub struct ContentCheckFragment {
  accepted: Vec<AcceptedMediaType>,
  json_fallback: bool,
}

impl ContentCheckFragment {
  /// Returns `None` when the case reads no body, or declares no media type or one that can't
  /// be parsed.
  pub fn new(case: &ResponseVariantCategory, response: &OperationResponse) -> Option<Self> {
    let variant = &case.variant;
    let schema_type = variant.schema_type.as_ref()?;
    response.param(variant.mapping.payload)?;

    let mut accepted = vec![];
    for media in variant
      .media_types
      .iter()
      .filter(|media| media.category == case.category)
    {
      accepted.extend(AcceptedMediaType::for_media(media)?);
    }
    if accepted.is_empty() {
      return None;
    }

    let decodes_json = BodyReading::new(schema_type, case.category, variant.mapping.parses_text) == BodyReading::Json;
    let declares_text = variant
      .media_types
      .iter()
      .any(|media| media.category == ContentCategory::Text);
    Some(Self {
      accepted: accepted.into_iter().unique().collect(),
      json_fallback: case.category == ContentCategory::Binary && decodes_json && !declares_text,
    })
  }
}

impl ToTokens for ContentCheckFragment {
  fn to_tokens(&self, tokens: &mut TokenStream) {
    let accepted = self
      .accepted
      .iter()
      .chain(self.json_fallback.then_some(&AcceptedMediaType::JSON));
    tokens.extend(quote! { #(#accepted)||* });
  }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
enum AcceptedMediaType {
  /// Any type of a structured syntax, such as `json` for `application/json` or `+json` types.
  Syntax(&'static str),
  /// Every subtype of a top-level type, such as `image/`.
  Range(String),
  Exact(String),
}

impl AcceptedMediaType {
  const JSON: Self = Self::Syntax("json");
  const XML: Self = Self::Syntax("xml");

  /// Returns `None` when `media` can't be parsed.
  fn for_media(media: &ResponseMediaType) -> Option<Vec<Self>> {
    let parsed = MediaType::parse(&media.content_type).ok()?;
    let syntax = parsed.suffix.unwrap_or(parsed.subty);
    Some(if syntax == "json" {
      vec![Self::JSON]
    } else if syntax == "xml" {
      vec![Self::XML]
    } else if parsed.ty == names::_STAR {
      vec![Self::sent_under_wildcard(media.category)]
    } else if parsed.subty == names::_STAR {
      let range = format!("{}/", parsed.ty).to_ascii_lowercase();
      match Self::sent_under_wildcard(media.category) {
        Self::Exact(sent) if sent.starts_with(&range) => vec![Self::Range(range)],
        sent => vec![Self::Range(range), sent],
      }
    } else {
      vec![Self::Exact(parsed.essence().to_string().to_ascii_lowercase())]
    })
  }

  /// The type a generated server sends a body of `category` under when the status declares only
  /// a wildcard.
  fn sent_under_wildcard(category: ContentCategory) -> Self {
    match category {
      ContentCategory::Json => Self::JSON,
      ContentCategory::Xml => Self::XML,
      category => Self::Exact(category_media_type(category).to_owned()),
    }
  }
}

impl ToTokens for AcceptedMediaType {
  fn to_tokens(&self, tokens: &mut TokenStream) {
    tokens.extend(match self {
      Self::Syntax(syntax) => {
        let subtype = format!("/{syntax}");
        let suffix = format!("+{syntax}");
        quote! { content_type.ends_with(#subtype) || content_type.ends_with(#suffix) }
      }
      Self::Range(range) => quote! { content_type.starts_with(#range) },
      Self::Exact(essence) => quote! { content_type == #essence },
    });
  }
}

#[derive(Clone, Copy, Debug)]
pub struct ResponseCaseFragment<'a> {
  case: &'a ResponseVariantCategory,
  response: &'a OperationResponse,
}

impl<'a> ResponseCaseFragment<'a> {
  pub fn new(case: &'a ResponseVariantCategory, response: &'a OperationResponse) -> Self {
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
    let optional = param.is_some_and(|p| p.body.nullable);
    let headers = param.and_then(|p| p.headers.as_ref());
    let read_headers = headers.map(|headers| {
      let name = &headers.name;
      quote! { let headers = #name::try_from(req.headers())?; }
    });
    let with_headers = |body: TokenStream| match headers {
      Some(headers) => {
        let wrapper = &headers.wrapper;
        quote! { #wrapper { headers, body: #body } }
      }
      None => body,
    };

    let ts = match (variant.schema_type.as_ref(), param) {
      (_, None) => {
        let fields = carries_status.then(|| quote! { (status) });
        quote! {
          let _ = req.bytes().await?;
          return Ok(#response_enum::#variant_name #fields);
        }
      }
      (None, Some(_)) => {
        let empty = with_headers(if optional {
          quote! { None }
        } else {
          quote! { () }
        });
        quote! {
          #read_headers
          let _ = req.bytes().await?;
          return Ok(#response_enum::#variant_name(#status_arg #empty));
        }
      }
      (Some(schema_type), Some(param)) => {
        let stream = ResponseMediaType::event_stream(&variant.media_types).and_then(|media| media.stream.as_ref());
        let data = ResponseExtractionFragment::new(schema_type, self.case.category, mapping.parses_text, stream);
        let mut value = quote! { data };
        if let Some(union_variant) = &mapping.union_variant {
          let union = &param.body.base_type;
          value = quote! { #union::#union_variant(#value) };
        }
        if optional {
          value = quote! { Some(#value) };
        }
        let value = with_headers(value);
        quote! {
          #read_headers
          let data = #data;
          return Ok(#response_enum::#variant_name(#status_arg #value));
        }
      }
    };

    tokens.extend(ts);
  }
}

#[derive(Clone, Copy, Debug)]
pub struct ResponseExtractionFragment<'a> {
  schema_type: &'a TypeRef,
  category: ContentCategory,
  parses_text: bool,
  stream: Option<&'a EventStreamFormat>,
}

impl<'a> ResponseExtractionFragment<'a> {
  /// Reads the body as `category`; a `text/event-stream` body reads as `stream` declares
  /// it, decoding each frame's data as JSON or text, or a union of frame types by event
  /// name, and ending at its sentinel.
  pub fn new(
    schema_type: &'a TypeRef,
    category: ContentCategory,
    parses_text: bool,
    stream: Option<&'a EventStreamFormat>,
  ) -> Self {
    Self {
      schema_type,
      category,
      parses_text,
      stream,
    }
  }
}

/// How the client reads a response body.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum BodyReading {
  Bytes,
  Text,
  ParsedText,
  EventStream,
  Xml,
  Json,
}

impl BodyReading {
  fn new(schema_type: &TypeRef, category: ContentCategory, parses_text: bool) -> Self {
    match category {
      ContentCategory::Text | ContentCategory::Binary if matches!(schema_type.base_type, RustPrimitive::Bytes) => {
        Self::Bytes
      }
      ContentCategory::Text | ContentCategory::Binary if schema_type.is_string_like() => Self::Text,
      ContentCategory::Text if parses_text => Self::ParsedText,
      ContentCategory::EventStream => Self::EventStream,
      ContentCategory::Xml => Self::Xml,
      _ => Self::Json,
    }
  }
}

impl ToTokens for ResponseExtractionFragment<'_> {
  fn to_tokens(&self, tokens: &mut TokenStream) {
    let schema_type = self.schema_type;

    let ts = match BodyReading::new(schema_type, self.category, self.parses_text) {
      BodyReading::Bytes => quote! { req.bytes().await?.to_vec() },
      BodyReading::Text => quote! { req.text().await? },
      BodyReading::ParsedText => {
        if matches!(self.schema_type.base_type, RustPrimitive::Custom(_)) {
          quote! { req.text().await?.parse::<#schema_type>().map_err(anyhow::Error::msg)? }
        } else {
          quote! { req.text().await?.parse::<#schema_type>()? }
        }
      }
      BodyReading::EventStream => {
        let constructor = match self.stream.map(|stream| stream.item).unwrap_or_default() {
          StreamItem::Frame(FrameData::Text) => quote! { text },
          StreamItem::FrameUnion => quote! { frames },
          StreamItem::Payload | StreamItem::Frame(FrameData::Json) => quote! { json },
        };
        let sentinel = self
          .stream
          .and_then(|stream| stream.sentinel.as_deref())
          .map_or_else(|| quote! { None }, |sentinel| quote! { Some(#sentinel) });
        quote! { <#schema_type>::#constructor(req, #sentinel) }
      }
      BodyReading::Xml => {
        quote! { oas3_gen_support::Diagnostics::<#schema_type>::xml_with_diagnostics(req).await? }
      }
      BodyReading::Json => {
        quote! { oas3_gen_support::Diagnostics::<#schema_type>::json_with_diagnostics(req).await? }
      }
    };

    tokens.extend(ts);
  }
}

#[derive(Clone, Copy, Debug)]
pub struct BuilderMethodFragment<'a> {
  fields: &'a [BuilderField],
  nested_structs: &'a [BuilderNestedStruct],
  visibility: Visibility,
  docs: &'a Documentation,
}

impl<'a> BuilderMethodFragment<'a> {
  pub fn new(
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
        let setter = builder_setter_name(name.as_str()).map(|setter| {
          let setter = format_ident!("{setter}");
          quote! { #[builder(name = #setter)] }
        });
        quote! { #setter #name: #ty }
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
pub struct BuilderConstructionFragment<'a> {
  fields: &'a [BuilderField],
  nested_structs: &'a [BuilderNestedStruct],
}

impl<'a> BuilderConstructionFragment<'a> {
  pub fn new(fields: &'a [BuilderField], nested_structs: &'a [BuilderNestedStruct]) -> Self {
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

use proc_macro2::{Span, TokenStream};
use quote::{ToTokens, format_ident, quote};
use syn::{Ident, LitStr};

use super::server::bad_request;
use crate::generator::{
  ast::{
    HandlerBodyInfo, MultipartFieldInfo, MultipartPartKind, OperationBody, PartMembers, PartRfc6570, PartStyle,
    PartType, PartValue, RustPrimitive, TypeRef, is_json_media_type,
  },
  naming::constants::DEFAULT_MEDIA_TYPE,
};

/// Client statements that build a `multipart/form-data` body into `req_builder`.
///
/// Each struct field becomes its parts directly: file fields move their bytes into a file part,
/// and other fields convert to text by their [`PartValue`]. A body without a struct is split into
/// parts by serializing it to a JSON object. Part names are sent as raw UTF-8, as browsers do,
/// because RFC 7578 has no percent-encoded `name*` form. A form that may end up with no parts
/// sends only the closing boundary, since a multipart body without one is malformed.
#[derive(Clone, Debug)]
pub(crate) struct MultipartFormFragment {
  body: OperationBody,
}

impl MultipartFormFragment {
  pub(crate) fn new(body: OperationBody) -> Self {
    Self { body }
  }
}

impl ToTokens for MultipartFormFragment {
  fn to_tokens(&self, tokens: &mut TokenStream) {
    let field = &self.body.field_name;
    let closing_boundary_only = closing_boundary_only();
    let (prelude, parts, has_parts) = match self.body.multipart_fields.as_deref() {
      Some([]) => {
        let empty_form = quote! {
          let form = reqwest::multipart::Form::new();
          req_builder = #closing_boundary_only;
        };
        tokens.extend(if self.body.optional {
          quote! { if request.#field.is_some() { #empty_form } }
        } else {
          empty_form
        });
        return;
      }
      Some(fields) => (
        None,
        fields.iter().map(client_parts).collect::<TokenStream>(),
        has_parts(fields),
      ),
      None => {
        let name = quote! { name };
        let part = dynamic_part(&name, &quote! { value }, DEFAULT_MEDIA_TYPE, None);
        (
          Some(quote! {
            let entries = match serde_json::to_value(body)? {
              serde_json::Value::Object(entries) => entries,
              _ => serde_json::Map::new(),
            };
          }),
          quote! { for (name, value) in entries { #part } },
          Some(quote! { let has_parts = entries.values().any(|value| !value.is_null()); }),
        )
      }
    };
    let attach = if has_parts.is_some() {
      quote! {
        req_builder = if has_parts {
          req_builder.multipart(form)
        } else {
          #closing_boundary_only
        };
      }
    } else {
      quote! { req_builder = req_builder.multipart(form); }
    };
    let form = quote! {
      #prelude
      #has_parts
      let mut form = reqwest::multipart::Form::new().percent_encode_noop();
      #parts
      #attach
    };

    tokens.extend(if self.body.optional {
      quote! { if let Some(body) = request.#field { #form } }
    } else {
      quote! { let body = request.#field; #form }
    });
  }
}

/// A request body holding only the closing boundary of `form`: how a form without parts is sent,
/// since reqwest sends such a form as an empty body, which is not valid multipart.
fn closing_boundary_only() -> TokenStream {
  quote! {
    req_builder
      .header(http::header::CONTENT_TYPE, format!("multipart/form-data; boundary={}", form.boundary()))
      .body(format!("--{}--\r\n", form.boundary()))
  }
}

/// A `has_parts` binding that is `true` when at least one field yields a part, or `None` when
/// some field always does.
fn has_parts(fields: &[MultipartFieldInfo]) -> Option<TokenStream> {
  let conditions = fields
    .iter()
    .map(|field| {
      let ident = &field.name;
      match (&field.kind, field.rust_type.nullable, field.rust_type.is_array) {
        (MultipartPartKind::Flattened { repeated: true, .. }, ..) => {
          Some(quote! { body.#ident.values().any(|values| !values.is_empty()) })
        }
        (MultipartPartKind::Flattened { .. }, ..) | (_, false, true) => Some(quote! { !body.#ident.is_empty() }),
        (_, true, true) => Some(quote! { body.#ident.as_ref().is_some_and(|items| !items.is_empty()) }),
        (_, true, false) => Some(quote! { body.#ident.is_some() }),
        (_, false, false) => None,
      }
    })
    .collect::<Option<Vec<_>>>()?;
  let allow_deprecated = fields
    .iter()
    .any(|field| field.deprecated)
    .then(|| quote! { #[allow(deprecated)] });
  Some(quote! {
    #allow_deprecated
    let has_parts = #(#conditions)||*;
  })
}

/// Statements that add one body field's parts to `form`.
fn client_parts(field: &MultipartFieldInfo) -> TokenStream {
  let ident = &field.name;
  let name = field.part_name.as_str();
  let part_name = quote! { #name };
  let serialize_as = field.serialize_as.as_deref();
  let ts = match &field.kind {
    MultipartPartKind::File { content_type } => for_each_value(field, |value| {
      quote! {
        form = form.part(
          #name,
          reqwest::multipart::Part::bytes(#value).file_name(#name).mime_str(#content_type)?,
        );
      }
      .into()
    }),
    MultipartPartKind::Value { value, content_type } => for_each_value(field, |item| {
      value_part(&part_name, *value, content_type.as_deref(), item, serialize_as)
    }),
    MultipartPartKind::Styled {
      value,
      rfc6570,
      members,
    } => styled_parts(field, *value, *rfc6570, members.is_some()),
    MultipartPartKind::Flattened { entry, repeated } => {
      if *repeated {
        let part = value_part(&quote! { name.clone() }, entry.value, None, &quote! { value }, None);
        quote! { for (name, values) in body.#ident { for value in values { #part } } }
      } else {
        let part = value_part(&quote! { name }, entry.value, None, &quote! { value }, None);
        quote! { for (name, value) in body.#ident { #part } }
      }
    }
  };
  allow_deprecated(field, ts)
}

/// Statements that add one value's parts to `form`.
enum ValueStatements {
  Plain(TokenStream),
  /// Runs `body` only when the `let` pattern `condition` matches, joining the field's
  /// `if let Some` as a let chain.
  Guarded {
    condition: TokenStream,
    body: TokenStream,
  },
}

impl From<TokenStream> for ValueStatements {
  fn from(statements: TokenStream) -> Self {
    Self::Plain(statements)
  }
}

impl ToTokens for ValueStatements {
  fn to_tokens(&self, tokens: &mut TokenStream) {
    tokens.extend(match self {
      Self::Plain(statements) => statements.clone(),
      Self::Guarded { condition, body } => quote! { if #condition { #body } },
    });
  }
}

/// Wraps the statements for one value in the iteration the field's cardinality needs.
fn for_each_value(field: &MultipartFieldInfo, statements: impl Fn(&TokenStream) -> ValueStatements) -> TokenStream {
  let ident = &field.name;
  let TypeRef { nullable, is_array, .. } = field.rust_type;
  let value = quote! { value };
  match (nullable, is_array) {
    (false, false) => statements(&quote! { body.#ident }).into_token_stream(),
    (true, false) => match statements(&value) {
      ValueStatements::Plain(statements) => quote! { if let Some(value) = body.#ident { #statements } },
      ValueStatements::Guarded { condition, body } => {
        quote! { if let Some(value) = body.#ident && #condition { #body } }
      }
    },
    (false, true) => {
      let statements = statements(&value);
      quote! { for value in body.#ident { #statements } }
    }
    (true, true) => {
      let statements = statements(&value);
      quote! { for value in body.#ident.into_iter().flatten() { #statements } }
    }
  }
}

/// Statements that add `item` as the part named by the expression `name`.
fn value_part(
  name: &TokenStream,
  value: PartValue,
  content_type: Option<&str>,
  item: &TokenStream,
  serialize_as: Option<&str>,
) -> ValueStatements {
  let json_content_type = content_type
    .filter(|content_type| is_json_media_type(content_type))
    .unwrap_or(DEFAULT_MEDIA_TYPE);
  let serialized = serialized_value(item, serialize_as);

  match value {
    PartValue::String => text_part(name, item, content_type).into(),
    PartValue::StringEnum | PartValue::Scalar => text_part(name, &quote! { #item.to_string() }, content_type).into(),
    PartValue::Json => quote! {
      form = form.part(
        #name,
        reqwest::multipart::Part::text(serde_json::to_string(&#serialized)?).mime_str(#json_content_type)?,
      );
    }
    .into(),
    PartValue::SerdeString => ValueStatements::Guarded {
      condition: quote! { let serde_json::Value::String(text) = serde_json::to_value(#serialized)? },
      body: text_part(name, &quote! { text }, content_type),
    },
    PartValue::Dynamic => dynamic_part(
      name,
      &quote! { serde_json::to_value(#serialized)? },
      json_content_type,
      content_type,
    )
    .into(),
  }
}

/// A statement that adds `text` as the part named by `name`, labelled `content_type` when given.
fn text_part(name: &TokenStream, text: &TokenStream, content_type: Option<&str>) -> TokenStream {
  if let Some(content_type) = content_type {
    quote! {
      form = form.part(#name, reqwest::multipart::Part::text(#text).mime_str(#content_type)?);
    }
  } else {
    quote! { form = form.text(#name, #text); }
  }
}

/// Statements that add a JSON value of unknown shape as the part named by `name`: a string as
/// text, and anything else as JSON, so the receiver can tell `"5"` from `5`.
fn dynamic_part(
  name: &TokenStream,
  json_value: &TokenStream,
  json_content_type: &str,
  text_content_type: Option<&str>,
) -> TokenStream {
  let string_part = text_part(name, &quote! { text }, text_content_type);
  quote! {
    match #json_value {
      serde_json::Value::Null => {}
      serde_json::Value::String(text) => { #string_part }
      other => {
        form = form.part(
          #name,
          reqwest::multipart::Part::text(other.to_string()).mime_str(#json_content_type)?,
        );
      }
    }
  }
}

/// Parts for an RFC 6570 encoded field: arrays repeat or join their items, struct and map
/// properties send their members as parts, and everything else is one text part.
fn styled_parts(field: &MultipartFieldInfo, value: PartValue, rfc6570: PartRfc6570, is_object: bool) -> TokenStream {
  let ident = &field.name;
  let name = field.part_name.as_str();
  let delimiter = rfc6570.style.delimiter().to_string();
  let serialize_as = field.serialize_as.as_deref();
  let TypeRef { nullable, is_array, .. } = field.rust_type;

  if is_array && !rfc6570.explode {
    let items = if nullable {
      quote! { body.#ident.into_iter().flatten() }
    } else {
      quote! { body.#ident }
    };
    let text = item_text(value, &quote! { value }, serialize_as);
    return quote! {
      let mut items = vec![];
      for value in #items {
        items.push(#text);
      }
      if !items.is_empty() {
        form = form.text(#name, items.join(#delimiter));
      }
    };
  }

  if is_array || !is_object {
    return for_each_value(field, |item| {
      let text = item_text(value, item, serialize_as);
      quote! { form = form.text(#name, #text); }.into()
    });
  }

  let member_text = quote! {
    match member {
      serde_json::Value::Null => continue,
      serde_json::Value::String(text) => text,
      other => other.to_string(),
    }
  };
  let member_part_name = match (rfc6570.style, rfc6570.explode) {
    (PartStyle::DeepObject, _) => {
      let escaped = name.replace('{', "{{").replace('}', "}}");
      let pattern = LitStr::new(&format!("{escaped}[{{key}}]"), Span::call_site());
      Some(quote! { format!(#pattern) })
    }
    (_, true) => Some(quote! { key }),
    (_, false) => None,
  };
  let members = if let Some(member_part_name) = member_part_name {
    quote! {
      for (key, member) in members {
        let text = #member_text;
        form = form.text(#member_part_name, text);
      }
    }
  } else {
    quote! {
      let mut items = vec![];
      for (key, member) in members {
        let text = #member_text;
        items.push(key);
        items.push(text);
      }
      if !items.is_empty() {
        form = form.text(#name, items.join(#delimiter));
      }
    }
  };
  for_each_value(field, |item| ValueStatements::Guarded {
    condition: quote! { let serde_json::Value::Object(members) = serde_json::to_value(#item)? },
    body: members.clone(),
  })
}

/// Text for one value of a field, as RFC 6570 serialization writes it.
fn item_text(value: PartValue, item: &TokenStream, serialize_as: Option<&str>) -> TokenStream {
  match value {
    PartValue::String => item.clone(),
    PartValue::StringEnum | PartValue::Scalar => quote! { #item.to_string() },
    PartValue::SerdeString | PartValue::Json | PartValue::Dynamic => {
      let serialized = serialized_value(item, serialize_as);
      quote! {
        match serde_json::to_value(#serialized)? {
          serde_json::Value::String(text) => text,
          other => other.to_string(),
        }
      }
    }
  }
}

fn serialized_value(item: &TokenStream, serialize_as: Option<&str>) -> TokenStream {
  match serialize_as.map(RustPrimitive::from) {
    Some(serialize_as) => quote! { serde_with::ser::SerializeAsWrap::<_, #serialize_as>::new(&#item) },
    None => item.clone(),
  }
}

/// Scopes `#[allow(deprecated)]` to the code that reads or writes a deprecated field.
fn allow_deprecated(field: &MultipartFieldInfo, code: TokenStream) -> TokenStream {
  if field.deprecated {
    quote! { #[allow(deprecated)] { #code } }
  } else {
    code
  }
}

/// Server statements, at the top of a handler, that read a `multipart/form-data` body from an
/// axum `Multipart` extractor into `body`, answering `400` when the parts don't form one.
///
/// A nested function does the reading. Text parts are converted to the JSON form of their field
/// and collected into one object, which is then deserialized into the body type in one step, so
/// every field's serde attributes apply. File parts bypass JSON: their bytes are assigned after
/// deserialization, with an empty placeholder standing in for required file fields until then.
#[derive(Clone, Debug)]
pub(crate) struct MultipartDecodeFragment<'a> {
  body: &'a HandlerBodyInfo,
}

impl<'a> MultipartDecodeFragment<'a> {
  pub(crate) fn new(body: &'a HandlerBodyInfo) -> Self {
    Self { body }
  }
}

impl ToTokens for MultipartDecodeFragment<'_> {
  fn to_tokens(&self, tokens: &mut TokenStream) {
    let body_type = &self.body.body_type;
    let fields = self.body.multipart_fields.as_deref().unwrap_or_default();
    let exploded_map = fields.iter().find(|field| is_exploded_map(field));
    let receiving = fields
      .iter()
      .filter(|field| !is_exploded_map(field) || exploded_map.is_some_and(|map| map.part_name == field.part_name))
      .collect::<Vec<_>>();

    let locals = receiving
      .iter()
      .filter_map(|field| server_local(field))
      .collect::<Vec<_>>();
    let arms = fields.iter().filter_map(server_arm).collect::<Vec<_>>();
    let member_routes = fields.iter().filter_map(member_route);
    let catch_all = match self.body.multipart_fields.as_deref() {
      None => Some(insert_unmatched(&quote! { fields }, &dynamic_value(), false)),
      Some(fields) => catch_all(fields, exploded_map),
    };
    let collected = receiving
      .iter()
      .filter_map(|field| server_collected(field))
      .collect::<Vec<_>>();
    let assignments = fields.iter().filter_map(server_assignment).collect::<Vec<_>>();

    let dispatch = if arms.len() < 2 {
      let named = arms
        .into_iter()
        .map(|(part_name, handler)| (quote! { name == #part_name }, handler));
      if_chain(named.chain(member_routes), catch_all)
    } else {
      let arms = arms
        .iter()
        .map(|(part_name, handler)| quote! { #part_name => { #handler } });
      let unmatched = if_chain(member_routes, catch_all);
      quote! {
        match name.as_str() {
          #(#arms)*
          _ => { #unmatched }
        }
      }
    };
    let read_parts = if dispatch.is_empty() {
      quote! { while multipart.next_field().await?.is_some() {} }
    } else {
      quote! {
        while let Some(part) = multipart.next_field().await? {
          let Some(name) = part.name().map(str::to_owned) else {
            continue;
          };
          #dispatch
        }
      }
    };

    let fields_binding = if self
      .body
      .multipart_fields
      .as_deref()
      .is_some_and(|fields| fields.iter().all(is_optional_file))
    {
      quote! { fields }
    } else {
      quote! { mut fields }
    };
    let body_binding = if assignments.is_empty() {
      quote! { body }
    } else {
      quote! { mut body }
    };
    let allow_similar_names = (locals.len() > 1).then(|| quote! { #[allow(clippy::similar_names)] });
    let (parameter, return_type, unwrap, result) = if self.body.optional {
      (
        quote! { multipart: Option<axum::extract::Multipart> },
        quote! { Option<#body_type> },
        quote! {
          let Some(mut multipart) = multipart else {
            return Ok(None);
          };
        },
        quote! { Some(body) },
      )
    } else {
      (
        quote! { mut multipart: axum::extract::Multipart },
        quote! { #body_type },
        quote! {},
        quote! { body },
      )
    };
    let bad_request = bad_request();

    tokens.extend(quote! {
      #allow_similar_names
      async fn multipart_body(#parameter) -> anyhow::Result<#return_type> {
        #unwrap
        let #fields_binding = serde_json::Map::new();
        #(#locals)*
        #read_parts
        #(#collected)*
        let #body_binding: #body_type = serde_json::from_value(serde_json::Value::Object(fields))?;
        #(#assignments)*
        Ok(#result)
      }
      let body = match multipart_body(multipart).await {
        Ok(body) => body,
        Err(e) => return #bad_request,
      };
    });
  }
}

/// A condition, such as `name == "file"` or a `let` pattern, and the statements it guards.
type Branch = (TokenStream, TokenStream);

/// Joins branches into one `if` / `else if` chain, ending in `otherwise` when given.
fn if_chain(branches: impl IntoIterator<Item = Branch>, otherwise: Option<TokenStream>) -> TokenStream {
  let branches = branches
    .into_iter()
    .map(|(condition, statements)| quote! { if #condition { #statements } })
    .collect::<Vec<_>>();
  match (branches.is_empty(), otherwise) {
    (true, otherwise) => otherwise.unwrap_or_default(),
    (false, None) => quote! { #(#branches)else* },
    (false, Some(otherwise)) => quote! { #(#branches)else* else { #otherwise } },
  }
}

/// Local variable that collects a field's parts across the loop.
fn local_name(field: &MultipartFieldInfo) -> Ident {
  let name = field.name.as_str();
  format_ident!("{}_parts", name.strip_prefix("r#").unwrap_or(name))
}

fn is_optional_file(field: &MultipartFieldInfo) -> bool {
  matches!(field.kind, MultipartPartKind::File { .. }) && field.rust_type.nullable
}

/// The members and RFC 6570 encoding of a struct- or map-typed styled field.
fn styled_object(field: &MultipartFieldInfo) -> Option<(&PartMembers, PartRfc6570)> {
  match &field.kind {
    MultipartPartKind::Styled {
      rfc6570,
      members: Some(members),
      ..
    } => Some((members, *rfc6570)),
    _ => None,
  }
}

/// Whether a map property takes its entries from parts no property claims.
fn is_exploded_map(field: &MultipartFieldInfo) -> bool {
  styled_object(field).is_some_and(|(members, rfc6570)| {
    matches!(members, PartMembers::Map(_)) && rfc6570.style != PartStyle::DeepObject && rfc6570.explode
  })
}

/// The type of one value of a field, without its `Option` or collection wrappers.
fn field_part_type(field: &MultipartFieldInfo, value: PartValue) -> PartType {
  PartType {
    rust_type: field.rust_type.element_type(),
    value,
  }
}

fn server_local(field: &MultipartFieldInfo) -> Option<TokenStream> {
  let local = local_name(field);
  let is_array = field.rust_type.is_array;
  let ts = match &field.kind {
    MultipartPartKind::File { .. } if !is_array => quote! { let mut #local = None; },
    MultipartPartKind::Value { .. } | MultipartPartKind::Styled { .. } | MultipartPartKind::File { .. } if is_array => {
      quote! { let mut #local = vec![]; }
    }
    MultipartPartKind::Styled { members: Some(_), .. } | MultipartPartKind::Flattened { repeated: true, .. } => {
      quote! { let mut #local = serde_json::Map::new(); }
    }
    _ => return None,
  };
  Some(ts)
}

/// The part name a field matches and the statements that record such a part.
fn server_arm(field: &MultipartFieldInfo) -> Option<(&str, TokenStream)> {
  let name = field.part_name.as_str();
  let local = local_name(field);
  let is_array = field.rust_type.is_array;

  let handler = match &field.kind {
    MultipartPartKind::File { .. } if is_array => quote! { #local.push(Vec::from(part.bytes().await?)); },
    MultipartPartKind::File { .. } => quote! { #local = Some(Vec::from(part.bytes().await?)); },
    MultipartPartKind::Styled { value, rfc6570, .. } if is_array && !rfc6570.explode => {
      let delimiter = rfc6570.style.delimiter();
      let decoded = decode(
        &field_part_type(field, *value),
        &PartText::borrowed(quote! { item }),
        Some(name),
      );
      quote! {
        let text = part.text().await?;
        if !text.is_empty() {
          for item in text.split(#delimiter) {
            #local.push(#decoded);
          }
        }
      }
    }
    MultipartPartKind::Styled {
      rfc6570,
      members: Some(members),
      ..
    } if !is_array => {
      if rfc6570.style == PartStyle::DeepObject || rfc6570.explode {
        return None;
      }
      let delimiter = rfc6570.style.delimiter();
      let member = decode_member(members, &quote! { key }, &PartText::borrowed(quote! { member }), name);
      quote! {
        let text = part.text().await?;
        let mut tokens = text.split(#delimiter);
        while let (Some(key), Some(member)) = (tokens.next(), tokens.next()) {
          let member = #member;
          #local.insert(key.to_owned(), member);
        }
      }
    }
    MultipartPartKind::Value { value, .. } | MultipartPartKind::Styled { value, .. } => {
      let decoded = decode_part(&field_part_type(field, *value), Some(name));
      if is_array {
        quote! { #local.push(#decoded); }
      } else {
        quote! { fields.insert(#name.to_owned(), #decoded); }
      }
    }
    MultipartPartKind::Flattened { .. } => return None,
  };

  Some((name, handler))
}

/// Routes parts no property name matches to the styled object they belong to: `deepObject`
/// members arrive as `name[key]`, exploded `form` struct members under their own names.
fn member_route(field: &MultipartFieldInfo) -> Option<Branch> {
  let (members, rfc6570) = styled_object(field)?;
  let local = local_name(field);
  let name = field.part_name.as_str();
  if rfc6570.style == PartStyle::DeepObject {
    let prefix = format!("{name}[");
    let member = decode_member(members, &quote! { key }, &PartText::part(), name);
    return Some((
      quote! { let Some(key) = name.strip_prefix(#prefix).and_then(|key| key.strip_suffix(']')) },
      quote! {
        let member = #member;
        #local.insert(key.to_owned(), member);
      },
    ));
  }
  let PartMembers::Named(named) = members else {
    return None;
  };
  if !rfc6570.explode || named.is_empty() {
    return None;
  }
  let member_names = named.iter().map(|(member_name, _)| member_name.as_str());
  let member = decode_member(members, &quote! { name.as_str() }, &PartText::part(), name);
  Some((
    quote! { matches!(name.as_str(), #(#member_names)|*) },
    quote! {
      let member = #member;
      #local.insert(name, member);
    },
  ))
}

/// Statements for parts no property claims: the first exploded `form` map property takes them
/// as its entries, or else a flattened `additionalProperties` field does.
fn catch_all(fields: &[MultipartFieldInfo], exploded_map: Option<&MultipartFieldInfo>) -> Option<TokenStream> {
  if let Some(field) = exploded_map
    && let Some((PartMembers::Map(entry), _)) = styled_object(field)
  {
    return Some(insert_unmatched(&local_name(field).into_token_stream(), entry, false));
  }
  fields.iter().find_map(|field| match &field.kind {
    MultipartPartKind::Flattened { entry, repeated } => {
      let target = if *repeated {
        local_name(field).into_token_stream()
      } else {
        quote! { fields }
      };
      Some(insert_unmatched(&target, entry, *repeated))
    }
    _ => None,
  })
}

/// Inserts an unmatched part into the `target` map under its own name, appending to an array
/// when `repeated`.
fn insert_unmatched(target: &TokenStream, entry: &PartType, repeated: bool) -> TokenStream {
  let decoded = decode_part(entry, None);
  if repeated {
    quote! {
      let value = #decoded;
      if let serde_json::Value::Array(items) = #target.entry(name).or_insert_with(|| serde_json::Value::Array(vec![])) {
        items.push(value);
      }
    }
  } else {
    quote! {
      let value = #decoded;
      #target.insert(name, value);
    }
  }
}

/// Moves values collected across parts into `fields`.
fn server_collected(field: &MultipartFieldInfo) -> Option<TokenStream> {
  let name = field.part_name.as_str();
  let local = local_name(field);
  let nullable = field.rust_type.nullable;
  let (value, is_empty) = match &field.kind {
    MultipartPartKind::Flattened { repeated: true, .. } => return Some(quote! { fields.extend(#local); }),
    MultipartPartKind::File { .. } if !nullable => {
      return Some(quote! { fields.insert(#name.to_owned(), serde_json::Value::Array(vec![])); });
    }
    MultipartPartKind::Value { .. } | MultipartPartKind::Styled { .. } if field.rust_type.is_array => (
      quote! { serde_json::Value::Array(#local) },
      quote! { #local.is_empty() },
    ),
    MultipartPartKind::Styled { members: Some(_), .. } => (
      quote! { serde_json::Value::Object(#local) },
      quote! { #local.is_empty() },
    ),
    _ => return None,
  };
  Some(if nullable {
    quote! {
      if !#is_empty {
        fields.insert(#name.to_owned(), #value);
      }
    }
  } else {
    quote! { fields.insert(#name.to_owned(), #value); }
  })
}

/// Assigns collected file bytes to the deserialized body.
fn server_assignment(field: &MultipartFieldInfo) -> Option<TokenStream> {
  let MultipartPartKind::File { .. } = field.kind else {
    return None;
  };
  let ident = &field.name;
  let local = local_name(field);
  let name = field.part_name.as_str();
  let unique_items = field.rust_type.unique_items;
  let value = match (field.rust_type.nullable, field.rust_type.is_array) {
    (false, false) => quote! { #local.ok_or_else(|| anyhow::anyhow!("missing multipart part `{}`", #name))? },
    (false, true) if unique_items => quote! { #local.into_iter().collect() },
    (true, true) if unique_items => quote! { (!#local.is_empty()).then(|| #local.into_iter().collect()) },
    (true, false) | (false, true) => quote! { #local },
    (true, true) => quote! { (!#local.is_empty()).then_some(#local) },
  };
  Some(allow_deprecated(field, quote! { body.#ident = #value; }))
}

/// Part text in the forms decoding needs: an owned `String`, and a `&str` to parse from.
struct PartText {
  owned: TokenStream,
  borrowed: TokenStream,
}

impl PartText {
  /// The text of the current `part`, read once.
  fn part() -> Self {
    Self {
      owned: quote! { part.text().await? },
      borrowed: quote! { &part.text().await? },
    }
  }

  /// A `&str` slice of already-read text.
  fn borrowed(text: TokenStream) -> Self {
    Self {
      owned: quote! { #text.to_owned() },
      borrowed: text,
    }
  }
}

/// Converts a whole part to the JSON value its field deserializes from. A `Dynamic` value is
/// parsed as JSON when the part is labelled JSON, which is how the generated client sends
/// structured values.
fn decode_part(part_type: &PartType, part_name: Option<&str>) -> TokenStream {
  if part_type.value != PartValue::Dynamic {
    return decode(part_type, &PartText::part(), part_name);
  }
  let parsed = parse_json(&quote! { &text }, part_name);
  let probed = string_or_json(&part_type.rust_type);
  quote! {
    {
      let is_json = part
        .content_type()
        .is_some_and(|content_type| content_type.contains("json"));
      let text = part.text().await?;
      if is_json { #parsed } else #probed
    }
  }
}

/// An expression that keeps the owned `text` as a string when `rust_type` accepts one, and
/// otherwise parses it as JSON, falling back to the string when it isn't JSON.
fn string_or_json(rust_type: &TypeRef) -> TokenStream {
  quote! {
    if serde_json::from_value::<#rust_type>(serde_json::Value::String(text.clone())).is_ok() {
      serde_json::Value::String(text)
    } else {
      serde_json::from_str(&text).unwrap_or(serde_json::Value::String(text))
    }
  }
}

/// Converts part text to the JSON value its type deserializes from. Text that must be JSON fails
/// with an error naming the part; `part_name` is `None` when the part is bound to `name`.
///
/// A `Dynamic` value stays a string when its type accepts one, and is otherwise parsed as JSON,
/// falling back to the string when it isn't JSON.
fn decode(part_type: &PartType, text: &PartText, part_name: Option<&str>) -> TokenStream {
  let PartText { owned, borrowed } = text;
  match part_type.value {
    PartValue::String | PartValue::StringEnum | PartValue::SerdeString => quote! { serde_json::Value::String(#owned) },
    PartValue::Scalar | PartValue::Json => parse_json(borrowed, part_name),
    PartValue::Dynamic => {
      let probed = string_or_json(&part_type.rust_type);
      quote! {
        {
          let text = #owned;
          #probed
        }
      }
    }
  }
}

/// Decodes an object member's text by the type of the member named `key`. Members whose text is
/// their value share the catch-all arm.
fn decode_member(members: &PartMembers, key: &TokenStream, text: &PartText, part_name: &str) -> TokenStream {
  match members {
    PartMembers::Map(entry) => decode(entry, text, Some(part_name)),
    PartMembers::Named(named) => {
      let arms = named
        .iter()
        .filter(|(_, member_type)| {
          !matches!(
            member_type.value,
            PartValue::String | PartValue::StringEnum | PartValue::SerdeString
          )
        })
        .map(|(member_name, member_type)| {
          let decoded = decode(member_type, text, Some(part_name));
          quote! { #member_name => #decoded, }
        })
        .collect::<Vec<_>>();
      let owned = &text.owned;
      if arms.is_empty() {
        quote! { serde_json::Value::String(#owned) }
      } else {
        quote! {
          match #key {
            #(#arms)*
            _ => serde_json::Value::String(#owned),
          }
        }
      }
    }
  }
}

fn parse_json(text: &TokenStream, part_name: Option<&str>) -> TokenStream {
  let error = if let Some(part_name) = part_name {
    quote! { anyhow::anyhow!("invalid multipart part `{}`: {e}", #part_name) }
  } else {
    quote! { anyhow::anyhow!("invalid multipart part `{name}`: {e}") }
  };
  quote! { serde_json::from_str(#text).map_err(|e| #error)? }
}

/// The decoding a body without a struct uses for every part.
fn dynamic_value() -> PartType {
  PartType {
    rust_type: TypeRef::new(RustPrimitive::Value),
    value: PartValue::Dynamic,
  }
}

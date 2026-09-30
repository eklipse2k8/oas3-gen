use std::collections::{BTreeMap, BTreeSet, HashMap};

use indexmap::IndexMap;
use itertools::Itertools;

use crate::generator::{
  ast::{
    Documentation, EnumToken, EnumVariantToken, FieldDef, FieldNameToken, MethodKind, MethodNameToken, OperationInfo,
    OperationResponse, ResponseEnumDef, ResponseEnumVariant, ResponseHeadersRef, ResponseMediaType, ResponseParam,
    ResponsePayload, ResponseStatusCategory, ResponseUnionDef, ResponseUnionVariant, ResponseVariant,
    ResponseVariantCategory, RustPrimitive, RustType, SerdeMode, StatusCodeToken, StatusHandler, StructDef, StructKind,
    StructMethod, StructToken, TypeRef, VariantMapping,
  },
  converter::GenerationTarget,
  naming::{
    constants::{OTHER_RESPONSE_VARIANT, RESPONSE_FALLBACK_NAME, RESPONSE_NAME, WITH_HEADERS_NAME},
    identifiers::ensure_unique,
    responses::{headers_name, union_name},
  },
};

/// Whether any operation gives a status code a payload, keyed by status code.
type StatusTable = BTreeMap<StatusCodeToken, bool>;

/// One header merged across a status class: its field, and how many of the
/// class's status codes require it.
type MergedHeader = (FieldDef, usize);

/// One declared body within a status class: its rendered type, the type, and the
/// variant it came from.
type Body<'a> = (String, &'a TypeRef, &'a ResponseVariant);

pub(crate) struct ResponseProcessor {
  types: Vec<RustType>,
  operations: Vec<OperationInfo>,
  target: GenerationTarget,
  table: StatusTable,
  unions: IndexMap<String, ResponseUnionDef>,
  headers: IndexMap<String, StructDef>,
  taken: BTreeSet<String>,
}

impl ResponseProcessor {
  pub(crate) fn new(types: Vec<RustType>, operations: Vec<OperationInfo>, target: GenerationTarget) -> Self {
    Self {
      types,
      operations,
      target,
      table: StatusTable::new(),
      unions: IndexMap::new(),
      headers: IndexMap::new(),
      taken: BTreeSet::new(),
    }
  }

  pub(crate) fn process(mut self) -> (Vec<RustType>, Vec<OperationInfo>) {
    let Some(table) = status_table(&self.operations) else {
      return (self.types, self.operations);
    };
    self.table = table;
    self.taken = self.types.iter().map(|t| t.type_name().to_string()).collect();

    let response_enum = self.build_response_enum();
    let request_structs = self
      .types
      .iter()
      .enumerate()
      .filter_map(|(index, rust_type)| match rust_type {
        RustType::Struct(def) => Some((def.name.clone(), index)),
        _ => None,
      })
      .collect::<HashMap<StructToken, usize>>();

    let mut operations = std::mem::take(&mut self.operations);
    for op in &mut operations {
      let Some(variants) = op.response_variants.as_mut() else {
        continue;
      };

      let response = self.resolve_operation(&response_enum, variants);

      if self.target == GenerationTarget::Client
        && let Some(index) = op.request_type.as_ref().and_then(|name| request_structs.get(name))
        && let RustType::Struct(request) = &mut self.types[*index]
      {
        request.methods.push(parse_method(&response, variants));
      }

      op.response = Some(response);
    }

    self.mark_header_enums();
    self.types.push(RustType::ResponseEnum(response_enum));
    self
      .types
      .extend(self.unions.into_values().map(RustType::ResponseUnion));
    self.types.extend(self.headers.into_values().map(RustType::Struct));

    (self.types, operations)
  }

  fn build_response_enum(&mut self) -> ResponseEnumDef {
    let name = if self.taken.contains(RESPONSE_NAME) {
      RESPONSE_FALLBACK_NAME
    } else {
      RESPONSE_NAME
    };
    self.taken.insert(name.to_string());

    let declared = self
      .table
      .iter()
      .sorted_by_key(|(status, _)| (status.code().is_none(), status.code().unwrap_or_default(), **status))
      .map(|(status, has_body)| {
        ResponseEnumVariant::builder()
          .name(status.to_variant_token())
          .status_code(*status)
          .payload(if *has_body {
            ResponsePayload::for_status(*status)
          } else {
            ResponsePayload::None
          })
          .build()
      });

    let needs_other = self
      .operations
      .iter()
      .filter_map(|op| op.response_variants.as_deref())
      .any(|variants| !variants.iter().any(|v| v.status_code.is_default()));

    let other = needs_other.then(|| {
      ResponseEnumVariant::builder()
        .name(EnumVariantToken::from_raw(OTHER_RESPONSE_VARIANT))
        .status_code(StatusCodeToken::Default)
        .payload(ResponsePayload::Raw)
        .build()
    });

    let any_headers = self
      .operations
      .iter()
      .filter_map(|op| op.response_variants.as_deref())
      .flatten()
      .any(|v| !v.headers.is_empty());
    let with_headers = any_headers.then(|| {
      let name = ensure_unique(WITH_HEADERS_NAME, &self.taken);
      self.taken.insert(name.clone());
      StructToken::new(name)
    });

    ResponseEnumDef::builder()
      .name(EnumToken::from_raw(name))
      .docs(Documentation::from_lines([
        "Response enum shared by every operation.",
        "",
        "`Value` is the operation's success body type and `Failure` its error body type.",
        "A status code is a unit variant when no operation gives it a body.",
      ]))
      .variants(declared.chain(other).collect())
      .maybe_with_headers(with_headers)
      .build()
  }

  fn resolve_operation(
    &mut self,
    response_enum: &ResponseEnumDef,
    variants: &mut [ResponseVariant],
  ) -> OperationResponse {
    let wrapper = response_enum.with_headers.as_ref();
    let value = response_enum
      .has_param(ResponsePayload::Value)
      .then(|| self.resolve_param(ResponsePayload::Value, variants, wrapper));
    let failure = response_enum
      .has_param(ResponsePayload::Failure)
      .then(|| self.resolve_param(ResponsePayload::Failure, variants, wrapper));

    OperationResponse::builder()
      .name(response_enum.name.clone())
      .maybe_value(value)
      .maybe_failure(failure)
      .build()
  }

  fn resolve_param(
    &mut self,
    class: ResponsePayload,
    variants: &mut [ResponseVariant],
    wrapper: Option<&StructToken>,
  ) -> ResponseParam {
    let headers = self
      .resolve_headers(class, variants)
      .zip(wrapper)
      .map(|(name, wrapper)| {
        ResponseHeadersRef::builder()
          .wrapper(wrapper.clone())
          .name(name)
          .build()
      });

    ResponseParam::builder()
      .body(self.resolve_payload(class, variants))
      .maybe_headers(headers)
      .build()
  }

  /// Chooses the body type for one payload class of an operation and records
  /// how each of its variants maps onto the shared enum.
  ///
  /// A single body type is used as is, several become a response union, and a
  /// bodiless status alongside a body makes the parameter `Option<_>`. With no
  /// body anywhere in the class the parameter is `()`.
  fn resolve_payload(&mut self, class: ResponsePayload, variants: &mut [ResponseVariant]) -> TypeRef {
    let keyed = variants
      .iter()
      .enumerate()
      .filter(|(_, v)| v.payload() == class && self.table[&v.status_code])
      .map(|(index, v)| (index, v.schema_type.as_ref().map(TypeRef::to_rust_type)))
      .collect::<Vec<_>>();

    let any_bodiless = keyed.iter().any(|(_, key)| key.is_none());
    let mut bodies = keyed
      .iter()
      .filter_map(|(index, key)| {
        let variant = &variants[*index];
        Some((key.clone()?, variant.schema_type.as_ref()?, variant))
      })
      .collect::<Vec<Body<'_>>>();
    bodies.sort_by(|a, b| a.0.cmp(&b.0));
    bodies.dedup_by(|a, b| a.0 == b.0);

    let (param, union) = match bodies.as_slice() {
      [] => (TypeRef::new(RustPrimitive::Unit), None),
      [(_, single, _)] => ((*single).clone(), None),
      many => {
        let name = self.response_union(many).name.clone();
        (TypeRef::new(name.to_string()), Some(name))
      }
    };

    let param = if any_bodiless && !bodies.is_empty() {
      param.with_option()
    } else {
      param
    };

    for (index, key) in keyed {
      variants[index].mapping = VariantMapping::builder()
        .payload(class)
        .maybe_union_variant(union.as_ref().and(key.as_deref()).map(EnumVariantToken::from_raw))
        .build();
    }

    param
  }

  /// Merges the headers declared across one payload class of an operation into
  /// a headers struct, returning its name, or `None` when the class declares none.
  ///
  /// A header stays required only when every status code in the class requires it.
  fn resolve_headers(&mut self, class: ResponsePayload, variants: &[ResponseVariant]) -> Option<StructToken> {
    if !variants.iter().any(|v| v.payload() == class && !v.headers.is_empty()) {
      return None;
    }

    let statuses = variants
      .iter()
      .filter(|v| v.payload() == class)
      .unique_by(|v| v.status_code)
      .collect::<Vec<_>>();

    let mut merged = IndexMap::<FieldNameToken, MergedHeader>::new();
    for field in statuses.iter().flat_map(|v| &v.headers) {
      let (_, required) = merged.entry(field.name.clone()).or_insert_with(|| (field.clone(), 0));
      *required += usize::from(!field.rust_type.nullable);
    }

    let fields = merged
      .into_values()
      .map(|(mut field, required)| {
        field.rust_type.nullable = required < statuses.len();
        field
      })
      .collect::<Vec<_>>();

    Some(self.headers_struct(fields))
  }

  /// Returns the name of the headers struct over the given fields, creating it on
  /// first use.
  ///
  /// Structs are keyed by their sorted fields, so status classes declaring the same
  /// headers share one definition.
  fn headers_struct(&mut self, fields: Vec<FieldDef>) -> StructToken {
    let key = fields
      .iter()
      .map(|field| format!("{}: {}", field.name, field.rust_type.to_rust_type()))
      .sorted()
      .join(", ");

    let def = self.headers.entry(key).or_insert_with(|| {
      let name = headers_name(
        fields.iter().filter_map(|field| field.original_name.as_deref()),
        &self.taken,
      );
      self.taken.insert(name.clone());

      StructDef::builder()
        .name(StructToken::new(name))
        .docs(Documentation::from_lines([
          "Response headers that share one status class in an operation.",
        ]))
        .fields(fields)
        .kind(StructKind::ResponseHeaders)
        .build()
    });

    def.name.clone()
  }

  /// Flags the enums that response headers parse into, so they implement `FromStr`
  /// on every target.
  fn mark_header_enums(&mut self) {
    if self.headers.is_empty() {
      return;
    }

    let parsed = self
      .headers
      .values()
      .flat_map(|def| &def.fields)
      .map(|field| field.rust_type.unboxed_base_type_name())
      .collect::<BTreeSet<_>>();

    for rust_type in &mut self.types {
      if let RustType::Enum(def) = rust_type
        && parsed.contains(def.name.as_str())
      {
        def.in_response_header = true;
      }
    }
  }

  /// Returns the union over the given bodies, creating it on first use.
  ///
  /// Unions are keyed by their sorted member types, so operations with the same
  /// set of bodies share one definition.
  fn response_union(&mut self, members: &[Body<'_>]) -> &ResponseUnionDef {
    let key = members.iter().map(|(name, _, _)| name.as_str()).join("|");

    self.unions.entry(key).or_insert_with(|| {
      let name = union_name(members.iter().map(|(name, _, _)| name.as_str()), &self.taken);
      self.taken.insert(name.clone());

      let streaming = members
        .iter()
        .any(|(_, _, v)| ResponseMediaType::has_event_stream(&v.media_types));
      let serde_mode = match self.target {
        GenerationTarget::Client => SerdeMode::None,
        GenerationTarget::Server => SerdeMode::SerializeOnly,
      };

      ResponseUnionDef::builder()
        .name(EnumToken::from_raw(name))
        .docs(Documentation::from_lines([
          "Response bodies that share one status class in an operation.",
        ]))
        .variants(
          members
            .iter()
            .map(|(type_name, rust_type, _)| {
              ResponseUnionVariant::builder()
                .name(EnumVariantToken::from_raw(type_name))
                .rust_type((*rust_type).clone())
                .build()
            })
            .collect(),
        )
        .serde_mode(serde_mode)
        .streaming(streaming)
        .build()
    })
  }
}

/// Collects every declared status code and whether any operation gives it a payload.
///
/// A status code has a payload when it declares a body, or when any status code in
/// its class within the same operation declares headers.
///
/// Returns `None` when no operation declares responses, in which case no response
/// enum is generated.
fn status_table(operations: &[OperationInfo]) -> Option<StatusTable> {
  let mut table = StatusTable::new();
  let mut any_responses = false;

  for variants in operations.iter().filter_map(|op| op.response_variants.as_deref()) {
    any_responses = true;
    let headed = variants
      .iter()
      .filter(|v| !v.headers.is_empty())
      .map(ResponseVariant::payload)
      .collect::<Vec<_>>();
    for variant in variants {
      *table.entry(variant.status_code).or_default() |=
        variant.schema_type.is_some() || headed.contains(&variant.payload());
    }
  }

  any_responses.then_some(table)
}

fn parse_method(response: &OperationResponse, variants: &[ResponseVariant]) -> StructMethod {
  let (status_handlers, default_handler) = partition_handlers(variants);

  StructMethod::builder()
    .name(MethodNameToken::from_raw("parse_response"))
    .docs(Documentation::from_lines([
      "Parse the HTTP response into the response enum.",
    ]))
    .kind(MethodKind::ParseResponse {
      response: response.clone(),
      status_handlers,
      default_handler,
    })
    .build()
}

/// Groups variants by status code into handlers and splits off the `default` handler.
fn partition_handlers(variants: &[ResponseVariant]) -> (Vec<StatusHandler>, Option<ResponseVariantCategory>) {
  let (default_variants, status_variants): (Vec<_>, Vec<_>) = variants.iter().partition(|v| v.status_code.is_default());

  let status_handlers = status_variants
    .into_iter()
    .fold(
      IndexMap::<StatusCodeToken, Vec<&ResponseVariant>>::new(),
      |mut acc, v| {
        acc.entry(v.status_code).or_default().push(v);
        acc
      },
    )
    .into_iter()
    .map(|(code, group)| StatusHandler {
      status_code: code,
      dispatch: ResponseStatusCategory::from_variants(&group),
    })
    .collect();

  let default_handler = default_variants.first().map(|v| ResponseVariantCategory {
    category: ResponseMediaType::primary_category(&v.media_types),
    variant: (*v).clone(),
  });

  (status_handlers, default_handler)
}

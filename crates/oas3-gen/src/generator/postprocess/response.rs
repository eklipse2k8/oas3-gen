use std::collections::{BTreeMap, BTreeSet, HashMap};

use indexmap::IndexMap;
use itertools::Itertools;

use crate::generator::{
  ast::{
    Documentation, EnumToken, EnumVariantToken, MethodKind, MethodNameToken, OperationInfo, OperationResponse,
    ResponseEnumDef, ResponseEnumVariant, ResponseMediaType, ResponsePayload, ResponseStatusCategory, ResponseUnionDef,
    ResponseUnionVariant, ResponseVariant, ResponseVariantCategory, RustPrimitive, RustType, SerdeMode,
    StatusCodeToken, StatusHandler, StructMethod, StructToken, TypeRef, VariantMapping,
  },
  converter::GenerationTarget,
  naming::{
    constants::{OTHER_RESPONSE_VARIANT, RESPONSE_FALLBACK_NAME, RESPONSE_NAME},
    responses::union_name,
  },
};

/// Whether any operation gives a status code a body, keyed by status code.
type StatusTable = BTreeMap<StatusCodeToken, bool>;

/// One declared body within a status class: its rendered type, the type, and the
/// variant it came from.
type Body<'a> = (String, &'a TypeRef, &'a ResponseVariant);

pub(crate) struct ResponseProcessor {
  types: Vec<RustType>,
  operations: Vec<OperationInfo>,
  target: GenerationTarget,
  table: StatusTable,
  unions: IndexMap<String, ResponseUnionDef>,
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

    self.types.push(RustType::ResponseEnum(response_enum));
    self
      .types
      .extend(self.unions.into_values().map(RustType::ResponseUnion));

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

    ResponseEnumDef::builder()
      .name(EnumToken::from_raw(name))
      .docs(Documentation::from_lines([
        "Response enum shared by every operation.",
        "",
        "`Value` is the operation's success body type and `Failure` its error body type.",
        "A status code is a unit variant when no operation gives it a body.",
      ]))
      .variants(declared.chain(other).collect())
      .build()
  }

  fn resolve_operation(
    &mut self,
    response_enum: &ResponseEnumDef,
    variants: &mut [ResponseVariant],
  ) -> OperationResponse {
    let value = response_enum
      .has_param(ResponsePayload::Value)
      .then(|| self.resolve_payload(ResponsePayload::Value, variants));
    let failure = response_enum
      .has_param(ResponsePayload::Failure)
      .then(|| self.resolve_payload(ResponsePayload::Failure, variants));

    OperationResponse::builder()
      .name(response_enum.name.clone())
      .maybe_value(value)
      .maybe_failure(failure)
      .build()
  }

  /// Chooses the type parameter for one payload class of an operation and records
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

/// Collects every declared status code and whether any operation gives it a body.
///
/// Returns `None` when no operation declares responses, in which case no response
/// enum is generated.
fn status_table(operations: &[OperationInfo]) -> Option<StatusTable> {
  let mut table = StatusTable::new();
  let mut any_responses = false;

  for variants in operations.iter().filter_map(|op| op.response_variants.as_deref()) {
    any_responses = true;
    for variant in variants {
      *table.entry(variant.status_code).or_default() |= variant.schema_type.is_some();
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

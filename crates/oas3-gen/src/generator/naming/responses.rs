use std::collections::{BTreeSet, HashSet};

use itertools::Itertools;
use oas3::{
  Spec,
  spec::{Operation, Response},
};

use crate::{
  generator::naming::{
    constants::{RESPONSE_UNION_FALLBACK_SUFFIX, RESPONSE_UNION_SEPARATOR},
    identifiers::{ensure_unique, to_rust_type_name},
  },
  utils::SchemaRefName,
};

const SUCCESS_RESPONSE_PREFIX: char = '2';

pub struct ResponseTypes {
  pub success: Vec<String>,
  pub error: Vec<String>,
}

pub fn extract_response_type_name(spec: &Spec, operation: &Operation) -> Option<String> {
  find_primary_response(spec, operation)
    .and_then(|resp| extract_schema_name_from_response(&resp))
    .map(|s| to_rust_type_name(&s))
}

pub fn extract_all_response_content_types(spec: &Spec, operation: &Operation) -> Vec<String> {
  find_primary_response(spec, operation)
    .map(|resp| resp.content.keys().cloned().collect())
    .unwrap_or_default()
}

pub fn extract_all_response_types(spec: &Spec, operation: &Operation) -> ResponseTypes {
  let mut success_set = HashSet::new();
  let mut error_set = HashSet::new();

  let Some(responses) = operation.responses.as_ref() else {
    return ResponseTypes {
      success: vec![],
      error: vec![],
    };
  };

  for (code, resp_ref) in responses {
    if let Ok(resp) = resp_ref.resolve(spec)
      && let Some(schema_name) = extract_schema_name_from_response(&resp)
    {
      let rust_name = to_rust_type_name(&schema_name);
      if is_success_code(code) {
        success_set.insert(rust_name);
      } else if is_error_code(code) {
        error_set.insert(rust_name);
      }
    }
  }

  ResponseTypes {
    success: success_set.into_iter().collect(),
    error: error_set.into_iter().collect(),
  }
}

fn find_primary_response(spec: &Spec, operation: &Operation) -> Option<Response> {
  let responses = operation.responses.as_ref()?;
  responses
    .iter()
    .find(|(code, _)| code.starts_with(SUCCESS_RESPONSE_PREFIX))
    .or_else(|| responses.iter().next())
    .and_then(|(_, resp_ref)| resp_ref.resolve(spec).ok())
}

pub fn is_success_code(code: &str) -> bool {
  code.starts_with(SUCCESS_RESPONSE_PREFIX)
}

pub fn is_error_code(code: &str) -> bool {
  code.starts_with('4') || code.starts_with('5')
}

pub fn extract_schema_name_from_response(response: &Response) -> Option<String> {
  response
    .content
    .values()
    .next()?
    .schema
    .as_ref()
    .and_then(SchemaRefName::schema_ref_name)
}

/// Names the union over several response body types, e.g. `BasicErrorOrValidationError`.
///
/// `members` are rendered Rust types in canonical order. A name already in use gets a
/// `Body` suffix, then a numeric one.
pub(crate) fn union_name<'a>(members: impl IntoIterator<Item = &'a str>, taken: &BTreeSet<String>) -> String {
  let base = members
    .into_iter()
    .map(to_rust_type_name)
    .join(RESPONSE_UNION_SEPARATOR);
  if taken.contains(&base) {
    ensure_unique(&format!("{base}{RESPONSE_UNION_FALLBACK_SUFFIX}"), taken)
  } else {
    base
  }
}

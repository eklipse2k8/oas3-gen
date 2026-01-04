use crate::generator::naming::{
  constants::{REQUEST_PARAMS_SUFFIX, REQUEST_SUFFIX, RESPONSE_ENUM_SUFFIX, RESPONSE_SUFFIX},
  identifiers::{split_snake_case, to_rust_field_name, to_rust_type_name},
  inference::{all_non_empty_and_unique, common_prefix_len, common_suffix_len, extract_middle_segments},
};

pub fn generate_unique_response_name<F>(base_name: &str, is_taken: F) -> String
where
  F: Fn(&str) -> bool,
{
  let mut response_name = format!("{base_name}{RESPONSE_SUFFIX}");
  let rust_response_name = to_rust_type_name(&response_name);

  if is_taken(&rust_response_name) {
    response_name = format!("{base_name}{RESPONSE_SUFFIX}{RESPONSE_ENUM_SUFFIX}");
  }

  response_name
}

pub fn generate_unique_request_name<F>(base_name: &str, is_taken: F) -> String
where
  F: Fn(&str) -> bool,
{
  let mut request_name = format!("{base_name}{REQUEST_SUFFIX}");
  let rust_request_name = to_rust_type_name(&request_name);

  if is_taken(&rust_request_name) {
    request_name = format!("{base_name}{REQUEST_SUFFIX}{REQUEST_PARAMS_SUFFIX}");
  }

  request_name
}

/// Simplifies operation IDs by stripping common prefix and suffix segments.
///
/// Returns simplified names if they remain unique and non-empty after stripping,
/// otherwise returns the original names unchanged.
///
/// # Example
///
/// ```text
/// ["api_users_list", "api_users_get", "api_users_create"]
/// => ["list", "get", "create"]
/// ```
pub fn trim_common_affixes<S>(ids: &[S]) -> Vec<String>
where
  S: AsRef<str>,
{
  let to_owned = || ids.iter().map(|s| s.as_ref().to_owned()).collect();

  let segments: Vec<_> = match ids {
    [] | [_] => return to_owned(),
    _ => ids.iter().map(|s| split_snake_case(s.as_ref())).collect(),
  };

  let (first, rest) = segments.split_first().unwrap();
  let mut prefix_len = common_prefix_len(first, rest);
  let mut suffix_len = common_suffix_len(first, rest);

  if prefix_len == 0 && suffix_len == 0 {
    return to_owned();
  }

  let min_len = segments.iter().map(Vec::len).min().unwrap_or(0);
  while prefix_len + suffix_len >= min_len && (prefix_len > 0 || suffix_len > 0) {
    if suffix_len > 0 {
      suffix_len -= 1;
    } else {
      prefix_len -= 1;
    }
  }

  if prefix_len == 0 && suffix_len == 0 {
    return to_owned();
  }

  let simplified = segments
    .iter()
    .map(|s| extract_middle_segments(s, prefix_len, suffix_len, "_"))
    .collect::<Vec<_>>();

  if all_non_empty_and_unique(&simplified) {
    simplified
  } else {
    to_owned()
  }
}

pub fn compute_stable_id<S>(method: S, path: S, operation_id: Option<S>) -> String
where
  S: AsRef<str>,
{
  to_rust_field_name(&operation_id.map_or_else(|| generate_operation_id(method, path), |s| s.as_ref().to_string()))
}

pub(crate) fn generate_operation_id<S>(method: S, path: S) -> String
where
  S: AsRef<str>,
{
  let path_parts = path
    .as_ref()
    .split('/')
    .filter(|s| !s.is_empty())
    .map(|s| {
      if s.starts_with('{') && s.ends_with('}') {
        "by_id"
      } else {
        s
      }
    })
    .collect::<Vec<_>>();

  if path_parts.is_empty() {
    method.as_ref().to_lowercase()
  } else {
    format!("{}_{}", method.as_ref(), path_parts.join("_")).to_lowercase()
  }
}

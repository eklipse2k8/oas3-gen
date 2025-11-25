use crate::generator::naming::{
  constants::{REQUEST_PARAMS_SUFFIX, REQUEST_SUFFIX, RESPONSE_ENUM_SUFFIX, RESPONSE_SUFFIX},
  identifiers::to_rust_type_name,
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

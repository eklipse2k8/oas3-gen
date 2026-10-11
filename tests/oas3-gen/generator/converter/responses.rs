use oas3_gen::generator::ast::{StatusCodeToken, TypeRef};
use rstest::rstest;

use super::convert;

#[rstest]
fn only_declared_statuses_with_a_body_or_a_specific_code_become_variants() {
  let converted = convert(spec!("converter/operations.json"), config!());
  let variants = |operation: &str| {
    converted
      .operation(operation)
      .response_variants
      .iter()
      .flatten()
      .map(|variant| {
        (
          variant.status_code,
          variant.schema_type.as_ref().map(TypeRef::to_rust_type),
        )
      })
      .collect::<Vec<_>>()
  };

  assert_eq!(variants("ping"), [(StatusCodeToken::NoContent204, None)]);
  assert_eq!(
    variants("get_item"),
    [
      (StatusCodeToken::Ok200, Some("serde_json::Value".to_string())),
      (StatusCodeToken::Default, Some("Error".to_string())),
    ]
  );
}

#[rstest]
fn response_headers_become_variant_fields_except_content_type() {
  let converted = convert(spec!("converter/operations.json"), config!());
  let operation = converted.operation("get_user_post");
  let headers = |status: StatusCodeToken| {
    operation
      .response_variants
      .iter()
      .flatten()
      .find(|variant| variant.status_code == status)
      .map(|variant| {
        variant
          .headers
          .iter()
          .map(|header| {
            format!(
              "{} {}: {}",
              header.original_name.as_deref().unwrap_or_default(),
              header.name,
              header.rust_type.to_rust_type()
            )
          })
          .collect::<Vec<_>>()
      })
  };

  assert_eq!(
    headers(StatusCodeToken::Ok200).unwrap_or_default(),
    [
      "X-Rate-Limit x_rate_limit: i32",
      "x-mode x_mode: Option<GetUserPostResponseHeaderXMode>",
      "x-trace x_trace: Option<String>",
    ]
  );
  assert_eq!(headers(StatusCodeToken::NotFound404), Some(vec![]));
}

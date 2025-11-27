use oas3::spec::Response;

use crate::generator::ast::{StatusCodeToken, status_codes::status_code_to_variant_name};

#[test]
fn test_from_openapi_specific_codes() {
  let cases = [
    ("200", StatusCodeToken::Ok200),
    ("201", StatusCodeToken::Created201),
    ("204", StatusCodeToken::NoContent204),
    ("400", StatusCodeToken::BadRequest400),
    ("404", StatusCodeToken::NotFound404),
    ("500", StatusCodeToken::InternalServerError500),
  ];
  for (input, expected) in cases {
    assert_eq!(StatusCodeToken::from_openapi(input), expected, "failed for {input}");
  }
}

#[test]
fn test_from_openapi_wildcards() {
  let cases = [
    ("1XX", StatusCodeToken::Informational1XX),
    ("1xx", StatusCodeToken::Informational1XX),
    ("2XX", StatusCodeToken::Success2XX),
    ("2xx", StatusCodeToken::Success2XX),
    ("4XX", StatusCodeToken::ClientError4XX),
    ("5XX", StatusCodeToken::ServerError5XX),
  ];
  for (input, expected) in cases {
    assert_eq!(StatusCodeToken::from_openapi(input), expected, "failed for {input}");
  }
}

#[test]
fn test_from_openapi_default_and_unknown() {
  assert_eq!(StatusCodeToken::from_openapi("default"), StatusCodeToken::Default);
  assert_eq!(StatusCodeToken::from_openapi("418"), StatusCodeToken::Unknown(418));
  assert_eq!(StatusCodeToken::from_openapi("invalid"), StatusCodeToken::Default);
}

#[test]
fn test_code() {
  assert_eq!(StatusCodeToken::Ok200.code(), Some(200));
  assert_eq!(StatusCodeToken::NotFound404.code(), Some(404));
  assert_eq!(StatusCodeToken::Unknown(418).code(), Some(418));
  assert_eq!(StatusCodeToken::Success2XX.code(), None);
  assert_eq!(StatusCodeToken::Default.code(), None);
}

#[test]
fn test_variant_name() {
  assert_eq!(StatusCodeToken::Ok200.variant_name(), "Ok");
  assert_eq!(StatusCodeToken::NotFound404.variant_name(), "NotFound");
  assert_eq!(StatusCodeToken::Success2XX.variant_name(), "Success");
  assert_eq!(StatusCodeToken::Default.variant_name(), "Unknown");
}

#[test]
fn test_display() {
  assert_eq!(StatusCodeToken::Ok200.to_string(), "200");
  assert_eq!(StatusCodeToken::Success2XX.to_string(), "2XX");
  assert_eq!(StatusCodeToken::Unknown(418).to_string(), "418");
  assert_eq!(StatusCodeToken::Default.to_string(), "default");
}

#[test]
fn test_is_success() {
  assert!(StatusCodeToken::Ok200.is_success());
  assert!(StatusCodeToken::Created201.is_success());
  assert!(StatusCodeToken::Success2XX.is_success());
  assert!(!StatusCodeToken::NotFound404.is_success());
  assert!(!StatusCodeToken::Default.is_success());
}

fn make_response(description: Option<&str>) -> Response {
  Response {
    description: description.map(ToString::to_string),
    ..Default::default()
  }
}

#[test]
fn test_known_status_codes() {
  let cases = [
    (StatusCodeToken::Ok200, "Ok"),
    (StatusCodeToken::Created201, "Created"),
    (StatusCodeToken::NotFound404, "NotFound"),
    (StatusCodeToken::InternalServerError500, "InternalServerError"),
  ];
  for (token, expected) in cases {
    let result = status_code_to_variant_name(token, &make_response(None));
    assert_eq!(result.to_string(), expected, "failed for {token:?}");
  }
}

#[test]
fn test_wildcard_status_codes() {
  let cases = [
    (StatusCodeToken::Informational1XX, "Informational"),
    (StatusCodeToken::Success2XX, "Success"),
    (StatusCodeToken::Redirection3XX, "Redirection"),
    (StatusCodeToken::ClientError4XX, "ClientError"),
    (StatusCodeToken::ServerError5XX, "ServerError"),
  ];
  for (token, expected) in cases {
    let result = status_code_to_variant_name(token, &make_response(None));
    assert_eq!(result.to_string(), expected, "failed for {token:?}");
  }
}

#[test]
fn test_default_status_code() {
  let result = status_code_to_variant_name(StatusCodeToken::Default, &make_response(None));
  assert_eq!(result.to_string(), "Unknown");
}

#[test]
fn test_unknown_with_description() {
  let response = make_response(Some("I'm a teapot"));
  let result = status_code_to_variant_name(StatusCodeToken::Unknown(418), &response);
  assert_eq!(result.to_string(), "ImATeapot");
}

#[test]
fn test_unknown_without_description() {
  let result = status_code_to_variant_name(StatusCodeToken::Unknown(418), &make_response(None));
  assert_eq!(result.to_string(), "Status418");
}

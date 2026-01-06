use crate::generator::ast::StatusCodeToken;

#[test]
fn test_parse_openapi_specific_codes() {
  let cases = [
    // 1xx
    ("100", StatusCodeToken::Continue100),
    ("101", StatusCodeToken::SwitchingProtocols101),
    ("102", StatusCodeToken::Processing102),
    ("103", StatusCodeToken::EarlyHints103),
    // 2xx
    ("200", StatusCodeToken::Ok200),
    ("201", StatusCodeToken::Created201),
    ("202", StatusCodeToken::Accepted202),
    ("203", StatusCodeToken::NonAuthoritativeInformation203),
    ("204", StatusCodeToken::NoContent204),
    ("205", StatusCodeToken::ResetContent205),
    ("206", StatusCodeToken::PartialContent206),
    ("207", StatusCodeToken::MultiStatus207),
    ("208", StatusCodeToken::AlreadyReported208),
    ("226", StatusCodeToken::ImUsed226),
    // 3xx
    ("300", StatusCodeToken::MultipleChoices300),
    ("301", StatusCodeToken::MovedPermanently301),
    ("302", StatusCodeToken::Found302),
    ("303", StatusCodeToken::SeeOther303),
    ("304", StatusCodeToken::NotModified304),
    ("305", StatusCodeToken::UseProxy305),
    ("307", StatusCodeToken::TemporaryRedirect307),
    ("308", StatusCodeToken::PermanentRedirect308),
    // 4xx
    ("400", StatusCodeToken::BadRequest400),
    ("401", StatusCodeToken::Unauthorized401),
    ("402", StatusCodeToken::PaymentRequired402),
    ("403", StatusCodeToken::Forbidden403),
    ("404", StatusCodeToken::NotFound404),
    ("405", StatusCodeToken::MethodNotAllowed405),
    ("406", StatusCodeToken::NotAcceptable406),
    ("407", StatusCodeToken::ProxyAuthenticationRequired407),
    ("408", StatusCodeToken::RequestTimeout408),
    ("409", StatusCodeToken::Conflict409),
    ("410", StatusCodeToken::Gone410),
    ("411", StatusCodeToken::LengthRequired411),
    ("412", StatusCodeToken::PreconditionFailed412),
    ("413", StatusCodeToken::ContentTooLarge413),
    ("414", StatusCodeToken::UriTooLong414),
    ("415", StatusCodeToken::UnsupportedMediaType415),
    ("416", StatusCodeToken::RangeNotSatisfiable416),
    ("417", StatusCodeToken::ExpectationFailed417),
    ("421", StatusCodeToken::MisdirectedRequest421),
    ("422", StatusCodeToken::UnprocessableEntity422),
    ("423", StatusCodeToken::Locked423),
    ("424", StatusCodeToken::FailedDependency424),
    ("425", StatusCodeToken::TooEarly425),
    ("426", StatusCodeToken::UpgradeRequired426),
    ("428", StatusCodeToken::PreconditionRequired428),
    ("429", StatusCodeToken::TooManyRequests429),
    ("431", StatusCodeToken::RequestHeaderFieldsTooLarge431),
    ("451", StatusCodeToken::UnavailableForLegalReasons451),
    // 5xx
    ("500", StatusCodeToken::InternalServerError500),
    ("501", StatusCodeToken::NotImplemented501),
    ("502", StatusCodeToken::BadGateway502),
    ("503", StatusCodeToken::ServiceUnavailable503),
    ("504", StatusCodeToken::GatewayTimeout504),
    ("505", StatusCodeToken::HttpVersionNotSupported505),
    ("506", StatusCodeToken::VariantAlsoNegotiates506),
    ("507", StatusCodeToken::InsufficientStorage507),
    ("508", StatusCodeToken::LoopDetected508),
    ("511", StatusCodeToken::NetworkAuthenticationRequired511),
  ];
  for (input, expected) in cases {
    assert_eq!(
      input.parse::<StatusCodeToken>().unwrap(),
      expected,
      "parse() failed for {input}"
    );
    assert_eq!(
      expected.code(),
      Some(input.parse().unwrap()),
      "code() failed for {input}"
    );
    assert_eq!(expected.to_string(), input, "to_string() failed for {input}");
  }
}

#[test]
fn test_parse_wildcards() {
  let cases = [
    ("1XX", StatusCodeToken::Informational1XX),
    ("1xx", StatusCodeToken::Informational1XX),
    ("2XX", StatusCodeToken::Success2XX),
    ("2xx", StatusCodeToken::Success2XX),
    ("3XX", StatusCodeToken::Redirection3XX),
    ("3xx", StatusCodeToken::Redirection3XX),
    ("4XX", StatusCodeToken::ClientError4XX),
    ("4xx", StatusCodeToken::ClientError4XX),
    ("5XX", StatusCodeToken::ServerError5XX),
    ("5xx", StatusCodeToken::ServerError5XX),
  ];
  for (input, expected) in cases {
    assert_eq!(
      input.parse::<StatusCodeToken>().unwrap(),
      expected,
      "failed for {input}"
    );
  }
}

#[test]
fn test_parse_default_and_unknown() {
  assert_eq!("default".parse::<StatusCodeToken>().unwrap(), StatusCodeToken::Default);
  assert_eq!("invalid".parse::<StatusCodeToken>().unwrap(), StatusCodeToken::Default);

  // Explicitly excluded codes should parse as Unknown
  assert_eq!("104".parse::<StatusCodeToken>().unwrap(), StatusCodeToken::Unknown(104));
  assert_eq!("306".parse::<StatusCodeToken>().unwrap(), StatusCodeToken::Unknown(306));
  assert_eq!("418".parse::<StatusCodeToken>().unwrap(), StatusCodeToken::Unknown(418));
  assert_eq!("510".parse::<StatusCodeToken>().unwrap(), StatusCodeToken::Unknown(510));

  // Random unknown code
  assert_eq!("599".parse::<StatusCodeToken>().unwrap(), StatusCodeToken::Unknown(599));
}

#[test]
fn test_code_helpers() {
  assert_eq!(StatusCodeToken::Ok200.code(), Some(200));
  assert_eq!(StatusCodeToken::Continue100.code(), Some(100));
  assert_eq!(StatusCodeToken::NotFound404.code(), Some(404));
  assert_eq!(StatusCodeToken::Unknown(418).code(), Some(418));
  assert_eq!(StatusCodeToken::Success2XX.code(), None);
  assert_eq!(StatusCodeToken::Default.code(), None);
}

#[test]
fn test_variant_name() {
  assert_eq!(StatusCodeToken::Ok200.variant_name(), "Ok");
  assert_eq!(StatusCodeToken::Continue100.variant_name(), "Continue");
  assert_eq!(StatusCodeToken::NotFound404.variant_name(), "NotFound");
  assert_eq!(StatusCodeToken::Success2XX.variant_name(), "Success");
  assert_eq!(StatusCodeToken::Default.variant_name(), "Unknown");
  assert_eq!(StatusCodeToken::Unknown(418).variant_name(), "Unknown");
}

#[test]
fn test_is_success() {
  // Existing
  assert!(StatusCodeToken::Ok200.is_success());
  assert!(StatusCodeToken::Created201.is_success());

  // New
  assert!(StatusCodeToken::NonAuthoritativeInformation203.is_success());
  assert!(StatusCodeToken::ResetContent205.is_success());
  assert!(StatusCodeToken::PartialContent206.is_success());
  assert!(StatusCodeToken::MultiStatus207.is_success());
  assert!(StatusCodeToken::AlreadyReported208.is_success());
  assert!(StatusCodeToken::ImUsed226.is_success());

  // Wildcard
  assert!(StatusCodeToken::Success2XX.is_success());

  // Negative cases
  assert!(!StatusCodeToken::Continue100.is_success());
  assert!(!StatusCodeToken::NotFound404.is_success());
  assert!(!StatusCodeToken::Default.is_success());
}

#[test]
fn test_known_status_codes_variant_generation() {
  let cases = [
    (StatusCodeToken::Ok200, "Ok"),
    (StatusCodeToken::Created201, "Created"),
    (StatusCodeToken::Continue100, "Continue"),
    (StatusCodeToken::NotFound404, "NotFound"),
    (StatusCodeToken::InternalServerError500, "InternalServerError"),
    (StatusCodeToken::ImUsed226, "ImUsed"),
  ];
  for (token, expected) in cases {
    let result = token.to_variant_token();
    assert_eq!(result.to_string(), expected, "failed for {token:?}");
  }
}

#[test]
fn test_wildcard_status_codes_variant_generation() {
  let cases = [
    (StatusCodeToken::Informational1XX, "Informational"),
    (StatusCodeToken::Success2XX, "Success"),
    (StatusCodeToken::Redirection3XX, "Redirection"),
    (StatusCodeToken::ClientError4XX, "ClientError"),
    (StatusCodeToken::ServerError5XX, "ServerError"),
  ];
  for (token, expected) in cases {
    let result = token.to_variant_token();
    assert_eq!(result.to_string(), expected, "failed for {token:?}");
  }
}

#[test]
fn test_default_status_code_variant_generation() {
  let result = StatusCodeToken::Default.to_variant_token();
  assert_eq!(result.to_string(), "Unknown");
}

#[test]
fn test_unknown_status_code_variant_generation() {
  assert_eq!(
    StatusCodeToken::Unknown(418).to_variant_token().to_string(),
    "Status418"
  );
  assert_eq!(
    StatusCodeToken::Unknown(599).to_variant_token().to_string(),
    "Status599"
  );
  assert_eq!(
    StatusCodeToken::Unknown(104).to_variant_token().to_string(),
    "Status104"
  );
}

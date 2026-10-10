use std::collections::{BTreeMap, BTreeSet};

use oas3_gen::generator::ast::StatusCodeToken;
use rstest::rstest;

const NAMED_STATUS_CODES: [(u16, &str); 60] = [
  (100, "Continue"),
  (101, "SwitchingProtocols"),
  (102, "Processing"),
  (103, "EarlyHints"),
  (200, "Ok"),
  (201, "Created"),
  (202, "Accepted"),
  (203, "NonAuthoritativeInformation"),
  (204, "NoContent"),
  (205, "ResetContent"),
  (206, "PartialContent"),
  (207, "MultiStatus"),
  (208, "AlreadyReported"),
  (226, "ImUsed"),
  (300, "MultipleChoices"),
  (301, "MovedPermanently"),
  (302, "Found"),
  (303, "SeeOther"),
  (304, "NotModified"),
  (305, "UseProxy"),
  (307, "TemporaryRedirect"),
  (308, "PermanentRedirect"),
  (400, "BadRequest"),
  (401, "Unauthorized"),
  (402, "PaymentRequired"),
  (403, "Forbidden"),
  (404, "NotFound"),
  (405, "MethodNotAllowed"),
  (406, "NotAcceptable"),
  (407, "ProxyAuthenticationRequired"),
  (408, "RequestTimeout"),
  (409, "Conflict"),
  (410, "Gone"),
  (411, "LengthRequired"),
  (412, "PreconditionFailed"),
  (413, "ContentTooLarge"),
  (414, "UriTooLong"),
  (415, "UnsupportedMediaType"),
  (416, "RangeNotSatisfiable"),
  (417, "ExpectationFailed"),
  (421, "MisdirectedRequest"),
  (422, "UnprocessableEntity"),
  (423, "Locked"),
  (424, "FailedDependency"),
  (425, "TooEarly"),
  (426, "UpgradeRequired"),
  (428, "PreconditionRequired"),
  (429, "TooManyRequests"),
  (431, "RequestHeaderFieldsTooLarge"),
  (451, "UnavailableForLegalReasons"),
  (500, "InternalServerError"),
  (501, "NotImplemented"),
  (502, "BadGateway"),
  (503, "ServiceUnavailable"),
  (504, "GatewayTimeout"),
  (505, "HttpVersionNotSupported"),
  (506, "VariantAlsoNegotiates"),
  (507, "InsufficientStorage"),
  (508, "LoopDetected"),
  (511, "NetworkAuthenticationRequired"),
];

#[rstest]
fn status_codes_round_trip_and_name_distinct_response_variants() -> anyhow::Result<()> {
  let named = BTreeMap::from(NAMED_STATUS_CODES);
  for code in 100u16..=599 {
    let token = code.to_string().parse::<StatusCodeToken>()?;
    assert_eq!(token.to_string(), code.to_string(), "display of {code}");
    assert_eq!(token.code(), Some(code), "code of {code}");
    let expected_variant = named
      .get(&code)
      .map_or_else(|| format!("Status{code}"), ToString::to_string);
    assert_eq!(
      token.to_variant_token().to_string(),
      expected_variant,
      "variant for {code}"
    );
    if named.contains_key(&code) {
      assert_eq!(token.is_success(), (200..300).contains(&code), "success of {code}");
    }
  }
  assert_eq!(
    named.values().collect::<BTreeSet<_>>().len(),
    NAMED_STATUS_CODES.len(),
    "variant names are distinct"
  );

  for (raw, display, variant, is_success) in [
    ("1XX", "1XX", "Informational", false),
    ("2xx", "2XX", "Success", true),
    ("3XX", "3XX", "Redirection", false),
    ("4xx", "4XX", "ClientError", false),
    ("5XX", "5XX", "ServerError", false),
    ("default", "default", "Unknown", false),
    ("invalid", "default", "Unknown", false),
  ] {
    let token = raw.parse::<StatusCodeToken>()?;
    assert_eq!(token.to_string(), display, "display of {raw}");
    assert_eq!(token.code(), None, "code of {raw}");
    assert_eq!(token.to_variant_token().to_string(), variant, "variant for {raw}");
    assert_eq!(token.is_success(), is_success, "success of {raw}");
  }
  Ok(())
}

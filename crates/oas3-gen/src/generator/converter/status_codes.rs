use oas3::spec::Response;

use crate::generator::naming::identifiers::to_rust_type_name;

pub(crate) const STATUS_OK: &str = "Ok";
pub(crate) const STATUS_CREATED: &str = "Created";
pub(crate) const STATUS_ACCEPTED: &str = "Accepted";
pub(crate) const STATUS_NO_CONTENT: &str = "NoContent";
pub(crate) const STATUS_MOVED_PERMANENTLY: &str = "MovedPermanently";
pub(crate) const STATUS_FOUND: &str = "Found";
pub(crate) const STATUS_NOT_MODIFIED: &str = "NotModified";
pub(crate) const STATUS_BAD_REQUEST: &str = "BadRequest";
pub(crate) const STATUS_UNAUTHORIZED: &str = "Unauthorized";
pub(crate) const STATUS_FORBIDDEN: &str = "Forbidden";
pub(crate) const STATUS_NOT_FOUND: &str = "NotFound";
pub(crate) const STATUS_METHOD_NOT_ALLOWED: &str = "MethodNotAllowed";
pub(crate) const STATUS_NOT_ACCEPTABLE: &str = "NotAcceptable";
pub(crate) const STATUS_REQUEST_TIMEOUT: &str = "RequestTimeout";
pub(crate) const STATUS_CONFLICT: &str = "Conflict";
pub(crate) const STATUS_GONE: &str = "Gone";
pub(crate) const STATUS_UNPROCESSABLE_ENTITY: &str = "UnprocessableEntity";
pub(crate) const STATUS_TOO_MANY_REQUESTS: &str = "TooManyRequests";
pub(crate) const STATUS_INTERNAL_SERVER_ERROR: &str = "InternalServerError";
pub(crate) const STATUS_NOT_IMPLEMENTED: &str = "NotImplemented";
pub(crate) const STATUS_BAD_GATEWAY: &str = "BadGateway";
pub(crate) const STATUS_SERVICE_UNAVAILABLE: &str = "ServiceUnavailable";
pub(crate) const STATUS_GATEWAY_TIMEOUT: &str = "GatewayTimeout";

pub(crate) const STATUS_INFORMATIONAL: &str = "Informational";
pub(crate) const STATUS_SUCCESS: &str = "Success";
pub(crate) const STATUS_REDIRECTION: &str = "Redirection";
pub(crate) const STATUS_CLIENT_ERROR: &str = "ClientError";
pub(crate) const STATUS_SERVER_ERROR: &str = "ServerError";
pub(crate) const STATUS_PREFIX: &str = "Status";

pub(crate) const STATUS_CODE_MAP: &[(&str, &str)] = &[
  ("200", STATUS_OK),
  ("201", STATUS_CREATED),
  ("202", STATUS_ACCEPTED),
  ("204", STATUS_NO_CONTENT),
  ("301", STATUS_MOVED_PERMANENTLY),
  ("302", STATUS_FOUND),
  ("304", STATUS_NOT_MODIFIED),
  ("400", STATUS_BAD_REQUEST),
  ("401", STATUS_UNAUTHORIZED),
  ("403", STATUS_FORBIDDEN),
  ("404", STATUS_NOT_FOUND),
  ("405", STATUS_METHOD_NOT_ALLOWED),
  ("406", STATUS_NOT_ACCEPTABLE),
  ("408", STATUS_REQUEST_TIMEOUT),
  ("409", STATUS_CONFLICT),
  ("410", STATUS_GONE),
  ("422", STATUS_UNPROCESSABLE_ENTITY),
  ("429", STATUS_TOO_MANY_REQUESTS),
  ("500", STATUS_INTERNAL_SERVER_ERROR),
  ("501", STATUS_NOT_IMPLEMENTED),
  ("502", STATUS_BAD_GATEWAY),
  ("503", STATUS_SERVICE_UNAVAILABLE),
  ("504", STATUS_GATEWAY_TIMEOUT),
];

pub(crate) fn status_code_to_variant_name(status_code: &str, response: &Response) -> String {
  if let Some((_, name)) = STATUS_CODE_MAP.iter().find(|(code, _)| *code == status_code) {
    return (*name).to_string();
  }

  match status_code {
    "1XX" => return STATUS_INFORMATIONAL.to_string(),
    "2XX" => return STATUS_SUCCESS.to_string(),
    "3XX" => return STATUS_REDIRECTION.to_string(),
    "4XX" => return STATUS_CLIENT_ERROR.to_string(),
    "5XX" => return STATUS_SERVER_ERROR.to_string(),
    _ => {}
  }

  if status_code.ends_with("XX") || status_code.ends_with("xx") {
    let prefix = &status_code[0..1];
    return match prefix {
      "1" => STATUS_INFORMATIONAL.to_string(),
      "2" => STATUS_SUCCESS.to_string(),
      "3" => STATUS_REDIRECTION.to_string(),
      "4" => STATUS_CLIENT_ERROR.to_string(),
      "5" => STATUS_SERVER_ERROR.to_string(),
      _ => format!("{STATUS_PREFIX}{}", status_code.replace(['X', 'x'], "")),
    };
  }

  if let Some(desc) = &response.description {
    let sanitized = desc
      .chars()
      .filter(|c| c.is_alphanumeric() || c.is_whitespace())
      .collect::<String>();
    let words: Vec<&str> = sanitized.split_whitespace().take(3).collect();
    if !words.is_empty() {
      return to_rust_type_name(&words.join("_"));
    }
  }

  format!("{STATUS_PREFIX}{status_code}")
}

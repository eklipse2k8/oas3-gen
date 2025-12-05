use std::fmt::{Display, Formatter};

use oas3::spec::Response;

use crate::generator::{ast::EnumVariantToken, naming::identifiers::to_rust_type_name};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Default)]
pub enum StatusCodeToken {
  Ok200,
  Created201,
  Accepted202,
  NoContent204,
  MovedPermanently301,
  Found302,
  NotModified304,
  BadRequest400,
  Unauthorized401,
  Forbidden403,
  NotFound404,
  MethodNotAllowed405,
  NotAcceptable406,
  RequestTimeout408,
  Conflict409,
  Gone410,
  UnprocessableEntity422,
  TooManyRequests429,
  InternalServerError500,
  NotImplemented501,
  BadGateway502,
  ServiceUnavailable503,
  GatewayTimeout504,
  Informational1XX,
  Success2XX,
  Redirection3XX,
  ClientError4XX,
  ServerError5XX,
  #[default]
  Default,
  Unknown(u16),
}

impl StatusCodeToken {
  pub fn from_openapi(code: &str) -> Self {
    match code.to_ascii_lowercase().as_str() {
      "200" => Self::Ok200,
      "201" => Self::Created201,
      "202" => Self::Accepted202,
      "204" => Self::NoContent204,
      "301" => Self::MovedPermanently301,
      "302" => Self::Found302,
      "304" => Self::NotModified304,
      "400" => Self::BadRequest400,
      "401" => Self::Unauthorized401,
      "403" => Self::Forbidden403,
      "404" => Self::NotFound404,
      "405" => Self::MethodNotAllowed405,
      "406" => Self::NotAcceptable406,
      "408" => Self::RequestTimeout408,
      "409" => Self::Conflict409,
      "410" => Self::Gone410,
      "422" => Self::UnprocessableEntity422,
      "429" => Self::TooManyRequests429,
      "500" => Self::InternalServerError500,
      "501" => Self::NotImplemented501,
      "502" => Self::BadGateway502,
      "503" => Self::ServiceUnavailable503,
      "504" => Self::GatewayTimeout504,
      "1xx" => Self::Informational1XX,
      "2xx" => Self::Success2XX,
      "3xx" => Self::Redirection3XX,
      "4xx" => Self::ClientError4XX,
      "5xx" => Self::ServerError5XX,
      "default" => Self::Default,
      other => other.parse::<u16>().map_or(Self::Default, Self::Unknown),
    }
  }

  pub const fn code(self) -> Option<u16> {
    match self {
      Self::Ok200 => Some(200),
      Self::Created201 => Some(201),
      Self::Accepted202 => Some(202),
      Self::NoContent204 => Some(204),
      Self::MovedPermanently301 => Some(301),
      Self::Found302 => Some(302),
      Self::NotModified304 => Some(304),
      Self::BadRequest400 => Some(400),
      Self::Unauthorized401 => Some(401),
      Self::Forbidden403 => Some(403),
      Self::NotFound404 => Some(404),
      Self::MethodNotAllowed405 => Some(405),
      Self::NotAcceptable406 => Some(406),
      Self::RequestTimeout408 => Some(408),
      Self::Conflict409 => Some(409),
      Self::Gone410 => Some(410),
      Self::UnprocessableEntity422 => Some(422),
      Self::TooManyRequests429 => Some(429),
      Self::InternalServerError500 => Some(500),
      Self::NotImplemented501 => Some(501),
      Self::BadGateway502 => Some(502),
      Self::ServiceUnavailable503 => Some(503),
      Self::GatewayTimeout504 => Some(504),
      Self::Unknown(code) => Some(code),
      Self::Informational1XX
      | Self::Success2XX
      | Self::Redirection3XX
      | Self::ClientError4XX
      | Self::ServerError5XX
      | Self::Default => None,
    }
  }

  pub const fn is_default(self) -> bool {
    matches!(self, Self::Default)
  }

  pub const fn is_success(self) -> bool {
    matches!(
      self,
      Self::Ok200 | Self::Created201 | Self::Accepted202 | Self::NoContent204 | Self::Success2XX
    )
  }

  pub const fn variant_name(self) -> &'static str {
    match self {
      Self::Ok200 => "Ok",
      Self::Created201 => "Created",
      Self::Accepted202 => "Accepted",
      Self::NoContent204 => "NoContent",
      Self::MovedPermanently301 => "MovedPermanently",
      Self::Found302 => "Found",
      Self::NotModified304 => "NotModified",
      Self::BadRequest400 => "BadRequest",
      Self::Unauthorized401 => "Unauthorized",
      Self::Forbidden403 => "Forbidden",
      Self::NotFound404 => "NotFound",
      Self::MethodNotAllowed405 => "MethodNotAllowed",
      Self::NotAcceptable406 => "NotAcceptable",
      Self::RequestTimeout408 => "RequestTimeout",
      Self::Conflict409 => "Conflict",
      Self::Gone410 => "Gone",
      Self::UnprocessableEntity422 => "UnprocessableEntity",
      Self::TooManyRequests429 => "TooManyRequests",
      Self::InternalServerError500 => "InternalServerError",
      Self::NotImplemented501 => "NotImplemented",
      Self::BadGateway502 => "BadGateway",
      Self::ServiceUnavailable503 => "ServiceUnavailable",
      Self::GatewayTimeout504 => "GatewayTimeout",
      Self::Informational1XX => "Informational",
      Self::Success2XX => "Success",
      Self::Redirection3XX => "Redirection",
      Self::ClientError4XX => "ClientError",
      Self::ServerError5XX => "ServerError",
      Self::Default | Self::Unknown(_) => "Unknown",
    }
  }

  pub const fn as_str(self) -> &'static str {
    match self {
      Self::Ok200 => "200",
      Self::Created201 => "201",
      Self::Accepted202 => "202",
      Self::NoContent204 => "204",
      Self::MovedPermanently301 => "301",
      Self::Found302 => "302",
      Self::NotModified304 => "304",
      Self::BadRequest400 => "400",
      Self::Unauthorized401 => "401",
      Self::Forbidden403 => "403",
      Self::NotFound404 => "404",
      Self::MethodNotAllowed405 => "405",
      Self::NotAcceptable406 => "406",
      Self::RequestTimeout408 => "408",
      Self::Conflict409 => "409",
      Self::Gone410 => "410",
      Self::UnprocessableEntity422 => "422",
      Self::TooManyRequests429 => "429",
      Self::InternalServerError500 => "500",
      Self::NotImplemented501 => "501",
      Self::BadGateway502 => "502",
      Self::ServiceUnavailable503 => "503",
      Self::GatewayTimeout504 => "504",
      Self::Informational1XX => "1XX",
      Self::Success2XX => "2XX",
      Self::Redirection3XX => "3XX",
      Self::ClientError4XX => "4XX",
      Self::ServerError5XX => "5XX",
      Self::Default => "default",
      Self::Unknown(_) => "unknown",
    }
  }
}

impl Display for StatusCodeToken {
  fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
    match self {
      Self::Unknown(code) => write!(f, "{code}"),
      _ => f.write_str(self.as_str()),
    }
  }
}

pub fn status_code_to_variant_name(status_code: StatusCodeToken, response: &Response) -> EnumVariantToken {
  if let StatusCodeToken::Unknown(code) = status_code {
    if let Some(desc) = &response.description {
      let sanitized = desc
        .chars()
        .filter(|c| c.is_alphanumeric() || c.is_whitespace())
        .collect::<String>();
      let words: Vec<&str> = sanitized.split_whitespace().take(3).collect();
      if !words.is_empty() {
        return EnumVariantToken::new(to_rust_type_name(&words.join("_")));
      }
    }
    return EnumVariantToken::new(format!("Status{code}"));
  }

  EnumVariantToken::new(status_code.variant_name())
}

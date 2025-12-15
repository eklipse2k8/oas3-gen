use std::{
  convert::Infallible,
  fmt::{Display, Formatter},
  str::FromStr,
};

use oas3::spec::Response;

use crate::generator::{ast::EnumVariantToken, naming::identifiers::to_rust_type_name};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum StatusCodeToken {
  // 1xx
  Continue100,
  SwitchingProtocols101,
  Processing102,
  EarlyHints103,
  // 2xx
  Ok200,
  Created201,
  Accepted202,
  NonAuthoritativeInformation203,
  NoContent204,
  ResetContent205,
  PartialContent206,
  MultiStatus207,
  AlreadyReported208,
  ImUsed226,
  // 3xx
  MultipleChoices300,
  MovedPermanently301,
  Found302,
  SeeOther303,
  NotModified304,
  UseProxy305,
  TemporaryRedirect307,
  PermanentRedirect308,
  // 4xx
  BadRequest400,
  Unauthorized401,
  PaymentRequired402,
  Forbidden403,
  NotFound404,
  MethodNotAllowed405,
  NotAcceptable406,
  ProxyAuthenticationRequired407,
  RequestTimeout408,
  Conflict409,
  Gone410,
  LengthRequired411,
  PreconditionFailed412,
  ContentTooLarge413,
  UriTooLong414,
  UnsupportedMediaType415,
  RangeNotSatisfiable416,
  ExpectationFailed417,
  MisdirectedRequest421,
  UnprocessableEntity422,
  Locked423,
  FailedDependency424,
  TooEarly425,
  UpgradeRequired426,
  PreconditionRequired428,
  TooManyRequests429,
  RequestHeaderFieldsTooLarge431,
  UnavailableForLegalReasons451,
  // 5xx
  InternalServerError500,
  NotImplemented501,
  BadGateway502,
  ServiceUnavailable503,
  GatewayTimeout504,
  HttpVersionNotSupported505,
  VariantAlsoNegotiates506,
  InsufficientStorage507,
  LoopDetected508,
  NetworkAuthenticationRequired511,
  // Wildcards
  Informational1XX,
  Success2XX,
  Redirection3XX,
  ClientError4XX,
  ServerError5XX,
  // Special
  Default,
  Unknown(u16),
}

impl StatusCodeToken {
  pub const fn code(self) -> Option<u16> {
    match self {
      Self::Continue100 => Some(100),
      Self::SwitchingProtocols101 => Some(101),
      Self::Processing102 => Some(102),
      Self::EarlyHints103 => Some(103),
      Self::Ok200 => Some(200),
      Self::Created201 => Some(201),
      Self::Accepted202 => Some(202),
      Self::NonAuthoritativeInformation203 => Some(203),
      Self::NoContent204 => Some(204),
      Self::ResetContent205 => Some(205),
      Self::PartialContent206 => Some(206),
      Self::MultiStatus207 => Some(207),
      Self::AlreadyReported208 => Some(208),
      Self::ImUsed226 => Some(226),
      Self::MultipleChoices300 => Some(300),
      Self::MovedPermanently301 => Some(301),
      Self::Found302 => Some(302),
      Self::SeeOther303 => Some(303),
      Self::NotModified304 => Some(304),
      Self::UseProxy305 => Some(305),
      Self::TemporaryRedirect307 => Some(307),
      Self::PermanentRedirect308 => Some(308),
      Self::BadRequest400 => Some(400),
      Self::Unauthorized401 => Some(401),
      Self::PaymentRequired402 => Some(402),
      Self::Forbidden403 => Some(403),
      Self::NotFound404 => Some(404),
      Self::MethodNotAllowed405 => Some(405),
      Self::NotAcceptable406 => Some(406),
      Self::ProxyAuthenticationRequired407 => Some(407),
      Self::RequestTimeout408 => Some(408),
      Self::Conflict409 => Some(409),
      Self::Gone410 => Some(410),
      Self::LengthRequired411 => Some(411),
      Self::PreconditionFailed412 => Some(412),
      Self::ContentTooLarge413 => Some(413),
      Self::UriTooLong414 => Some(414),
      Self::UnsupportedMediaType415 => Some(415),
      Self::RangeNotSatisfiable416 => Some(416),
      Self::ExpectationFailed417 => Some(417),
      Self::MisdirectedRequest421 => Some(421),
      Self::UnprocessableEntity422 => Some(422),
      Self::Locked423 => Some(423),
      Self::FailedDependency424 => Some(424),
      Self::TooEarly425 => Some(425),
      Self::UpgradeRequired426 => Some(426),
      Self::PreconditionRequired428 => Some(428),
      Self::TooManyRequests429 => Some(429),
      Self::RequestHeaderFieldsTooLarge431 => Some(431),
      Self::UnavailableForLegalReasons451 => Some(451),
      Self::InternalServerError500 => Some(500),
      Self::NotImplemented501 => Some(501),
      Self::BadGateway502 => Some(502),
      Self::ServiceUnavailable503 => Some(503),
      Self::GatewayTimeout504 => Some(504),
      Self::HttpVersionNotSupported505 => Some(505),
      Self::VariantAlsoNegotiates506 => Some(506),
      Self::InsufficientStorage507 => Some(507),
      Self::LoopDetected508 => Some(508),
      Self::NetworkAuthenticationRequired511 => Some(511),
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
      Self::Ok200
        | Self::Created201
        | Self::Accepted202
        | Self::NonAuthoritativeInformation203
        | Self::NoContent204
        | Self::ResetContent205
        | Self::PartialContent206
        | Self::MultiStatus207
        | Self::AlreadyReported208
        | Self::ImUsed226
        | Self::Success2XX
    )
  }

  pub const fn variant_name(self) -> &'static str {
    match self {
      Self::Continue100 => "Continue",
      Self::SwitchingProtocols101 => "SwitchingProtocols",
      Self::Processing102 => "Processing",
      Self::EarlyHints103 => "EarlyHints",
      Self::Ok200 => "Ok",
      Self::Created201 => "Created",
      Self::Accepted202 => "Accepted",
      Self::NonAuthoritativeInformation203 => "NonAuthoritativeInformation",
      Self::NoContent204 => "NoContent",
      Self::ResetContent205 => "ResetContent",
      Self::PartialContent206 => "PartialContent",
      Self::MultiStatus207 => "MultiStatus",
      Self::AlreadyReported208 => "AlreadyReported",
      Self::ImUsed226 => "ImUsed",
      Self::MultipleChoices300 => "MultipleChoices",
      Self::MovedPermanently301 => "MovedPermanently",
      Self::Found302 => "Found",
      Self::SeeOther303 => "SeeOther",
      Self::NotModified304 => "NotModified",
      Self::UseProxy305 => "UseProxy",
      Self::TemporaryRedirect307 => "TemporaryRedirect",
      Self::PermanentRedirect308 => "PermanentRedirect",
      Self::BadRequest400 => "BadRequest",
      Self::Unauthorized401 => "Unauthorized",
      Self::PaymentRequired402 => "PaymentRequired",
      Self::Forbidden403 => "Forbidden",
      Self::NotFound404 => "NotFound",
      Self::MethodNotAllowed405 => "MethodNotAllowed",
      Self::NotAcceptable406 => "NotAcceptable",
      Self::ProxyAuthenticationRequired407 => "ProxyAuthenticationRequired",
      Self::RequestTimeout408 => "RequestTimeout",
      Self::Conflict409 => "Conflict",
      Self::Gone410 => "Gone",
      Self::LengthRequired411 => "LengthRequired",
      Self::PreconditionFailed412 => "PreconditionFailed",
      Self::ContentTooLarge413 => "ContentTooLarge",
      Self::UriTooLong414 => "UriTooLong",
      Self::UnsupportedMediaType415 => "UnsupportedMediaType",
      Self::RangeNotSatisfiable416 => "RangeNotSatisfiable",
      Self::ExpectationFailed417 => "ExpectationFailed",
      Self::MisdirectedRequest421 => "MisdirectedRequest",
      Self::UnprocessableEntity422 => "UnprocessableEntity",
      Self::Locked423 => "Locked",
      Self::FailedDependency424 => "FailedDependency",
      Self::TooEarly425 => "TooEarly",
      Self::UpgradeRequired426 => "UpgradeRequired",
      Self::PreconditionRequired428 => "PreconditionRequired",
      Self::TooManyRequests429 => "TooManyRequests",
      Self::RequestHeaderFieldsTooLarge431 => "RequestHeaderFieldsTooLarge",
      Self::UnavailableForLegalReasons451 => "UnavailableForLegalReasons",
      Self::InternalServerError500 => "InternalServerError",
      Self::NotImplemented501 => "NotImplemented",
      Self::BadGateway502 => "BadGateway",
      Self::ServiceUnavailable503 => "ServiceUnavailable",
      Self::GatewayTimeout504 => "GatewayTimeout",
      Self::HttpVersionNotSupported505 => "HttpVersionNotSupported",
      Self::VariantAlsoNegotiates506 => "VariantAlsoNegotiates",
      Self::InsufficientStorage507 => "InsufficientStorage",
      Self::LoopDetected508 => "LoopDetected",
      Self::NetworkAuthenticationRequired511 => "NetworkAuthenticationRequired",
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
      Self::Continue100 => "100",
      Self::SwitchingProtocols101 => "101",
      Self::Processing102 => "102",
      Self::EarlyHints103 => "103",
      Self::Ok200 => "200",
      Self::Created201 => "201",
      Self::Accepted202 => "202",
      Self::NonAuthoritativeInformation203 => "203",
      Self::NoContent204 => "204",
      Self::ResetContent205 => "205",
      Self::PartialContent206 => "206",
      Self::MultiStatus207 => "207",
      Self::AlreadyReported208 => "208",
      Self::ImUsed226 => "226",
      Self::MultipleChoices300 => "300",
      Self::MovedPermanently301 => "301",
      Self::Found302 => "302",
      Self::SeeOther303 => "303",
      Self::NotModified304 => "304",
      Self::UseProxy305 => "305",
      Self::TemporaryRedirect307 => "307",
      Self::PermanentRedirect308 => "308",
      Self::BadRequest400 => "400",
      Self::Unauthorized401 => "401",
      Self::PaymentRequired402 => "402",
      Self::Forbidden403 => "403",
      Self::NotFound404 => "404",
      Self::MethodNotAllowed405 => "405",
      Self::NotAcceptable406 => "406",
      Self::ProxyAuthenticationRequired407 => "407",
      Self::RequestTimeout408 => "408",
      Self::Conflict409 => "409",
      Self::Gone410 => "410",
      Self::LengthRequired411 => "411",
      Self::PreconditionFailed412 => "412",
      Self::ContentTooLarge413 => "413",
      Self::UriTooLong414 => "414",
      Self::UnsupportedMediaType415 => "415",
      Self::RangeNotSatisfiable416 => "416",
      Self::ExpectationFailed417 => "417",
      Self::MisdirectedRequest421 => "421",
      Self::UnprocessableEntity422 => "422",
      Self::Locked423 => "423",
      Self::FailedDependency424 => "424",
      Self::TooEarly425 => "425",
      Self::UpgradeRequired426 => "426",
      Self::PreconditionRequired428 => "428",
      Self::TooManyRequests429 => "429",
      Self::RequestHeaderFieldsTooLarge431 => "431",
      Self::UnavailableForLegalReasons451 => "451",
      Self::InternalServerError500 => "500",
      Self::NotImplemented501 => "501",
      Self::BadGateway502 => "502",
      Self::ServiceUnavailable503 => "503",
      Self::GatewayTimeout504 => "504",
      Self::HttpVersionNotSupported505 => "505",
      Self::VariantAlsoNegotiates506 => "506",
      Self::InsufficientStorage507 => "507",
      Self::LoopDetected508 => "508",
      Self::NetworkAuthenticationRequired511 => "511",
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

impl FromStr for StatusCodeToken {
  type Err = Infallible;

  fn from_str(s: &str) -> Result<Self, Self::Err> {
    Ok(match s.to_ascii_lowercase().as_str() {
      "100" => Self::Continue100,
      "101" => Self::SwitchingProtocols101,
      "102" => Self::Processing102,
      "103" => Self::EarlyHints103,
      "200" => Self::Ok200,
      "201" => Self::Created201,
      "202" => Self::Accepted202,
      "203" => Self::NonAuthoritativeInformation203,
      "204" => Self::NoContent204,
      "205" => Self::ResetContent205,
      "206" => Self::PartialContent206,
      "207" => Self::MultiStatus207,
      "208" => Self::AlreadyReported208,
      "226" => Self::ImUsed226,
      "300" => Self::MultipleChoices300,
      "301" => Self::MovedPermanently301,
      "302" => Self::Found302,
      "303" => Self::SeeOther303,
      "304" => Self::NotModified304,
      "305" => Self::UseProxy305,
      "307" => Self::TemporaryRedirect307,
      "308" => Self::PermanentRedirect308,
      "400" => Self::BadRequest400,
      "401" => Self::Unauthorized401,
      "402" => Self::PaymentRequired402,
      "403" => Self::Forbidden403,
      "404" => Self::NotFound404,
      "405" => Self::MethodNotAllowed405,
      "406" => Self::NotAcceptable406,
      "407" => Self::ProxyAuthenticationRequired407,
      "408" => Self::RequestTimeout408,
      "409" => Self::Conflict409,
      "410" => Self::Gone410,
      "411" => Self::LengthRequired411,
      "412" => Self::PreconditionFailed412,
      "413" => Self::ContentTooLarge413,
      "414" => Self::UriTooLong414,
      "415" => Self::UnsupportedMediaType415,
      "416" => Self::RangeNotSatisfiable416,
      "417" => Self::ExpectationFailed417,
      "421" => Self::MisdirectedRequest421,
      "422" => Self::UnprocessableEntity422,
      "423" => Self::Locked423,
      "424" => Self::FailedDependency424,
      "425" => Self::TooEarly425,
      "426" => Self::UpgradeRequired426,
      "428" => Self::PreconditionRequired428,
      "429" => Self::TooManyRequests429,
      "431" => Self::RequestHeaderFieldsTooLarge431,
      "451" => Self::UnavailableForLegalReasons451,
      "500" => Self::InternalServerError500,
      "501" => Self::NotImplemented501,
      "502" => Self::BadGateway502,
      "503" => Self::ServiceUnavailable503,
      "504" => Self::GatewayTimeout504,
      "505" => Self::HttpVersionNotSupported505,
      "506" => Self::VariantAlsoNegotiates506,
      "507" => Self::InsufficientStorage507,
      "508" => Self::LoopDetected508,
      "511" => Self::NetworkAuthenticationRequired511,
      "1xx" => Self::Informational1XX,
      "2xx" => Self::Success2XX,
      "3xx" => Self::Redirection3XX,
      "4xx" => Self::ClientError4XX,
      "5xx" => Self::ServerError5XX,
      "default" => Self::Default,
      other => other.parse::<u16>().map_or(Self::Default, Self::Unknown),
    })
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

use proc_macro2::TokenStream;
use quote::{ToTokens, quote};

use crate::generator::ast::StatusCodeToken;

/// Wrapper to convert StatusCodeToken to http::StatusCode tokens
#[derive(Clone, Debug)]
pub(crate) struct HttpStatusCode(StatusCodeToken);

impl HttpStatusCode {
  pub(crate) fn new(token: StatusCodeToken) -> Self {
    Self(token)
  }
}

impl ToTokens for HttpStatusCode {
  fn to_tokens(&self, tokens: &mut TokenStream) {
    let ts = match self.0 {
      StatusCodeToken::Continue100 | StatusCodeToken::Informational1XX => quote! { http::StatusCode::CONTINUE },
      StatusCodeToken::SwitchingProtocols101 => quote! { http::StatusCode::SWITCHING_PROTOCOLS },
      StatusCodeToken::Processing102 => quote! { http::StatusCode::PROCESSING },
      StatusCodeToken::EarlyHints103 => quote! { http::StatusCode::EARLY_HINTS },
      StatusCodeToken::Ok200 | StatusCodeToken::Success2XX | StatusCodeToken::Default => {
        quote! { http::StatusCode::OK }
      }
      StatusCodeToken::Created201 => quote! { http::StatusCode::CREATED },
      StatusCodeToken::Accepted202 => quote! { http::StatusCode::ACCEPTED },
      StatusCodeToken::NonAuthoritativeInformation203 => {
        quote! { http::StatusCode::NON_AUTHORITATIVE_INFORMATION }
      }
      StatusCodeToken::NoContent204 => quote! { http::StatusCode::NO_CONTENT },
      StatusCodeToken::ResetContent205 => quote! { http::StatusCode::RESET_CONTENT },
      StatusCodeToken::PartialContent206 => quote! { http::StatusCode::PARTIAL_CONTENT },
      StatusCodeToken::MultiStatus207 => quote! { http::StatusCode::MULTI_STATUS },
      StatusCodeToken::AlreadyReported208 => quote! { http::StatusCode::ALREADY_REPORTED },
      StatusCodeToken::ImUsed226 => quote! { http::StatusCode::IM_USED },
      StatusCodeToken::MultipleChoices300 => quote! { http::StatusCode::MULTIPLE_CHOICES },
      StatusCodeToken::MovedPermanently301 => quote! { http::StatusCode::MOVED_PERMANENTLY },
      StatusCodeToken::Found302 => quote! { http::StatusCode::FOUND },
      StatusCodeToken::SeeOther303 => quote! { http::StatusCode::SEE_OTHER },
      StatusCodeToken::NotModified304 => quote! { http::StatusCode::NOT_MODIFIED },
      StatusCodeToken::UseProxy305 => quote! { http::StatusCode::USE_PROXY },
      StatusCodeToken::TemporaryRedirect307 => quote! { http::StatusCode::TEMPORARY_REDIRECT },
      StatusCodeToken::PermanentRedirect308 => quote! { http::StatusCode::PERMANENT_REDIRECT },
      StatusCodeToken::BadRequest400 | StatusCodeToken::ClientError4XX => quote! { http::StatusCode::BAD_REQUEST },
      StatusCodeToken::Unauthorized401 => quote! { http::StatusCode::UNAUTHORIZED },
      StatusCodeToken::PaymentRequired402 => quote! { http::StatusCode::PAYMENT_REQUIRED },
      StatusCodeToken::Forbidden403 => quote! { http::StatusCode::FORBIDDEN },
      StatusCodeToken::NotFound404 => quote! { http::StatusCode::NOT_FOUND },
      StatusCodeToken::MethodNotAllowed405 => quote! { http::StatusCode::METHOD_NOT_ALLOWED },
      StatusCodeToken::NotAcceptable406 => quote! { http::StatusCode::NOT_ACCEPTABLE },
      StatusCodeToken::ProxyAuthenticationRequired407 => {
        quote! { http::StatusCode::PROXY_AUTHENTICATION_REQUIRED }
      }
      StatusCodeToken::RequestTimeout408 => quote! { http::StatusCode::REQUEST_TIMEOUT },
      StatusCodeToken::Conflict409 => quote! { http::StatusCode::CONFLICT },
      StatusCodeToken::Gone410 => quote! { http::StatusCode::GONE },
      StatusCodeToken::LengthRequired411 => quote! { http::StatusCode::LENGTH_REQUIRED },
      StatusCodeToken::PreconditionFailed412 => quote! { http::StatusCode::PRECONDITION_FAILED },
      StatusCodeToken::ContentTooLarge413 => quote! { http::StatusCode::PAYLOAD_TOO_LARGE },
      StatusCodeToken::UriTooLong414 => quote! { http::StatusCode::URI_TOO_LONG },
      StatusCodeToken::UnsupportedMediaType415 => {
        quote! { http::StatusCode::UNSUPPORTED_MEDIA_TYPE }
      }
      StatusCodeToken::RangeNotSatisfiable416 => {
        quote! { http::StatusCode::RANGE_NOT_SATISFIABLE }
      }
      StatusCodeToken::ExpectationFailed417 => {
        quote! { http::StatusCode::EXPECTATION_FAILED }
      }
      StatusCodeToken::MisdirectedRequest421 => quote! { http::StatusCode::MISDIRECTED_REQUEST },
      StatusCodeToken::UnprocessableEntity422 => {
        quote! { http::StatusCode::UNPROCESSABLE_ENTITY }
      }
      StatusCodeToken::Locked423 => quote! { http::StatusCode::LOCKED },
      StatusCodeToken::FailedDependency424 => quote! { http::StatusCode::FAILED_DEPENDENCY },
      StatusCodeToken::TooEarly425 => quote! { http::StatusCode::TOO_EARLY },
      StatusCodeToken::UpgradeRequired426 => quote! { http::StatusCode::UPGRADE_REQUIRED },
      StatusCodeToken::PreconditionRequired428 => {
        quote! { http::StatusCode::PRECONDITION_REQUIRED }
      }
      StatusCodeToken::TooManyRequests429 => quote! { http::StatusCode::TOO_MANY_REQUESTS },
      StatusCodeToken::RequestHeaderFieldsTooLarge431 => {
        quote! { http::StatusCode::REQUEST_HEADER_FIELDS_TOO_LARGE }
      }
      StatusCodeToken::UnavailableForLegalReasons451 => {
        quote! { http::StatusCode::UNAVAILABLE_FOR_LEGAL_REASONS }
      }
      StatusCodeToken::InternalServerError500 => {
        quote! { http::StatusCode::INTERNAL_SERVER_ERROR }
      }
      StatusCodeToken::NotImplemented501 => quote! { http::StatusCode::NOT_IMPLEMENTED },
      StatusCodeToken::BadGateway502 => quote! { http::StatusCode::BAD_GATEWAY },
      StatusCodeToken::ServiceUnavailable503 => {
        quote! { http::StatusCode::SERVICE_UNAVAILABLE }
      }
      StatusCodeToken::GatewayTimeout504 => quote! { http::StatusCode::GATEWAY_TIMEOUT },
      StatusCodeToken::HttpVersionNotSupported505 => {
        quote! { http::StatusCode::HTTP_VERSION_NOT_SUPPORTED }
      }
      StatusCodeToken::VariantAlsoNegotiates506 => {
        quote! { http::StatusCode::VARIANT_ALSO_NEGOTIATES }
      }
      StatusCodeToken::InsufficientStorage507 => {
        quote! { http::StatusCode::INSUFFICIENT_STORAGE }
      }
      StatusCodeToken::LoopDetected508 => quote! { http::StatusCode::LOOP_DETECTED },
      StatusCodeToken::NetworkAuthenticationRequired511 => {
        quote! { http::StatusCode::NETWORK_AUTHENTICATION_REQUIRED }
      }
      StatusCodeToken::ServerError5XX => quote! { http::StatusCode::INTERNAL_SERVER_ERROR },
      other => {
        if let Some(code) = other.code() {
          quote! { http::StatusCode::from_u16(#code).unwrap_or(http::StatusCode::INTERNAL_SERVER_ERROR) }
        } else {
          quote! { http::StatusCode::INTERNAL_SERVER_ERROR }
        }
      }
    };
    tokens.extend(ts);
  }
}

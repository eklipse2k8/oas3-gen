#[cfg(feature = "eventsource")]
mod event_stream;
pub use better_default::Default;
pub use bon::bon;
#[cfg(feature = "eventsource")]
pub use event_stream::{EventStream, EventStreamError};
pub use http::Method;
use http::{StatusCode, header::RETRY_AFTER};
use serde::de::DeserializeOwned;
use serde_with::{
  StringWithSeparator,
  formats::{CommaSeparator, Separator, SpaceSeparator},
};

/// Pipe separator for `OpenAPI` pipeDelimited style
pub struct PipeSeparator;

impl Separator for PipeSeparator {
  #[inline]
  fn separator() -> &'static str {
    "|"
  }
}

/// De/Serialize a delimited collection using [`Display`] and [`FromStr`] implementation
///
/// An empty string deserializes as an empty collection.
pub type StringWithCommaSeparator = StringWithSeparator<CommaSeparator, String>;

/// De/Serialize a delimited collection using [`Display`] and [`FromStr`] implementation
///
/// An empty string deserializes as an empty collection.
pub type StringWithSpaceSeparator = StringWithSeparator<SpaceSeparator, String>;

/// De/Serialize a delimited collection using [`Display`] and [`FromStr`] implementation
///
/// An empty string deserializes as an empty collection.
pub type StringWithPipeSeparator = StringWithSeparator<PipeSeparator, String>;

#[derive(Debug, thiserror::Error)]
pub enum DiagnosticsError {
  #[cfg(feature = "reqwest")]
  #[error(transparent)]
  BodyReadError(#[from] reqwest::Error),

  #[error("JSON deserialization error at path '{path}': {inner}")]
  DeserializationError { path: String, inner: serde_json::Error },

  #[cfg(feature = "quick-xml")]
  #[error(transparent)]
  XmlDeserializationError(#[from] quick_xml::DeError),
}

#[allow(async_fn_in_trait)]
pub trait Diagnostics<T>
where
  T: serde::de::DeserializeOwned,
{
  async fn json_with_diagnostics(self) -> Result<T, DiagnosticsError>;

  #[cfg(feature = "quick-xml")]
  async fn xml_with_diagnostics(self) -> Result<T, DiagnosticsError>;
}

#[cfg(feature = "reqwest")]
const UTF8_BOM: &[u8] = b"\xEF\xBB\xBF";

#[cfg(feature = "reqwest")]
pub(crate) fn deserialize_json<T, E>(json: &[u8], on_error: impl FnOnce(String, serde_json::Error) -> E) -> Result<T, E>
where
  T: DeserializeOwned,
{
  T::deserialize(&mut serde_json::Deserializer::from_slice(json)).map_err(
    |error| match serde_path_to_error::deserialize::<_, T>(&mut serde_json::Deserializer::from_slice(json)) {
      Err(tracked) => on_error(tracked.path().to_string(), tracked.into_inner()),
      Ok(_) => on_error(".".to_string(), error),
    },
  )
}

#[cfg(feature = "reqwest")]
impl<T> Diagnostics<T> for reqwest::Response
where
  T: serde::de::DeserializeOwned,
{
  async fn json_with_diagnostics(self) -> Result<T, DiagnosticsError> {
    let body = self.bytes().await?;
    let body = body.strip_prefix(UTF8_BOM).unwrap_or(&body);
    deserialize_json(String::from_utf8_lossy(body).as_bytes(), |path, inner| {
      DiagnosticsError::DeserializationError { path, inner }
    })
  }

  #[cfg(feature = "quick-xml")]
  async fn xml_with_diagnostics(self) -> Result<T, DiagnosticsError> {
    let raw_body = self.bytes().await?;
    Ok(quick_xml::de::from_reader(std::io::Cursor::new(raw_body))?)
  }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Default)]
pub enum RateLimit {
  /// The client has sent too many requests in a given amount of time.
  #[default]
  Exceeded,
  /// The client should try the request again after the specified number of seconds.
  TryAgainAfter(u32),
}

impl RateLimit {
  /// If the `Retry-After` header is present and valid, it will be used to create a `TryAgainAfter` variant.
  /// Otherwise, it will return `Exceeded`.
  #[must_use]
  pub fn with_headers(headers: &http::HeaderMap) -> Self {
    if let Some(retry_after) = headers.get(RETRY_AFTER)
      && let Ok(seconds) = String::from_utf8_lossy(retry_after.as_bytes()).parse::<u32>()
    {
      return Self::TryAgainAfter(seconds);
    }
    Self::Exceeded
  }
}

#[derive(Debug, Clone)]
pub struct TooManyRequests<T: DeserializeOwned>(RateLimit, T);

impl<T: DeserializeOwned> TooManyRequests<T> {
  #[must_use]
  pub fn is_too_many_requests(status: StatusCode) -> bool {
    status == StatusCode::TOO_MANY_REQUESTS
  }

  /// Create a new `TooManyRequests` instance by extracting rate limit information from headers
  #[must_use]
  pub fn new(headers: &http::HeaderMap, inner: T) -> Self {
    Self(RateLimit::with_headers(headers), inner)
  }

  /// If the `Retry-After` header is present and valid, it will be used to create a `TryAgainAfter` variant.
  /// Otherwise, it will return `Exceeded`.
  pub fn rate_limit(&self) -> &RateLimit {
    &self.0
  }

  /// Consume the `TooManyRequests` and return the inner value
  #[must_use]
  pub fn into_inner(self) -> T {
    self.1
  }
}

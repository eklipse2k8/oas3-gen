mod event_stream;
pub use better_default::Default;
pub use bon::bon;
pub use event_stream::{EventStream, EventStreamError};
pub use http::Method;
use http::{StatusCode, header::RETRY_AFTER};
use reqwest::Response;
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
pub enum JsonDiagnostics {
  #[error("Failed to read response body: {0}")]
  BodyReadError(#[from] reqwest::Error),
  #[error("JSON deserialization error at path '{path}': {inner}")]
  DeserializationError { path: String, inner: serde_json::Error },
}

#[derive(Debug, thiserror::Error)]
pub enum XmlDiagnostics {
  #[error("Failed to read response body: {0}")]
  BodyReadError(#[from] reqwest::Error),
  #[error("XML deserialization error: {0}")]
  DeserializationError(#[from] quick_xml::DeError),
}

#[allow(async_fn_in_trait)]
pub trait Diagnostics<T>
where
  T: serde::de::DeserializeOwned,
{
  async fn json_with_diagnostics(self) -> Result<T, JsonDiagnostics>;
  async fn xml_with_diagnostics(self) -> Result<T, XmlDiagnostics>;
}

impl<T> Diagnostics<T> for reqwest::Response
where
  T: serde::de::DeserializeOwned,
{
  async fn json_with_diagnostics(self) -> Result<T, JsonDiagnostics> {
    let raw_body = self.text().await.map_err(JsonDiagnostics::BodyReadError)?;
    let mut de = serde_json::Deserializer::from_str(&raw_body);
    serde_path_to_error::deserialize(&mut de).map_err(|err| JsonDiagnostics::DeserializationError {
      path: err.path().to_string(),
      inner: err.into_inner(),
    })
  }

  async fn xml_with_diagnostics(self) -> Result<T, XmlDiagnostics> {
    let raw_body = self.bytes().await.map_err(XmlDiagnostics::BodyReadError)?;
    quick_xml::de::from_reader(std::io::Cursor::new(raw_body)).map_err(XmlDiagnostics::DeserializationError)
  }
}

#[derive(Debug)]
pub enum ClientErrorCode {
  Ok(Response),
  /// The client has sent too many requests in a given amount of time.
  RateLimitExceeded,
  /// The client should try the request again after the specified number of seconds.
  TryAgainAfter(u32),
  UnexpectedStatus(StatusCode),
}

pub trait ClientStatus: Sized {
  fn decode_status(self) -> ClientErrorCode;
}

impl ClientStatus for Response {
  fn decode_status(self) -> ClientErrorCode {
    let status = self.status();
    if status.is_success() {
      return ClientErrorCode::Ok(self);
    }

    match status {
      StatusCode::TOO_MANY_REQUESTS => {
        let retry_after = self
          .headers()
          .get(RETRY_AFTER)
          .and_then(|h| String::from_utf8_lossy(h.as_bytes()).parse::<u32>().ok());

        if let Some(seconds) = retry_after {
          ClientErrorCode::TryAgainAfter(seconds)
        } else {
          ClientErrorCode::RateLimitExceeded
        }
      }
      _ => ClientErrorCode::UnexpectedStatus(status),
    }
  }
}

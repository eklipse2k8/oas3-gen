/// Re-exported from `better_default` to enable `#[default(value)]` attribute on struct fields.
pub use better_default::Default;
/// Re-exported from `http` for HTTP method types in generated request structs.
pub use http::Method;
use serde_with::{
  StringWithSeparator,
  formats::{CommaSeparator, Separator, SpaceSeparator},
};
/// Re-exported from `serde_with` to enable `#[skip_serializing_none]` on generated types.
pub use serde_with::{serde_as, skip_serializing_none};

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
  #[error("Deserialization error: {inner}, path={path}")]
  DeserializationError { path: String, inner: serde_json::Error },
}

#[allow(async_fn_in_trait)]
pub trait Diagnostics<T>
where
  T: serde::de::DeserializeOwned,
{
  async fn json_with_diagnostics(self) -> Result<T, JsonDiagnostics>;
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
}

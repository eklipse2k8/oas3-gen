/// Re-exported from `better_default` to enable `#[default(value)]` attribute on struct fields.
pub use better_default::Default;
/// Re-exported from `http` for HTTP method types in generated request structs.
pub use http::Method;
/// Re-exported from `percent_encoding` to support custom URL encoding sets.
pub use percent_encoding::{AsciiSet, NON_ALPHANUMERIC, utf8_percent_encode};
/// Re-exported from `serde_with` to enable `#[skip_serializing_none]` on generated types.
pub use serde_with::skip_serializing_none;

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

/// Character set for percent-encoding URL path segments according to RFC 3986.
///
/// This set preserves unreserved characters (`-`, `_`, `.`, `~`) and encodes all other
/// non-alphanumeric characters. Used by [`percent_encode_path_segment`] to safely encode
/// dynamic path parameters in HTTP request URLs.
///
/// # RFC 3986 Compliance
///
/// Unreserved characters that are NOT encoded: `A-Z a-z 0-9 - _ . ~`
///
/// All other characters are percent-encoded, including reserved characters like `/` which
/// have special meaning in URL paths and must be encoded when appearing in path parameter
/// values.
pub const PATH_ENCODE_SET: &AsciiSet = &NON_ALPHANUMERIC.remove(b'-').remove(b'_').remove(b'.').remove(b'~');

/// Percent-encodes a URL path segment according to RFC 3986 rules.
///
/// This function encodes a string value for safe inclusion as a path parameter in an HTTP
/// request URL. It preserves unreserved characters and encodes everything else, including
/// path delimiters like `/` that would otherwise be interpreted as path separators.
///
/// Generated code calls this function when constructing request URLs with dynamic path
/// parameters, such as `/users/{user_id}/posts/{post_id}`.
///
/// # RFC 3986 Compliance
///
/// Follows RFC 3986 path encoding rules:
/// - Unreserved characters (`A-Z a-z 0-9 - _ . ~`) are preserved
/// - All other characters, including spaces and `/`, are percent-encoded
/// - For example: `hello/world` → `hello%2Fworld`
///
/// # Examples
///
/// ```
/// use oas3_gen_support::percent_encode_path_segment;
///
/// assert_eq!(percent_encode_path_segment("user123"), "user123");
/// assert_eq!(percent_encode_path_segment("hello world"), "hello%20world");
/// assert_eq!(percent_encode_path_segment("path/with/slashes"), "path%2Fwith%2Fslashes");
/// assert_eq!(percent_encode_path_segment("user@example.com"), "user%40example.com");
/// ```
///
/// # Performance
///
/// This function is marked `#[inline]` for performance. It only allocates when encoding
/// is required; if the input contains only unreserved characters, it returns a clone.
#[inline]
#[must_use]
pub fn percent_encode_path_segment(segment: &str) -> String {
  utf8_percent_encode(segment, PATH_ENCODE_SET).to_string()
}

/// Character set for percent-encoding URL query string components according to RFC 3986.
///
/// This set preserves unreserved characters (`-`, `_`, `.`, `~`) and encodes all other
/// non-alphanumeric characters. Used by [`percent_encode_query_component`] to safely encode
/// query parameter values in HTTP request URLs.
///
/// # RFC 3986 Compliance
///
/// Unreserved characters that are NOT encoded: `A-Z a-z 0-9 - _ . ~`
///
/// All other characters are percent-encoded, including query-significant characters like
/// `&`, `=`, and `+` which have special meaning in query strings and must be encoded when
/// appearing in parameter values.
pub const QUERY_ENCODE_SET: &AsciiSet = &NON_ALPHANUMERIC.remove(b'-').remove(b'_').remove(b'.').remove(b'~');

/// Percent-encodes a URL query parameter value according to RFC 3986 rules.
///
/// This function encodes a string value for safe inclusion as a query parameter value in
/// an HTTP request URL. It preserves unreserved characters and encodes everything else,
/// including query delimiters like `&` and `=` that would otherwise interfere with query
/// string parsing.
///
/// Generated code calls this function when constructing request URLs with query parameters,
/// typically after serializing the parameter value with [`serialize_query_param`]. The
/// combination handles both type conversion (Rust value → string) and URL safety (string
/// → percent-encoded string).
///
/// # RFC 3986 Compliance
///
/// Follows RFC 3986 query encoding rules:
/// - Unreserved characters (`A-Z a-z 0-9 - _ . ~`) are preserved
/// - All other characters, including spaces, `&`, `=`, and `+`, are percent-encoded
/// - For example: `hello world` → `hello%20world`
/// - For example: `a+b=c&d` → `a%2Bb%3Dc%26d`
///
/// # Examples
///
/// ```
/// use oas3_gen_support::percent_encode_query_component;
///
/// assert_eq!(percent_encode_query_component("simple"), "simple");
/// assert_eq!(percent_encode_query_component("hello world"), "hello%20world");
/// assert_eq!(percent_encode_query_component("a+b"), "a%2Bb");
/// assert_eq!(percent_encode_query_component("user@example.com"), "user%40example.com");
/// assert_eq!(percent_encode_query_component("a=b&c=d"), "a%3Db%26c%3Dd");
/// ```
///
/// # Performance
///
/// This function is marked `#[inline]` for performance. It only allocates when encoding
/// is required; if the input contains only unreserved characters, it returns a clone.
#[inline]
#[must_use]
pub fn percent_encode_query_component(component: &str) -> String {
  utf8_percent_encode(component, QUERY_ENCODE_SET).to_string()
}

/// Serializes a `serde_json::Value` to a string for use in URL query parameters.
///
/// Handles `serde_json::Value` specifically:
/// - Strings are returned as-is (unquoted)
/// - Objects and Arrays are serialized to JSON strings
/// - Numbers, Booleans, and Null are serialized to their JSON representation
///
/// # Errors
///
/// Returns an error when the value cannot be converted to a JSON string.
#[inline]
pub fn serialize_any_query_param(value: &serde_json::Value) -> Result<String, serde_json::Error> {
  if let serde_json::Value::String(s) = value {
    Ok(s.clone())
  } else {
    serde_json::to_string(value)
  }
}

/// Serializes a Rust value to a plain string representation for use in URL query parameters.
///
/// This function converts Rust values (primitives, enums, Option types) into their string
/// representations suitable for URL query parameters. It uses `serde_plain` for serialization,
/// which handles common types including:
/// - Primitives: `42` → `"42"`, `true` → `"true"`, `3.14` → `"3.14"`
/// - Strings: `"hello"` → `"hello"`
/// - Enums: Serializes using serde rename attributes (e.g., `#[serde(rename = "value_1")]`)
/// - Options: `Some(42)` → `"42"`, `None` → `""`
///
/// Generated code calls this function before [`percent_encode_query_component`] to build
/// complete query strings. The two-step process (serialize → encode) ensures both type safety
/// and URL safety.
///
/// # Examples
///
/// ```
/// use oas3_gen_support::serialize_query_param;
///
/// assert_eq!(serialize_query_param(&42).unwrap(), "42");
/// assert_eq!(serialize_query_param(&true).unwrap(), "true");
/// assert_eq!(serialize_query_param(&"hello").unwrap(), "hello");
/// assert_eq!(serialize_query_param(&Some(42)).unwrap(), "42");
/// assert_eq!(serialize_query_param(&None::<i32>).unwrap(), "");
/// ```
///
/// # Enum Serialization
///
/// For enums with `#[serde(rename = "...")]` attributes, the renamed value is used:
///
/// ```ignore
/// #[derive(serde::Serialize)]
/// enum Status {
///   #[serde(rename = "active")]
///   Active,
///   #[serde(rename = "inactive")]
///   Inactive,
/// }
///
/// assert_eq!(serialize_query_param(&Status::Active).unwrap(), "active");
/// ```
///
/// # Errors
///
/// Returns `Err` if the value cannot be serialized to a plain string representation.
/// This can occur for:
/// - Complex nested structures (maps, nested objects)
/// - Types without appropriate `Serialize` implementations
/// - Custom serialization logic that produces non-plain-text output
///
/// In practice, errors are rare for the primitive and enum types typically used in query
/// parameters. Generated code propagates errors using `?` to the caller.
#[inline]
pub fn serialize_query_param<T: serde::Serialize>(value: &T) -> Result<String, serde_plain::Error> {
  serde_plain::to_string(value)
}

#[cfg(test)]
mod tests {
  #[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
  enum QueryParamTestEnum {
    #[serde(rename = "value_1")]
    Value1,
    #[serde(rename = "value_2")]
    Value2,
    #[serde(rename = "0")]
    Zero,
    #[serde(rename = "1")]
    One,
  }

  #[test]
  fn test_serialize_query_param_string_enum() {
    let value = QueryParamTestEnum::Value1;
    let result = super::serialize_query_param(&value).unwrap();
    assert_eq!(result, "value_1");

    let value = QueryParamTestEnum::Zero;
    let result = super::serialize_query_param(&value).unwrap();
    assert_eq!(result, "0");
  }

  #[test]
  #[allow(clippy::approx_constant)]
  fn test_serialize_query_param_primitive_types() {
    assert_eq!(super::serialize_query_param(&42).unwrap(), "42");
    assert_eq!(super::serialize_query_param(&true).unwrap(), "true");
    assert_eq!(super::serialize_query_param(&false).unwrap(), "false");
    assert_eq!(super::serialize_query_param(&"hello").unwrap(), "hello");
    assert_eq!(super::serialize_query_param(&3.14).unwrap(), "3.14");
  }

  #[test]
  fn test_serialize_query_param_option_types() {
    let some_value: Option<i32> = Some(42);
    let none_value: Option<i32> = None;
    assert_eq!(super::serialize_query_param(&some_value).unwrap(), "42");
    assert_eq!(super::serialize_query_param(&none_value).unwrap(), "");
  }

  #[test]
  fn test_percent_encode_query_component() {
    assert_eq!(super::percent_encode_query_component("hello world"), "hello%20world");
    assert_eq!(super::percent_encode_query_component("a+b"), "a%2Bb");
    assert_eq!(
      super::percent_encode_query_component("test@example.com"),
      "test%40example.com"
    );
    assert_eq!(super::percent_encode_query_component("simple"), "simple");
    assert_eq!(
      super::percent_encode_query_component("with-dash_underscore"),
      "with-dash_underscore"
    );
  }

  #[test]
  fn test_serialize_and_encode_query_param() {
    let value = QueryParamTestEnum::Value1;
    let serialized = super::serialize_query_param(&value).unwrap();
    let encoded = super::percent_encode_query_component(&serialized);
    assert_eq!(encoded, "value_1");

    let value = "hello world";
    let serialized = super::serialize_query_param(&value).unwrap();
    let encoded = super::percent_encode_query_component(&serialized);
    assert_eq!(encoded, "hello%20world");
  }
}

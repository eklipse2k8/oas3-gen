use inflections::Inflect as _;
use proc_macro2::TokenStream;
use quote::{ToTokens, format_ident, quote};

use crate::generator::{
  ast::{
    RegexKey,
    tokens::{ConstToken, HeaderNameToken},
  },
  naming::identifiers::sanitize,
};

impl From<&RegexKey> for ConstToken {
  fn from(key: &RegexKey) -> Self {
    let joined = key
      .parts()
      .iter()
      .map(|part| sanitize(part))
      .collect::<Vec<_>>()
      .join("_");

    let mut ident = joined.to_constant_case();

    if ident.starts_with(|c: char| c.is_ascii_digit()) {
      ident.insert(0, '_');
    }

    ConstToken::new(format!("REGEX_{ident}"))
  }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, bon::Builder)]
pub struct HttpHeaderRef {
  pub const_token: ConstToken,
  pub header_name: HeaderNameToken,
}

/// Declares [`http_header_constant`] over the named `http::header` constants, so the
/// compiler checks that each one exists.
macro_rules! http_header_constants {
  ($($constant:ident),* $(,)?) => {
    /// The `http::header` constant that names `header`, if `http` declares one.
    fn http_header_constant(header: &http::HeaderName) -> Option<&'static str> {
      $(
        if *header == http::header::$constant {
          return Some(stringify!($constant));
        }
      )*
      None
    }
  };
}

http_header_constants!(
  ACCEPT,
  ACCEPT_CHARSET,
  ACCEPT_ENCODING,
  ACCEPT_LANGUAGE,
  ACCEPT_RANGES,
  ACCESS_CONTROL_ALLOW_CREDENTIALS,
  ACCESS_CONTROL_ALLOW_HEADERS,
  ACCESS_CONTROL_ALLOW_METHODS,
  ACCESS_CONTROL_ALLOW_ORIGIN,
  ACCESS_CONTROL_EXPOSE_HEADERS,
  ACCESS_CONTROL_MAX_AGE,
  ACCESS_CONTROL_REQUEST_HEADERS,
  ACCESS_CONTROL_REQUEST_METHOD,
  AGE,
  ALLOW,
  ALT_SVC,
  AUTHORIZATION,
  CACHE_CONTROL,
  CACHE_STATUS,
  CDN_CACHE_CONTROL,
  CONNECTION,
  CONTENT_DISPOSITION,
  CONTENT_ENCODING,
  CONTENT_LANGUAGE,
  CONTENT_LENGTH,
  CONTENT_LOCATION,
  CONTENT_RANGE,
  CONTENT_SECURITY_POLICY,
  CONTENT_SECURITY_POLICY_REPORT_ONLY,
  CONTENT_TYPE,
  COOKIE,
  DNT,
  DATE,
  ETAG,
  EXPECT,
  EXPIRES,
  FORWARDED,
  FROM,
  HOST,
  IF_MATCH,
  IF_MODIFIED_SINCE,
  IF_NONE_MATCH,
  IF_RANGE,
  IF_UNMODIFIED_SINCE,
  LAST_MODIFIED,
  LINK,
  LOCATION,
  MAX_FORWARDS,
  ORIGIN,
  PRAGMA,
  PROXY_AUTHENTICATE,
  PROXY_AUTHORIZATION,
  PUBLIC_KEY_PINS,
  PUBLIC_KEY_PINS_REPORT_ONLY,
  RANGE,
  REFERER,
  REFERRER_POLICY,
  REFRESH,
  RETRY_AFTER,
  SEC_WEBSOCKET_ACCEPT,
  SEC_WEBSOCKET_EXTENSIONS,
  SEC_WEBSOCKET_KEY,
  SEC_WEBSOCKET_PROTOCOL,
  SEC_WEBSOCKET_VERSION,
  SERVER,
  SET_COOKIE,
  STRICT_TRANSPORT_SECURITY,
  TE,
  TRAILER,
  TRANSFER_ENCODING,
  USER_AGENT,
  UPGRADE,
  UPGRADE_INSECURE_REQUESTS,
  VARY,
  VIA,
  WARNING,
  WWW_AUTHENTICATE,
  X_CONTENT_TYPE_OPTIONS,
  X_DNS_PREFETCH_CONTROL,
  X_FRAME_OPTIONS,
  X_XSS_PROTECTION,
);

impl HttpHeaderRef {
  /// Whether `http::header` already declares this header's constant, so `types.rs`
  /// doesn't declare one.
  #[must_use]
  pub fn is_declared_by_http(&self) -> bool {
    self.http_constant().is_some()
  }

  /// The constant generated code names this header by: `http`'s own, such as
  /// `http::header::CONTENT_TYPE`, or the one `types.rs` declares, such as `X_API_KEY`.
  #[must_use]
  pub fn path(&self) -> TokenStream {
    let Some(constant) = self.http_constant() else {
      let const_token = &self.const_token;
      return quote! { #const_token };
    };
    let constant = format_ident!("{constant}");
    quote! { http::header::#constant }
  }

  fn http_constant(&self) -> Option<&'static str> {
    let header = http::HeaderName::from_bytes(self.header_name.to_atom().as_bytes()).ok()?;
    http_header_constant(&header)
  }
}

impl<T: ToString> From<T> for HttpHeaderRef {
  fn from(s: T) -> Self {
    let header_name_str = s.to_string();
    Self {
      const_token: ConstToken::from_raw(&header_name_str),
      header_name: HeaderNameToken::from_raw(&header_name_str),
    }
  }
}

impl ToTokens for HttpHeaderRef {
  fn to_tokens(&self, tokens: &mut TokenStream) {
    let const_token = &self.const_token;
    let header_name = &self.header_name;

    let header = quote! {
      pub const #const_token: http::HeaderName = http::HeaderName::from_static(#header_name);
    };

    tokens.extend(header);
  }
}

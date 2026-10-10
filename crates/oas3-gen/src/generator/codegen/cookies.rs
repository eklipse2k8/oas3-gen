use proc_macro2::TokenStream;
use quote::{ToTokens, quote};

use crate::generator::{
  ast::{FieldDef, ParameterLocation, RustPrimitive, StructDef, StructKind},
  converter::GenerationTarget,
};

/// Converts a cookie parameter struct to and from the `Cookie` header.
///
/// Cookie parameters use OpenAPI's `form` style: an array's items are joined with `,`.
/// The client renders the struct into `name=value` pairs, percent-encoded by the
/// `cookie` crate's `Cookie::encoded`; the server parses the request's `Cookie` header
/// back into it with `Cookie::split_parse_encoded`, rejecting a missing required cookie
/// or a value that doesn't parse.
#[derive(Clone, Copy, Debug)]
pub(crate) struct CookieParamsFragment<'a> {
  def: &'a StructDef,
  target: GenerationTarget,
}

impl<'a> CookieParamsFragment<'a> {
  pub(crate) fn new(def: &'a StructDef, target: GenerationTarget) -> Self {
    Self { def, target }
  }

  fn render_pairs(self) -> TokenStream {
    let struct_name = &self.def.name;
    let pushes = self.def.fields.iter().filter_map(|field| {
      let name = field.original_name.as_deref()?;
      let ident = &field.name;
      let rendered = render_value(field);
      let push = quote! {
        pairs.push(reqwest_cookie_store::RawCookie::new(#name, #rendered).encoded().to_string());
      };
      Some(if field.rust_type.nullable {
        quote! {
          if let Some(value) = &self.#ident {
            #push
          }
        }
      } else {
        quote! {
          let value = &self.#ident;
          #push
        }
      })
    });

    quote! {
      impl #struct_name {
        /// The `name=value` pairs these cookies send, percent-encoded for a `Cookie` header.
        #[must_use]
        pub fn cookie_pairs(&self) -> Vec<String> {
          let mut pairs = vec![];
          #(#pushes)*
          pairs
        }
      }
    }
  }

  fn parse_header(self) -> TokenStream {
    let struct_name = &self.def.name;
    let fields = self.def.fields.iter().filter_map(|field| {
      let name = field.original_name.as_deref()?;
      let ident = &field.name;
      let invalid_message = format!("invalid value for cookie `{name}`");
      let missing_message = format!("missing required cookie `{name}`");
      let invalid = quote! { anyhow::anyhow!(#invalid_message) };
      let parsed = if field.rust_type.is_array {
        quote! { value.split(',').map(|item| item.parse()).collect::<Result<_, _>>() }
      } else {
        quote! { value.parse() }
      };
      Some(if field.rust_type.nullable {
        quote! {
          #ident: cookie(#name)
            .map(|value| #parsed)
            .transpose()
            .map_err(|_| #invalid)?,
        }
      } else {
        quote! {
          #ident: cookie(#name)
            .ok_or_else(|| anyhow::anyhow!(#missing_message))
            .and_then(|value| #parsed.map_err(|_| #invalid))?,
        }
      })
    });

    let cookie_lookup = request_cookie_lookup(&quote! { headers }, CookieValues::Decoded);

    quote! {
      impl core::convert::TryFrom<&http::HeaderMap> for #struct_name {
        type Error = anyhow::Error;

        fn try_from(headers: &http::HeaderMap) -> anyhow::Result<Self> {
          #cookie_lookup
          Ok(Self {
            #(#fields)*
          })
        }
      }
    }
  }
}

impl ToTokens for CookieParamsFragment<'_> {
  fn to_tokens(&self, tokens: &mut TokenStream) {
    if self.def.kind != StructKind::CookieParams || self.def.fields.is_empty() {
      return;
    }
    tokens.extend(match self.target {
      GenerationTarget::Client => self.render_pairs(),
      GenerationTarget::Server => self.parse_header(),
    });
  }
}

/// The form in which a request cookie lookup returns values.
#[derive(Clone, Copy, Debug)]
pub(crate) enum CookieValues {
  /// As the request sent them, for cookie API keys, which a client's cookie store sends unencoded
  AsSent,
  /// Percent-decoded, for cookie parameters, which a generated client percent-encodes
  Decoded,
}

/// Binds `cookie`, which finds the value of the request cookie with a given name, from
/// the `Cookie` headers in the `http::HeaderMap` `headers` evaluates to.
pub(crate) fn request_cookie_lookup(headers: &TokenStream, values: CookieValues) -> TokenStream {
  let split = match values {
    CookieValues::AsSent => quote! { axum_extra::extract::cookie::Cookie::split_parse },
    CookieValues::Decoded => quote! { axum_extra::extract::cookie::Cookie::split_parse_encoded },
  };
  quote! {
    let cookies = #headers
      .get_all(http::header::COOKIE)
      .iter()
      .filter_map(|value| value.to_str().ok())
      .flat_map(#split)
      .filter_map(Result::ok)
      .collect::<Vec<_>>();
    let cookie = |name: &str| {
      cookies
        .iter()
        .find(|cookie| cookie.name() == name)
        .map(axum_extra::extract::cookie::Cookie::value_trimmed)
    };
  }
}

/// The text of one cookie value bound as `value`, before encoding.
fn render_value(field: &FieldDef) -> TokenStream {
  if field.rust_type.is_array {
    quote! { value.iter().map(ToString::to_string).collect::<Vec<_>>().join(",") }
  } else if field.rust_type.base_type == RustPrimitive::String {
    quote! { value }
  } else {
    quote! { value.to_string() }
  }
}

/// Builds the `Cookie` header an operation's cookie parameters send, as `cookie_header`.
///
/// A request that carries its own `Cookie` header keeps `reqwest` from adding the
/// client's cookie store, so the stored cookies for the URL are merged in first.
#[derive(Clone, Copy, Debug)]
pub(crate) struct CookieHeaderFragment {
  has_cookie_store: bool,
}

impl CookieHeaderFragment {
  /// The fragment for an operation that sends cookie parameters, or `None` when it sends none.
  pub(crate) fn new(parameters: &[FieldDef], has_cookie_store: bool) -> Option<Self> {
    parameters
      .iter()
      .any(|parameter| parameter.parameter_location == Some(ParameterLocation::Cookie))
      .then_some(Self { has_cookie_store })
  }

  /// Attaches `cookie_header` to `req_builder`.
  pub(crate) fn attach() -> TokenStream {
    quote! {
      if let Some(cookie) = cookie_header {
        req_builder = req_builder.header(reqwest::header::COOKIE, cookie);
      }
    }
  }
}

impl ToTokens for CookieHeaderFragment {
  fn to_tokens(&self, tokens: &mut TokenStream) {
    let pairs = if self.has_cookie_store {
      quote! {
        reqwest::cookie::CookieStore::cookies(&*self.cookies, &url)
          .and_then(|stored| stored.to_str().ok().map(str::to_owned))
          .into_iter()
          .chain(request.cookie.cookie_pairs())
          .collect::<Vec<_>>()
      }
    } else {
      quote! { request.cookie.cookie_pairs() }
    };
    tokens.extend(quote! {
      let cookie_header = {
        let pairs = #pairs;
        (!pairs.is_empty()).then(|| pairs.join("; "))
      };
    });
  }
}

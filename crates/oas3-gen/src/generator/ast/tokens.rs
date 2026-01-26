use std::{
  fmt::{Display, Formatter},
  str::FromStr,
};

use proc_macro2::{Span, TokenStream};
use quote::{IdentFragment, ToTokens};
pub use string_cache::DefaultAtom;
use syn::{Ident, LitStr};

use crate::generator::naming::identifiers::{
  to_http_header_name, to_rust_const_name, to_rust_field_name, to_rust_type_name,
};

macro_rules! define_ident_token {
  (
    $(#[$meta:meta])*
    $name:ident => $convert_fn:path
  ) => {
    $(#[$meta])*
    #[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
    pub struct $name(DefaultAtom);

    #[allow(dead_code)]
    impl $name {
      #[must_use]
      pub fn new(ident: impl AsRef<str>) -> Self {
        Self(ident.as_ref().into())
      }

      #[must_use]
      pub fn from_raw(value: impl AsRef<str>) -> Self {
        Self($convert_fn(value.as_ref()).into())
      }

      #[must_use]
      pub fn is_empty(&self) -> bool {
        self.0.is_empty()
      }

      #[must_use]
      pub fn to_atom(&self) -> DefaultAtom {
        self.0.clone()
      }

      #[must_use]
      pub fn as_str(&self) -> &str {
        &self.0
      }
    }

    impl Display for $name {
      fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
      }
    }

    impl PartialEq<&str> for $name {
      fn eq(&self, other: &&str) -> bool {
        self.as_str() == *other
      }
    }

    impl PartialEq<$name> for &str {
      fn eq(&self, other: &$name) -> bool {
        *self == other.as_str()
      }
    }

    impl From<&str> for $name {
      fn from(s: &str) -> Self {
        Self(s.into())
      }
    }

    impl From<String> for $name {
      fn from(s: String) -> Self {
        Self(s.into())
      }
    }

    impl From<&String> for $name {
      fn from(s: &String) -> Self {
        Self(s.as_str().into())
      }
    }

    impl From<DefaultAtom> for $name {
      fn from(atom: DefaultAtom) -> Self {
        Self(atom)
      }
    }

    impl FromStr for $name {
      type Err = std::convert::Infallible;

      fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(Self($convert_fn(s).into()))
      }
    }

    impl ToTokens for $name {
      fn to_tokens(&self, tokens: &mut TokenStream) {
        let token = if let Some(raw_ident) = self.0.strip_prefix("r#") {
          Ident::new_raw(raw_ident, Span::call_site())
        } else {
          Ident::new(&self.0, Span::call_site())
        };
        token.to_tokens(tokens);
      }
    }

    impl IdentFragment for $name {
      fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        Display::fmt(self, f)
      }

      fn span(&self) -> Option<Span> {
        Some(Span::call_site())
      }
    }
  };
}

macro_rules! define_literal_token {
  (
    $(#[$meta:meta])*
    $name:ident => $convert_fn:path
  ) => {
    $(#[$meta])*
    #[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
    pub struct $name(DefaultAtom);

    #[allow(dead_code)]
    impl $name {
      #[must_use]
      pub fn new(literal: impl AsRef<str>) -> Self {
        Self(literal.as_ref().into())
      }

      #[must_use]
      pub fn from_raw(value: impl AsRef<str>) -> Self {
        Self($convert_fn(value.as_ref()).into())
      }

      #[must_use]
      pub fn is_empty(&self) -> bool {
        self.0.is_empty()
      }

      #[must_use]
      pub fn to_atom(&self) -> DefaultAtom {
        self.0.clone()
      }
    }

    impl Display for $name {
      fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
      }
    }

    impl From<&str> for $name {
      fn from(s: &str) -> Self {
        Self(s.into())
      }
    }

    impl FromStr for $name {
      type Err = std::convert::Infallible;

      fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(Self($convert_fn(s).into()))
      }
    }

    impl ToTokens for $name {
      fn to_tokens(&self, tokens: &mut TokenStream) {
        let lit = LitStr::new(&self.0, Span::call_site());
        lit.to_tokens(tokens);
      }
    }
  };
}

define_ident_token!(
  /// Token representing a Struct
  StructToken => to_rust_type_name
);

define_ident_token!(
  /// Token representing an Enum
  EnumToken => to_rust_type_name
);

define_ident_token!(
  /// Token representing an Enum variant
  EnumVariantToken => to_rust_type_name
);

define_ident_token!(
  /// Token representing a rust type alias
  TypeAliasToken => to_rust_type_name
);

define_ident_token!(
  /// Token representing a valid Rust method name
  MethodNameToken => to_rust_field_name
);

define_ident_token!(
  /// Token representing a valid Rust struct field name
  FieldNameToken => to_rust_field_name
);

define_ident_token!(
  /// Token representing a const name
  ConstToken => to_rust_const_name
);

define_literal_token!(
  /// Token representing a header name literal
  HeaderNameToken => to_http_header_name
);

impl From<StructToken> for EnumToken {
  fn from(token: StructToken) -> Self {
    Self(token.to_atom())
  }
}

impl From<&StructToken> for EnumToken {
  fn from(token: &StructToken) -> Self {
    Self(token.to_atom())
  }
}

define_ident_token!(
  /// Token representing a Trait
  TraitToken => to_rust_type_name
);

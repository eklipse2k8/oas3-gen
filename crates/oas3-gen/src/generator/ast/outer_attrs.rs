use proc_macro2::TokenStream;
use quote::{ToTokens, quote};

/// Typed outer attributes for structs and enums.
///
/// Replaces the stringly-typed `Vec<String>` approach with a type-safe enum
/// representing all supported outer attributes in generated code.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OuterAttr {
  /// Skips serializing fields that are `None`.
  /// Renders as `#[oas3_gen_support::skip_serializing_none]`
  SkipSerializingNone,
  /// Marks the type as non-exhaustive.
  /// Renders as `#[non_exhaustive]`
  #[allow(dead_code)] // Reserved for future use
  NonExhaustive,
}

impl ToTokens for OuterAttr {
  fn to_tokens(&self, tokens: &mut TokenStream) {
    let attr = match self {
      OuterAttr::SkipSerializingNone => quote! { #[oas3_gen_support::skip_serializing_none] },
      OuterAttr::NonExhaustive => quote! { #[non_exhaustive] },
    };
    tokens.extend(attr);
  }
}

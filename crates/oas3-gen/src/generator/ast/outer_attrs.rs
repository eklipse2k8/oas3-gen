use proc_macro2::TokenStream;
use quote::{ToTokens, quote};

/// Typed outer attributes for structs and enums.
///
/// Replaces the stringly-typed `Vec<String>` approach with a type-safe enum
/// representing all supported outer attributes in generated code.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OuterAttr {
  /// Skips serializing fields that are `None`.
  /// Renders as `#[serde_with::skip_serializing_none]`
  SkipSerializingNone,
  /// Enables `serde_as` transformations on fields.
  /// Renders as `#[serde_with::serde_as]`
  SerdeAs,
}

impl ToTokens for OuterAttr {
  fn to_tokens(&self, tokens: &mut TokenStream) {
    let attr = match self {
      Self::SkipSerializingNone => quote! { #[serde_with::skip_serializing_none] },
      Self::SerdeAs => quote! { #[serde_with::serde_as] },
    };
    tokens.extend(attr);
  }
}

/// Separator type for non-exploded array query parameters.
///
/// Maps OpenAPI style/explode combinations to `serde_with` separator types.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SerdeAsSeparator {
  Comma,
  Space,
  Pipe,
}

impl ToTokens for SerdeAsSeparator {
  fn to_tokens(&self, tokens: &mut TokenStream) {
    let separator_type = match self {
      Self::Comma => quote! { oas3_gen_support::StringWithCommaSeparator },
      Self::Space => quote! { oas3_gen_support::StringWithSpaceSeparator },
      Self::Pipe => quote! { oas3_gen_support::StringWithPipeSeparator },
    };
    tokens.extend(separator_type);
  }
}

/// Field-level `#[serde_as]` attribute for custom serialization.
///
/// Used for non-exploded array query parameters that need custom
/// serialization via separator-based string conversion.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SerdeAsFieldAttr {
  SeparatedList {
    separator: SerdeAsSeparator,
    optional: bool,
  },
}

impl ToTokens for SerdeAsFieldAttr {
  fn to_tokens(&self, tokens: &mut TokenStream) {
    let Self::SeparatedList { separator, optional } = self;
    let attr = if *optional {
      quote! { #[serde_as(as = Option<#separator>)] }
    } else {
      quote! { #[serde_as(as = #separator)] }
    };
    tokens.extend(attr);
  }
}

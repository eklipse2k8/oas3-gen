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
  /// Enables `serde_as` transformations on fields.
  /// Renders as `#[oas3_gen_support::serde_as]`
  SerdeAs,
}

impl ToTokens for OuterAttr {
  fn to_tokens(&self, tokens: &mut TokenStream) {
    let attr = match self {
      OuterAttr::SkipSerializingNone => quote! { #[oas3_gen_support::skip_serializing_none] },
      OuterAttr::SerdeAs => quote! { #[oas3_gen_support::serde_as] },
    };
    tokens.extend(attr);
  }
}

/// Separator type for non-exploded array query parameters.
///
/// Maps OpenAPI style/explode combinations to `serde_with` separator types.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SerdeAsSeparator {
  /// Comma-separated values (OpenAPI form style with explode=false)
  Comma,
  /// Space-separated values (OpenAPI spaceDelimited style)
  Space,
  /// Pipe-separated values (OpenAPI pipeDelimited style)
  Pipe,
}

impl SerdeAsSeparator {
  fn type_path(self) -> &'static str {
    match self {
      SerdeAsSeparator::Comma => "oas3_gen_support::StringWithCommaSeparator",
      SerdeAsSeparator::Space => "oas3_gen_support::StringWithSpaceSeparator",
      SerdeAsSeparator::Pipe => "oas3_gen_support::StringWithPipeSeparator",
    }
  }
}

impl ToTokens for SerdeAsSeparator {
  fn to_tokens(&self, tokens: &mut TokenStream) {
    let separator_type = match self {
      SerdeAsSeparator::Comma => quote! { oas3_gen_support::StringWithCommaSeparator },
      SerdeAsSeparator::Space => quote! { oas3_gen_support::StringWithSpaceSeparator },
      SerdeAsSeparator::Pipe => quote! { oas3_gen_support::StringWithPipeSeparator },
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
    let attr = match self {
      SerdeAsFieldAttr::SeparatedList { separator, optional } => {
        let type_str = if *optional {
          format!("Option<{}>", separator.type_path())
        } else {
          separator.type_path().to_string()
        };
        quote! { #[serde_as(as = #type_str)] }
      }
    };
    tokens.extend(attr);
  }
}

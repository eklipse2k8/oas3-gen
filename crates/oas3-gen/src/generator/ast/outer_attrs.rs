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
#[derive(Debug, Clone, Copy, PartialEq, Eq, strum::AsRefStr)]
pub enum SerdeAsSeparator {
  #[strum(serialize = "oas3_gen_support::StringWithCommaSeparator")]
  Comma,
  #[strum(serialize = "oas3_gen_support::StringWithSpaceSeparator")]
  Space,
  #[strum(serialize = "oas3_gen_support::StringWithPipeSeparator")]
  Pipe,
}

/// Field-level `#[serde_as]` attribute for custom serialization.
///
/// Used for non-exploded array query parameters that need custom
/// serialization via separator-based string conversion, or for
/// custom type overrides via the `--customize` CLI flag.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SerdeAsFieldAttr {
  SeparatedList {
    separator: SerdeAsSeparator,
    optional: bool,
  },
  CustomOverride {
    custom_type: String,
    optional: bool,
    is_array: bool,
  },
}

impl ToTokens for SerdeAsFieldAttr {
  fn to_tokens(&self, tokens: &mut TokenStream) {
    let type_str = match self {
      Self::SeparatedList { separator, optional } => {
        if *optional {
          format!("Option<{}>", separator.as_ref())
        } else {
          separator.as_ref().to_string()
        }
      }
      Self::CustomOverride {
        custom_type,
        optional,
        is_array,
      } => {
        let mut result = custom_type.clone();
        if *is_array {
          result = format!("Vec<{result}>");
        }
        if *optional {
          result = format!("Option<{result}>");
        }
        result
      }
    };
    let attr = quote! { #[serde_as(as = #type_str)] };
    tokens.extend(attr);
  }
}

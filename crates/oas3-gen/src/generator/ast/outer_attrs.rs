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
  /// A `uniqueItems` array, which rejects a repeated item instead of collapsing it
  UniqueItems { optional: bool },
  /// A required field that reads `null` as its type's default, through `inner` when the
  /// field has its own adapter
  DefaultOnNull { inner: Option<Box<SerdeAsFieldAttr>> },
}

impl SerdeAsFieldAttr {
  /// The `--customize` type the field serializes through, looking past `DefaultOnNull`.
  #[must_use]
  pub fn custom_type(&self) -> Option<&str> {
    match self {
      Self::CustomOverride { custom_type, .. } => Some(custom_type),
      Self::DefaultOnNull { inner: Some(inner) } => inner.custom_type(),
      Self::SeparatedList { .. } | Self::UniqueItems { .. } | Self::DefaultOnNull { inner: None } => None,
    }
  }

  /// The `serde_with` adapter type written in `#[serde_as(as = "...")]`.
  fn adapter(&self) -> String {
    match self {
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
      Self::UniqueItems { optional: true } => "Option<serde_with::SetPreventDuplicates<_>>".to_string(),
      Self::UniqueItems { optional: false } => "serde_with::SetPreventDuplicates<_>".to_string(),
      Self::DefaultOnNull { inner: None } => "serde_with::DefaultOnNull".to_string(),
      Self::DefaultOnNull { inner: Some(inner) } => format!("serde_with::DefaultOnNull<{}>", inner.adapter()),
    }
  }
}

impl ToTokens for SerdeAsFieldAttr {
  fn to_tokens(&self, tokens: &mut TokenStream) {
    let type_str = self.adapter();
    let attr = quote! { #[serde_as(as = #type_str)] };
    tokens.extend(attr);
  }
}

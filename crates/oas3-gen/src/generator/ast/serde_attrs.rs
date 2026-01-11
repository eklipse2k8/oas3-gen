use proc_macro2::TokenStream;
use quote::{ToTokens, quote};

/// Represents a serde attribute applied to structs, fields, or enum variants.
///
/// These attributes control serialization and deserialization behavior in generated Rust code.
/// Each variant maps directly to a serde attribute that will be rendered in the output.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum SerdeAttribute {
  Alias(String),
  Default,
  DenyUnknownFields,
  Flatten,
  Rename(String),
  Skip,
  SkipDeserializing,
  Untagged,
}

impl ToTokens for SerdeAttribute {
  fn to_tokens(&self, tokens: &mut TokenStream) {
    let attr = match self {
      Self::Alias(name) => quote! { alias = #name },
      Self::Default => quote! { default },
      Self::DenyUnknownFields => quote! { deny_unknown_fields },
      Self::Flatten => quote! { flatten },
      Self::Rename(name) => quote! { rename = #name },
      Self::Skip => quote! { skip },
      Self::SkipDeserializing => quote! { skip_deserializing },
      Self::Untagged => quote! { untagged },
    };
    tokens.extend(attr);
  }
}

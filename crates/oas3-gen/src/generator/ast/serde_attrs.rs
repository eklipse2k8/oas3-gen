use proc_macro2::TokenStream;
use quote::{ToTokens, quote};

/// Represents a serde attribute applied to structs, fields, or enum variants.
///
/// These attributes control serialization and deserialization behavior in generated Rust code.
/// Each variant maps directly to a serde attribute that will be rendered in the output.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum SerdeAttribute {
  Rename(String),
  Alias(String),
  Default,
  Flatten,
  Skip,
  SkipDeserializing,
  DenyUnknownFields,
  Untagged,
  /// Internal enum tagging: `#[serde(tag = "...")]`
  Tag(String),
}

impl ToTokens for SerdeAttribute {
  fn to_tokens(&self, tokens: &mut TokenStream) {
    let attr = match self {
      SerdeAttribute::Rename(name) => quote! { rename = #name },
      SerdeAttribute::Alias(name) => quote! { alias = #name },
      SerdeAttribute::Default => quote! { default },
      SerdeAttribute::Flatten => quote! { flatten },
      SerdeAttribute::Skip => quote! { skip },
      SerdeAttribute::SkipDeserializing => quote! { skip_deserializing },
      SerdeAttribute::DenyUnknownFields => quote! { deny_unknown_fields },
      SerdeAttribute::Untagged => quote! { untagged },
      SerdeAttribute::Tag(field) => quote! { tag = #field },
    };
    tokens.extend(attr);
  }
}

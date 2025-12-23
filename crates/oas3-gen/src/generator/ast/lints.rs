use proc_macro2::TokenStream;
use quote::{ToTokens, quote};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LintAllow {
  ClippyDefaultTraitAccess,
  ClippyDocMarkdown,
  ClippyEnumVariantNames,
  ClippyLargeEnumVariant,
  ClippyMissingPanicsDoc,
  ClippyResultLargeErr,
  ClippyStructFieldNames,
  ClippyTooManyLines,
  ClippyUnnecessaryWraps,
  ClippyUnusedSelf,
  DeadCode,
}

impl ToTokens for LintAllow {
  fn to_tokens(&self, tokens: &mut TokenStream) {
    let attr = match self {
      Self::ClippyDefaultTraitAccess => quote! { #![allow(clippy::default_trait_access)] },
      Self::ClippyDocMarkdown => quote! { #![allow(clippy::doc_markdown)] },
      Self::ClippyEnumVariantNames => quote! { #![allow(clippy::enum_variant_names)] },
      Self::ClippyLargeEnumVariant => quote! { #![allow(clippy::large_enum_variant)] },
      Self::ClippyMissingPanicsDoc => quote! { #![allow(clippy::missing_panics_doc)] },
      Self::ClippyResultLargeErr => quote! { #![allow(clippy::result_large_err)] },
      Self::ClippyStructFieldNames => quote! { #![allow(clippy::struct_field_names)] },
      Self::ClippyTooManyLines => quote! { #![allow(clippy::too_many_lines)] },
      Self::ClippyUnnecessaryWraps => quote! { #![allow(clippy::unnecessary_wraps)] },
      Self::ClippyUnusedSelf => quote! { #![allow(clippy::unused_self)] },
      Self::DeadCode => quote! { #![allow(dead_code)] },
    };
    tokens.extend(attr);
  }
}

#[derive(Debug, Clone)]
pub struct LintConfig {
  pub allows: Vec<LintAllow>,
}

impl Default for LintConfig {
  fn default() -> Self {
    Self {
      allows: vec![
        LintAllow::ClippyDefaultTraitAccess,
        LintAllow::ClippyDocMarkdown,
        LintAllow::ClippyEnumVariantNames,
        LintAllow::ClippyLargeEnumVariant,
        LintAllow::ClippyMissingPanicsDoc,
        LintAllow::ClippyResultLargeErr,
        LintAllow::ClippyStructFieldNames,
        LintAllow::ClippyTooManyLines,
        LintAllow::ClippyUnnecessaryWraps,
        LintAllow::ClippyUnusedSelf,
        LintAllow::DeadCode,
      ],
    }
  }
}

impl ToTokens for LintConfig {
  fn to_tokens(&self, tokens: &mut TokenStream) {
    for allow in &self.allows {
      allow.to_tokens(tokens);
    }
  }
}

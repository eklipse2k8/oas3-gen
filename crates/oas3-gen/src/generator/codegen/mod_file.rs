use proc_macro2::{Ident, Span, TokenStream};
use quote::{ToTokens, quote};

use super::Visibility;
use crate::generator::{
  ast::{ApiMetadata, GlobalLintsNode},
  codegen::generate_source,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModFileKind {
  Client,
  Server,
}

impl ModFileKind {
  pub(crate) const fn label(self) -> &'static str {
    match self {
      Self::Client => "client",
      Self::Server => "server",
    }
  }
}

#[derive(Debug, Clone)]
pub struct ModFileFragment {
  metadata: ApiMetadata,
  visibility: Visibility,
  kind: ModFileKind,
  source_path: String,
  gen_version: String,
}

impl ModFileFragment {
  pub fn new(
    metadata: ApiMetadata,
    visibility: Visibility,
    kind: ModFileKind,
    source_path: String,
    gen_version: String,
  ) -> Self {
    Self {
      metadata,
      visibility,
      kind,
      source_path,
      gen_version,
    }
  }

  pub fn for_client(metadata: ApiMetadata, visibility: Visibility, source_path: String, gen_version: String) -> Self {
    Self::new(metadata, visibility, ModFileKind::Client, source_path, gen_version)
  }

  pub fn for_server(metadata: ApiMetadata, visibility: Visibility, source_path: String, gen_version: String) -> Self {
    Self::new(metadata, visibility, ModFileKind::Server, source_path, gen_version)
  }

  pub fn generate(&self) -> anyhow::Result<String> {
    let lint_config = GlobalLintsNode::default();
    generate_source(
      &self.to_token_stream(),
      &self.metadata,
      Some(&lint_config),
      &self.source_path,
      &self.gen_version,
    )
  }
}

impl ToTokens for ModFileFragment {
  fn to_tokens(&self, tokens: &mut TokenStream) {
    let secondary_mod = Ident::new(self.kind.label(), Span::call_site());
    let reexports = match self.visibility {
      Visibility::File => None,
      visibility => {
        let vis = visibility.to_tokens();
        Some(quote! {
          #vis use types::*;
          #vis use #secondary_mod::*;
        })
      }
    };

    tokens.extend(quote! {
      mod types;
      mod #secondary_mod;

      #reexports
    });
  }
}

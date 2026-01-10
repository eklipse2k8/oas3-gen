use quote::quote;

use super::Visibility;
use crate::generator::{
  ast::{ClientRootNode, LintConfig},
  codegen::generate_source,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModFileKind {
  Client,
  Server,
}

pub struct ModFileGenerator {
  metadata: ClientRootNode,
  visibility: Visibility,
  kind: ModFileKind,
}

impl ModFileGenerator {
  pub fn new(metadata: &ClientRootNode, visibility: Visibility) -> Self {
    Self {
      metadata: metadata.clone(),
      visibility,
      kind: ModFileKind::Client,
    }
  }

  pub fn for_server(metadata: &ClientRootNode, visibility: Visibility) -> Self {
    Self {
      metadata: metadata.clone(),
      visibility,
      kind: ModFileKind::Server,
    }
  }

  pub fn generate(&self, source_path: &str, gen_version: &str) -> anyhow::Result<String> {
    let vis = self.visibility.to_tokens();

    let code = match self.kind {
      ModFileKind::Client => quote! {
        mod types;
        mod client;

        #vis use types::*;
        #vis use client::*;
      },
      ModFileKind::Server => quote! {
        mod types;
        mod server;

        #vis use types::*;
        #vis use server::*;
      },
    };

    let lint_config = LintConfig::default();
    generate_source(&code, &self.metadata, Some(&lint_config), source_path, gen_version)
  }
}

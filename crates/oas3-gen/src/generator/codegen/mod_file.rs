use quote::quote;
use syn::Ident;

use super::{Visibility, metadata::CodeMetadata};
use crate::generator::ast::LintConfig;

pub struct ModFileGenerator<'a> {
  metadata: &'a CodeMetadata,
  client_ident: &'a Ident,
  visibility: Visibility,
}

impl<'a> ModFileGenerator<'a> {
  pub fn new(metadata: &'a CodeMetadata, client_ident: &'a Ident, visibility: Visibility) -> Self {
    Self {
      metadata,
      client_ident,
      visibility,
    }
  }

  pub fn generate(&self, source_path: &str, gen_version: &str) -> anyhow::Result<String> {
    let vis = self.visibility.to_tokens();
    let client_ident = self.client_ident;

    let code = quote! {
      mod types;
      mod client;

      #vis use types::*;
      #vis use client::#client_ident;
    };

    let lint_config = LintConfig::default();
    super::generate_source(&code, self.metadata, Some(&lint_config), source_path, gen_version)
  }
}

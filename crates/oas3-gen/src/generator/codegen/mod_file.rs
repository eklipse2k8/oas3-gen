use quote::quote;

use super::Visibility;
use crate::generator::{
  ast::{ClientDef, LintConfig},
  codegen::generate_source,
};

pub struct ModFileGenerator<'a> {
  metadata: &'a ClientDef,
  visibility: Visibility,
}

impl<'a> ModFileGenerator<'a> {
  pub fn new(metadata: &'a ClientDef, visibility: Visibility) -> Self {
    Self { metadata, visibility }
  }

  pub fn generate(&self, source_path: &str, gen_version: &str) -> anyhow::Result<String> {
    let vis = self.visibility.to_tokens();

    let code = quote! {
      mod types;
      mod client;

      #vis use types::*;
      #vis use client::*;
    };

    let lint_config = LintConfig::default();
    generate_source(&code, self.metadata, Some(&lint_config), source_path, gen_version)
  }
}

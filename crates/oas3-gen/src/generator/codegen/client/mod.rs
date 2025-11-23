use std::collections::BTreeSet;

use proc_macro2::TokenStream;
use quote::{format_ident, quote};

use crate::{
  generator::{
    ast::{OperationInfo, ParameterLocation, RustType},
    codegen::constants,
  },
  naming::identifiers::to_rust_type_name,
};

mod methods;

pub fn generate_client(
  spec: &oas3::Spec,
  operations: &[OperationInfo],
  rust_types: &[RustType],
) -> anyhow::Result<TokenStream> {
  let metadata = extract_metadata(spec);
  let client_name = if metadata.title.is_empty() {
    "Api".to_string()
  } else {
    to_rust_type_name(&metadata.title)
  };
  let client_ident = format_ident!("{client_name}Client");

  let base_url = spec
    .servers
    .first()
    .map_or_else(|| "https://example.com/".to_string(), |server| server.url.clone());
  let base_url_lit = syn::LitStr::new(&base_url, proc_macro2::Span::call_site());

  let header_names = extract_header_names(operations);
  let header_refs: Vec<&String> = header_names.iter().collect();
  let header_consts = constants::generate_header_constants(&header_refs);

  let mut method_tokens = Vec::new();
  for operation in operations {
    method_tokens.push(methods::build_method_tokens(operation, rust_types)?);
  }

  Ok(quote! {
    use anyhow::Context;
    use reqwest::{
      Client,
      Url,
    };
    #[allow(unused_imports)]
    use reqwest::multipart::{Form, Part};
    #[allow(unused_imports)]
    use reqwest::header::HeaderValue;
    use validator::Validate;

    const BASE_URL: &str = #base_url_lit;

    #header_consts

    #[derive(Debug, Clone)]
    pub struct #client_ident {
      client: Client,
      base_url: Url,
    }

    impl #client_ident {
      /// Create a client using the OpenAPI `servers[0]` URL.
      #[must_use]
      pub fn new() -> Self {
        let default_client = {
          Client::builder()
            .build()
            .expect("client")
        };

        let url = Url::parse(BASE_URL).expect("valid base url");

        Self {
          client: default_client,
          base_url: url,
        }
      }

      /// Create a client with a custom base URL.
      pub fn with_base_url(base_url: impl AsRef<str>) -> anyhow::Result<Self> {
        let client = {
          Client::builder()
            .build()
            .expect("client")
        };

        let url = Url::parse(base_url.as_ref()).context("parsing base url")?;

        Ok(Self { client, base_url: url })
      }

      /// Create a client from an existing `reqwest::Client`.
      pub fn with_client(base_url: impl AsRef<str>, client: Client) -> anyhow::Result<Self> {
        let url = Url::parse(base_url.as_ref()).context("parsing base url")?;
        Ok(Self { client, base_url: url })
      }

      #(#method_tokens)*
    }
  })
}

struct Metadata {
  title: String,
}

fn extract_metadata(spec: &oas3::Spec) -> Metadata {
  Metadata {
    title: spec.info.title.clone(),
  }
}

fn extract_header_names(operations: &[OperationInfo]) -> BTreeSet<String> {
  operations
    .iter()
    .flat_map(|op| &op.parameters)
    .filter(|param| matches!(param.location, ParameterLocation::Header))
    .map(|param| param.original_name.to_ascii_lowercase().clone())
    .collect()
}

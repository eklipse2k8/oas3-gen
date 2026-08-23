use oas3::{Spec, spec::Server};

use crate::generator::{ast::StructToken, naming::identifiers::to_rust_type_name};

const DEFAULT_BASE_URL: &str = "https://example.com/";

/// Root node for client generation: the client struct name and its default base URL.
#[derive(Debug, Clone, Default)]
pub struct ClientRootNode {
  pub name: StructToken,
  pub base_url: String,
}

#[bon::bon]
impl ClientRootNode {
  #[builder]
  pub fn new(name: StructToken, servers: &[Server]) -> Self {
    Self {
      name,
      base_url: servers
        .first()
        .map_or_else(|| DEFAULT_BASE_URL.to_string(), |server| server.url.clone()),
    }
  }
}

impl StructToken {
  fn new_client(name: Option<&str>, fallback: &str) -> Self {
    let struct_name = if let Some(name) = name
      && !name.is_empty()
    {
      name.to_string()
    } else if fallback.is_empty() {
      "ApiClient".to_string()
    } else {
      format!("{}Client", to_rust_type_name(fallback))
    };

    Self::from_raw(struct_name)
  }
}

impl ClientRootNode {
  /// Creates the client root from the spec, using `name` for the
  /// client struct name when provided instead of deriving it from the spec title.
  pub fn from_spec(spec: &Spec, name: Option<&str>) -> Self {
    Self::builder()
      .name(StructToken::new_client(name, &spec.info.title))
      .servers(&spec.servers)
      .build()
  }
}

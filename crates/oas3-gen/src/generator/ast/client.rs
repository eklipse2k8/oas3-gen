use oas3::{
  Spec,
  spec::{Info, Server},
};

use crate::generator::{ast::StructToken, naming::identifiers::to_rust_type_name};

const DEFAULT_BASE_URL: &str = "https://example.com/";

#[derive(Debug, Clone, Default)]
pub struct ClientRootNode {
  pub name: StructToken,
  pub title: String,
  pub version: String,
  pub description: Option<String>,
  pub base_url: String,
}

#[bon::bon]
impl ClientRootNode {
  #[builder]
  pub fn new(name: StructToken, info: &Info, servers: &[Server]) -> Self {
    Self {
      name,
      title: info.title.clone(),
      version: info.version.clone(),
      description: info.description.clone(),
      base_url: servers
        .first()
        .map_or_else(|| DEFAULT_BASE_URL.to_string(), |server| server.url.clone()),
    }
  }
}

impl From<&Spec> for ClientRootNode {
  fn from(value: &Spec) -> Self {
    ClientRootNode::builder()
      .name(StructToken::new(if value.info.title.is_empty() {
        "ApiClient".to_string()
      } else {
        format!("{}Client", to_rust_type_name(&value.info.title))
      }))
      .info(&value.info)
      .servers(&value.servers)
      .build()
  }
}

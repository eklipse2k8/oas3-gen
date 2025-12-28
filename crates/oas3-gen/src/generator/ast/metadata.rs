const DEFAULT_BASE_URL: &str = "https://example.com/";

#[derive(Debug, Clone)]
pub struct CodeMetadata {
  pub title: String,
  pub version: String,
  pub description: Option<String>,
  pub base_url: String,
}

impl From<&oas3::Spec> for CodeMetadata {
  fn from(spec: &oas3::Spec) -> Self {
    Self {
      title: spec.info.title.clone(),
      version: spec.info.version.clone(),
      description: spec.info.description.clone(),
      base_url: spec
        .servers
        .first()
        .map_or_else(|| DEFAULT_BASE_URL.to_string(), |server| server.url.clone()),
    }
  }
}

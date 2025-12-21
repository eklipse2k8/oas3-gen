#[derive(Debug, Clone)]
pub struct LintConfig {
  pub clippy_allows: Vec<String>,
}

impl Default for LintConfig {
  fn default() -> Self {
    Self {
      clippy_allows: vec![
        "clippy::doc_markdown".to_string(),
        "clippy::enum_variant_names".to_string(),
        "clippy::large_enum_variant".to_string(),
        "clippy::missing_panics_doc".to_string(),
        "clippy::result_large_err".to_string(),
        "clippy::unnecessary_wraps".to_string(),
        "clippy::unused_self".to_string(),
        "dead_code".to_string(),
      ],
    }
  }
}

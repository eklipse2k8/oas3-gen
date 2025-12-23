use crate::generator::ast::ParsedPath;

#[test]
fn extract_template_params_simple() {
  let params: Vec<_> = ParsedPath::extract_template_params("/projects/{projectKey}/repos/{repositorySlug}").collect();
  assert_eq!(params, vec!["projectKey", "repositorySlug"]);
}

#[test]
fn extract_template_params_no_params() {
  let params: Vec<_> = ParsedPath::extract_template_params("/api/v1/status").collect();
  assert!(params.is_empty());
}

#[test]
fn extract_template_params_single() {
  let params: Vec<_> = ParsedPath::extract_template_params("/users/{id}").collect();
  assert_eq!(params, vec!["id"]);
}

#[test]
fn extract_template_params_adjacent() {
  let params: Vec<_> = ParsedPath::extract_template_params("/{a}{b}/{c}").collect();
  assert_eq!(params, vec!["a", "b", "c"]);
}

use super::support::{assert_contains_all, generate_types, make_orchestrator, parse_spec};

#[test]
fn test_undeclared_path_parameters_are_synthesized() {
  let spec_json = r#"{
    "openapi": "3.1.0",
    "info": { "title": "Undeclared path parameters reproduction", "version": "1.0.0" },
    "paths": {
      "/projects/{projectKey}/repos/{repositorySlug}/status": {
        "get": {
          "operationId": "getRepoStatus",
          "summary": "Get repository status",
          "description": "Get the status of a repository - note: path parameters are NOT declared in this operation",
          "responses": {
            "200": {
              "description": "Repository status",
              "content": {
                "application/json": {
                  "schema": { "type": "object", "properties": { "status": { "type": "string" } } }
                }
              }
            }
          }
        }
      }
    }
  }"#;

  let orchestrator = make_orchestrator(parse_spec(spec_json), false);
  let output = generate_types(&orchestrator, "test.json");

  assert_contains_all(
    &output.code,
    &[
      (
        "GetRepoStatusRequestPath",
        "should generate nested path struct for synthesized params",
      ),
      ("pub project_key: String", "should synthesize project_key field"),
      ("pub repository_slug: String", "should synthesize repository_slug field"),
      (
        "pub path: GetRepoStatusRequestPath",
        "main request should have path field",
      ),
    ],
  );
}

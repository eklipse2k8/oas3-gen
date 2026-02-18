use super::support::{assert_contains_all, generate_types, make_orchestrator, parse_spec};

#[test]
fn test_undeclared_path_parameters_are_synthesized() {
  let spec = parse_spec(include_str!("../../../fixtures/undeclared_path_params.json"));
  let orchestrator = make_orchestrator(spec, false);
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

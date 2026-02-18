use super::support::{assert_contains_all, generate_types, make_orchestrator, parse_spec};

#[test]
fn test_untyped_parameter_generation() {
  let spec = parse_spec(include_str!("../../../fixtures/untyped_parameter.json"));
  let orchestrator = make_orchestrator(spec, false);
  let output = generate_types(&orchestrator, "test.json");

  assert_contains_all(
    &output.code,
    &[
      (
        "Option<serde_json::Value>",
        "should use Option<serde_json::Value> for untyped query param",
      ),
      ("GetItemsRequestQuery", "should generate nested query struct"),
      (
        "pub query: GetItemsRequestQuery",
        "main request should have query field",
      ),
    ],
  );
}

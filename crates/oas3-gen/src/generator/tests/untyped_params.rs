use super::support::{assert_contains_all, generate_types, make_orchestrator, parse_spec};

#[test]
fn test_untyped_parameter_generation() {
  let spec_json = r#"{
    "openapi": "3.1.0",
    "info": { "title": "Untyped parameter reproduction", "version": "1.0.0" },
    "paths": {
      "/items": {
        "get": {
          "operationId": "getItems",
          "parameters": [
            {
              "name": "filter",
              "in": "query",
              "description": "Parameter without an explicit schema type",
              "required": false,
              "schema": {}
            }
          ],
          "responses": {
            "200": {
              "description": "ok",
              "content": { "application/json": { "schema": { "type": "string" } } }
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

use std::collections::HashMap;

use crate::generator::{codegen::Visibility, converter::GenerationTarget, orchestrator::Orchestrator};

fn make_orchestrator(spec: oas3::Spec, all_schemas: bool) -> Orchestrator {
  Orchestrator::new(
    spec,
    Visibility::default(),
    all_schemas,
    None,
    None,
    false,
    false,
    false,
    false,
    GenerationTarget::default(),
    HashMap::new(),
  )
}

#[test]
fn test_untyped_parameter_generation() {
  let spec_json = include_str!("../../../fixtures/untyped_parameter.json");
  let spec: oas3::Spec = oas3::from_json(spec_json).unwrap();
  let orchestrator = make_orchestrator(spec, false);

  let result = orchestrator.generate_with_header("test.json");

  assert!(
    result.is_ok(),
    "Generation failed for untyped parameter: {:?}",
    result.err()
  );

  let (code, _) = result.unwrap();

  assert!(
    code.contains("Option<serde_json::Value>"),
    "Should use Option<serde_json::Value> for untyped query param"
  );

  assert!(
    code.contains("GetItemsRequestQuery"),
    "Should generate nested query struct"
  );

  assert!(
    code.contains("pub query: GetItemsRequestQuery"),
    "Main request should have query field"
  );
}

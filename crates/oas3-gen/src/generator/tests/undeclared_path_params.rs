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
fn test_undeclared_path_parameters_are_synthesized() {
  let spec_json = include_str!("../../../fixtures/undeclared_path_params.json");
  let spec: oas3::Spec = oas3::from_json(spec_json).unwrap();
  let orchestrator = make_orchestrator(spec, false);

  let result = orchestrator.generate_with_header("test.json");

  assert!(
    result.is_ok(),
    "Generation failed for undeclared path params: {:?}",
    result.err()
  );

  let (code, _) = result.unwrap();

  // Should generate path struct with synthesized parameters
  assert!(
    code.contains("GetRepoStatusRequestPath"),
    "Should generate nested path struct for synthesized params"
  );

  // Should have the synthesized fields
  assert!(
    code.contains("pub project_key: String"),
    "Should synthesize project_key field"
  );
  assert!(
    code.contains("pub repository_slug: String"),
    "Should synthesize repository_slug field"
  );

  // Main request should reference the path struct
  assert!(
    code.contains("pub path: GetRepoStatusRequestPath"),
    "Main request should have path field"
  );
}

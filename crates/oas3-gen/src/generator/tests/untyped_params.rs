use crate::generator::{codegen::Visibility, orchestrator::Orchestrator};

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
  )
}

#[test]
fn test_untyped_parameter_generation() {
  let spec_json = include_str!("../../../fixtures/untyped_parameter.json");
  let spec: oas3::Spec = oas3::from_json(spec_json).unwrap();
  let orchestrator = make_orchestrator(spec, false);

  let result = orchestrator.generate_with_header("test.json");

  // This is expected to fail currently
  assert!(
    result.is_ok(),
    "Generation failed for untyped parameter: {:?}",
    result.err()
  );

  let (code, _) = result.unwrap();

  // Check if serde_json::Value is used
  assert!(
    code.contains("Option<serde_json::Value>"),
    "Should use Option<serde_json::Value>"
  );

  // Check if serialize_any_query_param is used for the Value type
  assert!(
    code.contains("oas3_gen_support::serialize_any_query_param(value)"),
    "Should use serialize_any_query_param"
  );
}

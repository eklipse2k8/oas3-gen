use crate::generator::{
  CodegenConfig, SchemaScope, TypesMode,
  codegen::{GeneratedFileType, Visibility},
  orchestrator::Orchestrator,
};

fn make_orchestrator(spec: oas3::Spec, all_schemas: bool) -> Orchestrator {
  let config = CodegenConfig {
    schema_scope: if all_schemas {
      SchemaScope::All
    } else {
      SchemaScope::ReferencedOnly
    },
    ..Default::default()
  };
  Orchestrator::new(spec, Visibility::default(), config, None, None)
}

#[test]
fn test_untyped_parameter_generation() {
  let spec_json = include_str!("../../../fixtures/untyped_parameter.json");
  let spec: oas3::Spec = oas3::from_json(spec_json).unwrap();
  let orchestrator = make_orchestrator(spec, false);

  let result = orchestrator.generate(&TypesMode, "test.json");

  assert!(
    result.is_ok(),
    "Generation failed for untyped parameter: {:?}",
    result.err()
  );

  let output = result.unwrap();
  let code = output.code.code(&GeneratedFileType::Types).unwrap();

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

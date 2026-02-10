use std::collections::HashSet;

use crate::generator::{
  CodegenConfig, TypesMode,
  codegen::{GeneratedFileType, Visibility},
  orchestrator::Orchestrator,
};

#[test]
fn test_implicit_dependency_via_union_fingerprint() {
  let spec_json = include_str!("../../../../fixtures/implicit_union.json");
  let spec: oas3::Spec = oas3::from_json(spec_json).unwrap();

  let mut only_ops = HashSet::new();
  only_ops.insert("test_operation".to_string());

  let orchestrator = Orchestrator::new(
    spec,
    Visibility::default(),
    CodegenConfig::default(),
    Some(&only_ops),
    None,
  );

  let output = orchestrator.generate(&TypesMode, "test.json").unwrap();
  let code = output.code.code(&GeneratedFileType::Types).unwrap();

  assert!(
    code.contains("pub enum ImplicitlyRequiredUnion"),
    "ImplicitlyRequiredUnion was not generated!"
  );

  assert!(code.contains("pub struct ComponentA"), "ComponentA was not generated!");
  assert!(code.contains("pub struct ComponentB"), "ComponentB was not generated!");
}

use super::support::{
  assert_contains, assert_contains_all, assert_not_contains, generate_types, make_orchestrator, parse_spec,
};

const SPEC: &str = include_str!("../../../fixtures/path_level_params.json");

#[test]
fn path_level_param_oneof_schema_is_emitted() {
  let orchestrator = make_orchestrator(parse_spec(SPEC), false);
  let output = generate_types(&orchestrator, "test.json");

  assert_contains_all(
    &output.code,
    &[
      ("pub enum OfficeIdUnion", "oneOf union type should be emitted"),
      ("ForecastOffice(ForecastOfficeId)", "first variant should reference ForecastOfficeId"),
      ("RegionalHQ(RegionalHQId)", "second variant should reference RegionalHQId"),
      ("NationalHQ(NationalHQId)", "third variant should reference NationalHQId"),
    ],
  );
}

#[test]
fn path_level_param_transitive_deps_are_reachable() {
  let orchestrator = make_orchestrator(parse_spec(SPEC), false);
  let output = generate_types(&orchestrator, "test.json");

  assert_contains_all(
    &output.code,
    &[
      ("pub enum ForecastOfficeId", "leaf enum ForecastOfficeId should be emitted"),
      ("pub enum RegionalHQId", "leaf enum RegionalHQId should be emitted"),
      ("pub enum NationalHQId", "leaf enum NationalHQId should be emitted"),
    ],
  );
}

#[test]
fn path_level_param_generates_typed_field() {
  let orchestrator = make_orchestrator(parse_spec(SPEC), false);
  let output = generate_types(&orchestrator, "test.json");

  assert_contains(
    &output.code,
    "office_id: Box<OfficeIdUnion>",
    "request path struct should have typed office_id field",
  );
}

#[test]
fn operation_level_params_still_reachable() {
  let orchestrator = make_orchestrator(parse_spec(SPEC), false);
  let output = generate_types(&orchestrator, "test.json");

  assert_contains(
    &output.code,
    "pub enum ItemCategory",
    "operation-level param schema should be emitted",
  );
}

#[test]
fn unreferenced_schemas_excluded_without_all_schemas() {
  let orchestrator = make_orchestrator(parse_spec(SPEC), false);
  let output = generate_types(&orchestrator, "test.json");

  assert_not_contains(
    &output.code,
    "UnreferencedSchema",
    "unreferenced schema should be excluded when not using --all-schemas",
  );
}

#[test]
fn all_schemas_mode_includes_everything() {
  let orchestrator = make_orchestrator(parse_spec(SPEC), true);
  let output = generate_types(&orchestrator, "test.json");

  assert_contains_all(
    &output.code,
    &[
      ("pub enum OfficeIdUnion", "oneOf union should be emitted in all-schemas mode"),
      ("pub enum ForecastOfficeId", "ForecastOfficeId should be emitted"),
      ("pub enum ItemCategory", "ItemCategory should be emitted"),
      ("UnreferencedSchema", "unreferenced schema should be included in all-schemas mode"),
    ],
  );
}

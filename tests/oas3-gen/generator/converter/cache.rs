use rstest::rstest;

use super::convert;
use crate::{
  assert_contains_all, assert_not_contains, generate_types, make_orchestrator_with_ops, parse_spec, string_set,
};

#[rstest]
fn inline_schemas_with_one_shape_share_one_type() {
  let converted = convert(spec!("converter/shared_shapes.json"), config!());

  assert_eq!(
    converted.fields("First"),
    [
      "meta: Option<Meta>",
      "mood: Option<Pembroke>",
      "level: Option<Level>",
      "bounds: Option<FirstBounds>",
    ]
  );
  assert_eq!(
    converted.fields("Second"),
    ["meta: Option<Meta>", "mood: Option<Pembroke>", "level: Option<Level>"]
  );
  assert_eq!(
    converted.declarations(),
    [
      "enum Pembroke",
      "enum PembrokeOrAny",
      "struct First",
      "struct Second",
      "struct Meta",
      "enum Level",
      "struct FirstBounds",
    ]
  );
  assert!(converted.warnings.is_empty(), "{:?}", converted.warnings);
}

#[rstest]
fn union_reached_only_through_a_matching_inline_union_is_generated() {
  let orchestrator = make_orchestrator_with_ops(
    parse_spec(spec!("converter/implicit_union.json")),
    false,
    Some(&string_set(&["test_operation"])),
    None,
  );
  let generated = generate_types(&orchestrator, "implicit_union.json");

  assert_contains_all(
    &generated.code,
    &[
      ("pub enum ImplicitlyRequiredUnion", "union matching the request body"),
      (
        "pub body: Option<ImplicitlyRequiredUnion>",
        "request body reusing the union",
      ),
      ("pub struct ComponentA", "first union member"),
      ("pub struct ComponentB", "second union member"),
    ],
  );
  assert_not_contains(&generated.code, "Unrelated", "schema of a filtered-out operation");
}

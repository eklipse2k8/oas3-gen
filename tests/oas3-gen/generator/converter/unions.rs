use oas3_gen::generator::{
  EnumLayoutPolicy,
  ast::{EnumDefault, RustType},
};
use proptest::prelude::*;
use rstest::rstest;
use serde_json::{Map, Value, json};

use super::{Converted, convert, enum_signature};

#[rstest]
#[case::sorted_layout_still_tries_plain_string_last(
  "StringAfterSorted",
  EnumLayoutPolicy::Sorted,
  "StringAfterSorted { Chat(Chat), Zeta(Zeta), String(String) }",
  EnumDefault::Derive
)]
#[case::inline_union_default_avoids_a_cycle_through_its_parent(
  "BranchChild",
  EnumLayoutPolicy::Spec,
  "BranchChild { Node(Box<Node>), #[default] String(String) }",
  EnumDefault::Derive
)]
#[case::titled_string_enum_branch_becomes_its_own_enum(
  "ModeOrConfig",
  EnumLayoutPolicy::Spec,
  "ModeOrConfig { Mode(ModeOrConfigMode), Cfg(Cfg) }",
  EnumDefault::Derive
)]
#[case::single_value_enum_branch_stays_a_string(
  "AutoOrConfig",
  EnumLayoutPolicy::Spec,
  "AutoOrConfig { Cfg(Cfg), #[default] Enum(String) }",
  EnumDefault::Derive
)]
#[case::null_default_leaves_a_nullable_union_without_default(
  "NullDefault",
  EnumLayoutPolicy::Spec,
  "NullDefault { Integer(i64), #[default] String(String) }",
  EnumDefault::None
)]
#[case::unmatched_default_derives_the_first_declared_branch(
  "UnmatchedDefault",
  EnumLayoutPolicy::Spec,
  "UnmatchedDefault { Integer(i64), #[default] String(String) }",
  EnumDefault::Derive
)]
fn union_variant_order_and_default(
  #[case] union: &str,
  #[case] layout: EnumLayoutPolicy,
  #[case] expected_signature: &str,
  #[case] expected_mode: EnumDefault,
) {
  let converted = convert(spec!("converter/unions.json"), config!(enum_layout: layout));
  let def = converted.enum_def(union);
  assert_eq!(enum_signature(def), expected_signature);
  assert_eq!(def.default_mode, expected_mode);
}

#[rstest]
fn only_any_of_with_a_freeform_string_is_open() {
  let converted = convert(spec!("converter/unions.json"), config!());
  let open = converted
    .types
    .iter()
    .filter_map(|rust_type| match rust_type {
      RustType::Enum(def) if def.non_exhaustive => Some(enum_signature(def)),
      _ => None,
    })
    .collect::<Vec<_>>();
  assert_eq!(
    open,
    [
      "OpenConsts { Known1, Known2, Other(String) }",
      "OpenHyphenated { KnownValue, Other(String) }",
      "OpenNested { Known1, Known2, Other(String) }",
      "OpenListedDefault { Italian, #[default] Thai, Other(String) }",
      "OpenNumberDefault { Italian, Thai, Other(String) }",
    ]
  );
}

fn spec_in_order(status: &[&str], shapes: &[&str]) -> String {
  let mut spec = serde_json::from_str::<Value>(spec!("converter/enum_shapes.json")).expect("spec is JSON");
  let schemas = &mut spec["components"]["schemas"];
  schemas["Status"]["enum"] = json!(status);
  let refs = shapes
    .iter()
    .map(|shape| json!({ "$ref": format!("#/components/schemas/{shape}") }))
    .collect::<Vec<_>>();
  schemas["Pick"]["oneOf"] = json!(refs);
  schemas["Fruit"]["oneOf"] = json!(refs);
  schemas["Fruit"]["discriminator"]["mapping"] = shapes
    .iter()
    .map(|shape| (shape.to_lowercase(), json!(format!("#/components/schemas/{shape}"))))
    .collect::<Map<_, _>>()
    .into();
  spec.to_string()
}

fn variant_orders(converted: &Converted) -> [Vec<String>; 3] {
  let names = |def| {
    converted
      .enum_def(def)
      .variants
      .iter()
      .map(|variant| variant.name.to_string())
      .collect()
  };
  let tags = converted
    .discriminated_enum("Fruit")
    .variants
    .iter()
    .map(|variant| variant.variant_name.to_string())
    .collect();
  [names("Status"), names("Pick"), tags]
}

fn capitalized(values: &[&str]) -> Vec<String> {
  values
    .iter()
    .map(|value| value[..1].to_uppercase() + &value[1..])
    .collect()
}

proptest! {
  #![proptest_config(ProptestConfig::with_cases(32))]

  #[test]
  fn sorted_layout_ignores_declaration_order_and_spec_layout_keeps_it(
    status in Just(vec!["zeta", "alpha", "gamma", "beta"]).prop_shuffle(),
    shapes in Just(vec!["Zebra", "Apple", "Mango"]).prop_shuffle(),
  ) {
    let spec = spec_in_order(&status, &shapes);
    let sorted = convert(&spec, config!(enum_layout: EnumLayoutPolicy::Sorted));
    let declared = convert(&spec, config!());

    let alphabetical_shapes = capitalized(&["Apple", "Mango", "Zebra"]);
    prop_assert_eq!(
      variant_orders(&sorted),
      [capitalized(&["alpha", "beta", "gamma", "zeta"]), alphabetical_shapes.clone(), alphabetical_shapes]
    );
    prop_assert_eq!(variant_orders(&declared), [capitalized(&status), capitalized(&shapes), capitalized(&shapes)]);
  }
}

use std::collections::HashSet;

use oas3_gen::generator::{
  EnumCasePolicy,
  ast::{Documentation, RustPrimitive, SerdeAttribute, VariantDef},
};
use proptest::prelude::*;
use rstest::rstest;
use serde_json::{Value, json};

use super::{convert, enum_signature};

fn wire_values(variant: &VariantDef) -> Vec<&str> {
  std::iter::once(variant.serde_name())
    .chain(variant.serde_attrs.iter().filter_map(|attr| match attr {
      SerdeAttribute::Alias(alias) => Some(alias.as_str()),
      _ => None,
    }))
    .collect()
}

#[rstest]
#[case::deduplicate_folds_spellings_into_aliases(
  "CaseFold",
  EnumCasePolicy::Deduplicate,
  &[("Item", &["ITEM", "item"][..]), ("Select", &["SELECT", "select"])],
)]
#[case::preserve_suffixes_collisions_with_their_position(
  "Collisions",
  EnumCasePolicy::Preserve,
  &[
    ("Active", &["Active"][..]),
    ("Active1", &["active"]),
    ("InProgress", &["in_progress"]),
    ("Active3", &["ACTIVE"]),
    ("InProgress4", &["InProgress"]),
  ],
)]
#[case::preserve_skips_suffixes_already_taken(
  "Taken",
  EnumCasePolicy::Preserve,
  &[("A3", &["A3"][..]), ("A32", &["A32"]), ("A", &["a"]), ("A33", &["A"])],
)]
#[case::negative_numbers_keep_their_sign(
  "Signed",
  EnumCasePolicy::Deduplicate,
  &[("Value0", &["0"][..]), ("Value1", &["1"]), ("Value_5", &["-5"])],
)]
fn variant_names_resolve_collisions(
  #[case] schema: &str,
  #[case] policy: EnumCasePolicy,
  #[case] expected: &[(&str, &[&str])],
) {
  let converted = convert(spec!("converter/value_enums.json"), config!(enum_case: policy));
  let variants = converted
    .enum_def(schema)
    .variants
    .iter()
    .map(|variant| (variant.name.as_str(), wire_values(variant)))
    .collect::<Vec<_>>();
  let expected = expected
    .iter()
    .map(|(name, wire)| (*name, wire.to_vec()))
    .collect::<Vec<_>>();
  assert_eq!(variants, expected);
}

#[rstest]
fn value_enums_carry_wire_repr_default_and_docs() {
  let converted = convert(spec!("converter/value_enums.json"), config!());
  let names = [
    "Int32",
    "NullableInt",
    "NumbersAsStrings",
    "MergedDefault",
    "UnlistedDefault",
    "MixedConsts",
    "NullableConsts",
  ];

  assert_eq!(
    names.map(|name| enum_signature(converted.enum_def(name))),
    [
      "Int32 { Value1, Value2 }",
      "NullableInt { Value1, Value2 }",
      "NumbersAsStrings { Value1, Value2 }",
      "MergedDefault { #[default] FooBar }",
      "UnlistedDefault { Active, Inactive }",
      "MixedConsts { Auto, Value1, True }",
      "NullableConsts { A, B }",
    ]
  );
  assert_eq!(
    names.map(|name| converted.enum_def(name).scalar_repr.clone()),
    [
      Some(RustPrimitive::I32),
      Some(RustPrimitive::I64),
      None,
      None,
      None,
      Some(RustPrimitive::Value),
      None,
    ]
  );
  assert_eq!(
    converted
      .enum_def("PartlyDocumented")
      .variants
      .iter()
      .map(|variant| variant.docs.clone())
      .collect::<Vec<_>>(),
    [
      Documentation::from_lines(["first only"]),
      Documentation::default(),
      Documentation::default()
    ]
  );
}

fn distinct_values() -> impl Strategy<Value = Vec<String>> {
  prop::collection::vec("[aAbB1 _-]{0,3}", 1..8).prop_map(|values| {
    let mut seen = HashSet::new();
    values.into_iter().filter(|value| seen.insert(value.clone())).collect()
  })
}

fn spec_with_status_values(values: &[String]) -> String {
  let mut spec = serde_json::from_str::<Value>(spec!("converter/enum_shapes.json")).expect("spec is JSON");
  spec["components"]["schemas"]["Status"]["enum"] = json!(values);
  spec.to_string()
}

fn case_policy() -> impl Strategy<Value = EnumCasePolicy> {
  prop_oneof![Just(EnumCasePolicy::Deduplicate), Just(EnumCasePolicy::Preserve)]
}

proptest! {
  #![proptest_config(ProptestConfig::with_cases(64))]

  #[test]
  fn variant_names_stay_unique_and_keep_every_value(values in distinct_values(), policy in case_policy()) {
    let converted = convert(&spec_with_status_values(&values), config!(enum_case: policy));
    let variants = &converted.enum_def("Status").variants;

    let names = variants.iter().map(|variant| variant.name.as_str()).collect::<HashSet<_>>();
    prop_assert_eq!(names.len(), variants.len(), "variant names collide");
    for name in names {
      prop_assert!(syn::parse_str::<syn::Ident>(name).is_ok(), "{} is not an identifier", name);
    }

    let mut wire = variants.iter().flat_map(wire_values).collect::<Vec<_>>();
    if policy == EnumCasePolicy::Preserve {
      prop_assert_eq!(&wire, &values, "preserved variants serialize as declared");
    }
    wire.sort_unstable();
    let mut declared = values.iter().map(String::as_str).collect::<Vec<_>>();
    declared.sort_unstable();
    prop_assert_eq!(wire, declared, "each declared value deserializes through exactly one variant");
  }
}

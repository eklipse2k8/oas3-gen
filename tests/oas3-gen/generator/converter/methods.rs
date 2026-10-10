use oas3_gen::generator::{
  EnumHelperPolicy,
  ast::{EnumMethodKind, RustType},
};
use rstest::rstest;

use super::convert;

#[rstest]
#[case::generated_for_variants_with_at_most_one_settable_field(
  EnumHelperPolicy::Generate,
  &[
    "Shapes::simple(opt_field: Option<String>)",
    "Shapes::single_param(req_field: String)",
    "ResponseFormat::text(dummy: Option<String>)",
    "Corgi::pembroke(waddle: String)",
  ][..],
)]
#[case::disabled_by_policy(EnumHelperPolicy::Disable, &[])]
fn union_helper_constructors(#[case] policy: EnumHelperPolicy, #[case] expected: &[&str]) {
  let converted = convert(spec!("converter/methods.json"), config!(enum_helpers: policy));
  let constructors = converted
    .types
    .iter()
    .flat_map(|rust_type| match rust_type {
      RustType::Enum(def) => def.methods.iter().map(|method| (def.name.as_str(), method)).collect(),
      RustType::DiscriminatedEnum(def) => def.methods.iter().map(|method| (def.name.as_str(), method)).collect(),
      _ => vec![],
    })
    .filter_map(|(owner, method)| match &method.kind {
      EnumMethodKind::ParameterizedConstructor {
        param_name, param_type, ..
      } => Some(format!(
        "{owner}::{}({param_name}: {})",
        method.name,
        param_type.to_rust_type()
      )),
      _ => None,
    })
    .collect::<Vec<_>>();
  assert_eq!(constructors, expected);
}

use std::{collections::HashMap, path::Path};

use oas3_gen::ui::{
  GenerateMode,
  commands::generate::{parse_customizations, parse_fn_name_overrides, resolve_crate_package},
};
use proptest::prelude::*;
use rstest::rstest;

proptest! {
  #![proptest_config(ProptestConfig::with_cases(64))]

  #[test]
  fn key_value_flags_split_at_the_first_equals(
    entries in prop::option::of(prop::collection::btree_map("[a-z_][a-zA-Z0-9_]{0,12}", "[ -~]{0,24}", 0..4)),
    bare in "[a-zA-Z_:]{1,16}",
  ) {
    let expected = entries.clone().unwrap_or_default().into_iter().collect::<HashMap<_, _>>();
    let input = entries.map(|entries| entries.iter().map(|(key, value)| format!("{key}={value}")).collect::<Vec<_>>());
    prop_assert_eq!(parse_customizations(input.clone()).ok(), Some(expected.clone()));
    prop_assert_eq!(parse_fn_name_overrides(input).ok(), Some(expected));

    let customize_error = parse_customizations(Some(vec![bare.clone()])).err().map(|error| error.to_string());
    prop_assert!(
      customize_error.as_ref().is_some_and(|error| error.contains("Invalid customize format")),
      "{:?}", customize_error
    );
    let fn_name_error = parse_fn_name_overrides(Some(vec![bare])).err().map(|error| error.to_string());
    prop_assert!(
      fn_name_error.as_ref().is_some_and(|error| error.contains("Invalid fn-name format")),
      "{:?}", fn_name_error
    );
  }
}

#[rstest]
#[case::module_modes_name_the_crate_after_the_directory(&[GenerateMode::ClientMod, GenerateMode::ServerMod], Some("petstore-api"))]
#[case::single_file_modes_reject_the_flag(&[GenerateMode::Types, GenerateMode::Client], None)]
fn workspace_flag_packages_only_module_modes(
  #[case] modes: &[GenerateMode],
  #[case] crate_name: Option<&str>,
) -> anyhow::Result<()> {
  let output = Path::new("output/Petstore API");
  for mode in modes {
    assert_eq!(
      resolve_crate_package(mode, output, false, "1.2.3".to_string())?,
      None,
      "{mode:?} without --workspace"
    );
    match (
      resolve_crate_package(mode, output, true, "1.2.3".to_string()),
      crate_name,
    ) {
      (Ok(Some(package)), Some(name)) => assert_eq!((package.name(), package.version()), (name, "1.2.3")),
      (Err(error), None) => assert!(
        error.to_string().contains("client-mod or server-mod"),
        "{mode:?}: unexpected error {error}"
      ),
      (outcome, _) => panic!("{mode:?} with --workspace: unexpected {outcome:?}"),
    }
  }
  Ok(())
}

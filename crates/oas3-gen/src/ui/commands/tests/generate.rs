use std::path::PathBuf;

use crate::ui::{
  GenerateMode,
  commands::generate::{parse_customizations, resolve_crate_package},
};

#[test]
fn test_parse_customizations_none() {
  let result = parse_customizations(None).unwrap();
  assert!(result.is_empty());
}

#[test]
fn test_parse_customizations_empty_vec() {
  let result = parse_customizations(Some(vec![])).unwrap();
  assert!(result.is_empty());
}

#[test]
fn test_parse_customizations_single_entry() {
  let result = parse_customizations(Some(vec!["date_time=crate::MyDateTime".to_string()])).unwrap();
  assert_eq!(result.len(), 1);
  assert_eq!(result.get("date_time"), Some(&"crate::MyDateTime".to_string()));
}

#[test]
fn test_parse_customizations_multiple_entries() {
  let result = parse_customizations(Some(vec![
    "date_time=crate::MyDateTime".to_string(),
    "date=crate::MyDate".to_string(),
    "uuid=crate::MyUuid".to_string(),
  ]))
  .unwrap();

  assert_eq!(result.len(), 3);
  assert_eq!(result.get("date_time"), Some(&"crate::MyDateTime".to_string()));
  assert_eq!(result.get("date"), Some(&"crate::MyDate".to_string()));
  assert_eq!(result.get("uuid"), Some(&"crate::MyUuid".to_string()));
}

#[test]
fn test_parse_customizations_with_module_path() {
  let result = parse_customizations(Some(vec!["date_time=my_crate::types::custom::IsoDateTime".to_string()])).unwrap();
  assert_eq!(
    result.get("date_time"),
    Some(&"my_crate::types::custom::IsoDateTime".to_string())
  );
}

#[test]
fn test_parse_customizations_invalid_format_no_equals() {
  let result = parse_customizations(Some(vec!["date_time".to_string()]));
  assert!(result.is_err());
  let err = result.unwrap_err();
  assert!(err.to_string().contains("Invalid customize format"));
}

#[test]
fn test_parse_customizations_with_equals_in_value() {
  let result = parse_customizations(Some(vec!["date_time=crate::Type=Something".to_string()])).unwrap();
  assert_eq!(result.get("date_time"), Some(&"crate::Type=Something".to_string()));
}

#[test]
fn test_resolve_crate_package_disabled() {
  let modes = [
    GenerateMode::Types,
    GenerateMode::Client,
    GenerateMode::ClientMod,
    GenerateMode::ServerMod,
  ];

  for mode in modes {
    let result = resolve_crate_package(&mode, &PathBuf::from("output/api"), false, "0.0.0".to_string()).unwrap();
    assert!(result.is_none(), "no package expected for {mode:?} without --workspace");
  }
}

#[test]
fn test_resolve_crate_package_for_module_modes() {
  let modes = [GenerateMode::ClientMod, GenerateMode::ServerMod];

  for mode in modes {
    let package = resolve_crate_package(&mode, &PathBuf::from("output/Petstore API"), true, "1.2.3".to_string())
      .unwrap()
      .unwrap_or_else(|| panic!("{mode:?} should produce a package"));
    assert_eq!(package.name(), "petstore-api", "unexpected package name for {mode:?}");
    assert_eq!(package.version(), "1.2.3", "unexpected package version for {mode:?}");
  }
}

#[test]
fn test_resolve_crate_package_rejects_single_file_modes() {
  let modes = [GenerateMode::Types, GenerateMode::Client];

  for mode in modes {
    let error = resolve_crate_package(&mode, &PathBuf::from("output.rs"), true, "0.0.0".to_string())
      .expect_err(&format!("{mode:?} should reject --workspace"));
    assert!(
      error.to_string().contains("client-mod or server-mod"),
      "unexpected error for {mode:?}: {error}"
    );
  }
}

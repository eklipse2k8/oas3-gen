use std::{path::Path, sync::LazyLock};

use oas3_gen::generator::{
  ClientModMode, CodegenConfig, CollectionTypePolicy,
  codegen::{
    GeneratedFileType, Visibility,
    cargo_manifest::{CargoManifest, CratePackage},
    mod_file::ModFileKind,
  },
  orchestrator::Orchestrator,
};
use proptest::prelude::*;
use regex::Regex;
use rstest::rstest;
use toml::{Table, Value};

use crate::parse_spec;

const WORKSPACE_MANIFEST: &str = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/../Cargo.toml"));

static CARGO_PACKAGE_NAME: LazyLock<Regex> =
  LazyLock::new(|| Regex::new("^[a-z][a-z0-9]*(-[a-z0-9]+)*$").expect("package name pattern compiles"));

fn render(kind: ModFileKind, title: &str, policy: CollectionTypePolicy, sources: &[&str]) -> anyhow::Result<Table> {
  let package = CratePackage::new("demo-api", "3.2.1".to_string())?;
  let manifest = CargoManifest::new(package, kind, title, "1.2.3".to_string(), policy, sources)?.render()?;
  Ok(manifest.parse::<Table>()?)
}

fn version(dependency: &Value) -> Option<&str> {
  dependency
    .as_str()
    .or_else(|| dependency.get("version").and_then(Value::as_str))
}

fn features(dependency: &Value) -> Vec<&str> {
  dependency
    .get("features")
    .and_then(Value::as_array)
    .map(|features| features.iter().filter_map(Value::as_str).collect())
    .unwrap_or_default()
}

#[rstest]
fn manifest_pins_match_the_workspace_manifest() -> anyhow::Result<()> {
  let workspace = WORKSPACE_MANIFEST.parse::<Table>()?;
  let declared = &workspace["workspace"]["dependencies"];
  let Some(declared_names) = declared.as_table() else {
    anyhow::bail!("workspace manifest declares no dependency table");
  };
  let every_crate = declared_names
    .keys()
    .map(|name| format!("{}::Item", name.replace('-', "_")))
    .collect::<Vec<_>>()
    .join(" ");

  let manifest = render(
    ModFileKind::Client,
    "Demo",
    CollectionTypePolicy::Ordered,
    &[&every_crate],
  )?;
  for key in ["edition", "rust-version"] {
    assert_eq!(
      manifest["package"][key], workspace["workspace"]["package"][key],
      "generated `{key}` drifted from the workspace"
    );
  }

  let Some(dependencies) = manifest["dependencies"].as_table() else {
    anyhow::bail!("generated manifest has no dependency table");
  };
  assert!(
    dependencies.len() > 10,
    "expected the full dependency set: {dependencies:?}"
  );
  for (name, dependency) in dependencies {
    let pinned = declared
      .get(name)
      .and_then(version)
      .map(|pinned| pinned.trim_start_matches(['>', '=', '^', '~', ' ']));
    assert_eq!(version(dependency), pinned, "`{name}` drifted from the workspace");
  }
  Ok(())
}

#[rstest]
fn crate_package_derives_its_name_from_the_resolved_output_directory() -> anyhow::Result<()> {
  let package = CratePackage::from_output_dir(Path::new("generated/Café PetStore/api/.."), "0.0.0".to_string())?;
  assert_eq!(package.name(), "cafe-pet-store");
  Ok(())
}

#[rstest]
#[case::indexmap_and_ordered_collections(
  CollectionTypePolicy::Ordered,
  "use serde::{Deserialize, Serialize}; #[derive(validator::Validate)] #[serde_with::serde_as] \
   pub struct Pet { pub http: String, pub axum: bool, pub tags: indexmap::IndexSet<String>, pub extra: serde_json::Value }",
  &["indexmap", "serde", "serde_json", "serde_with", "validator"],
  &[("serde_json", "preserve_order"), ("serde_with", "indexmap_2"), ("validator", "indexmap")],
  &[],
)]
#[case::hashed_collections_without_indexmap(
  CollectionTypePolicy::Hashed,
  "#[derive(validator::Validate)] #[serde_with::serde_as] pub struct Pet { pub extra: serde_json::Value }",
  &["serde_json", "serde_with", "validator"],
  &[],
  &[("serde_json", "preserve_order"), ("serde_with", "indexmap_2"), ("validator", "indexmap")],
)]
fn manifest_depends_only_on_referenced_crates(
  #[case] policy: CollectionTypePolicy,
  #[case] source: &str,
  #[case] expected: &[&str],
  #[case] enabled: &[(&str, &str)],
  #[case] disabled: &[(&str, &str)],
) -> anyhow::Result<()> {
  let manifest = render(ModFileKind::Client, "Demo", policy, &[source])?;
  let Some(dependencies) = manifest["dependencies"].as_table() else {
    anyhow::bail!("generated manifest has no dependency table");
  };
  assert_eq!(dependencies.keys().collect::<Vec<_>>(), expected);
  for (name, feature) in enabled {
    assert!(
      features(&dependencies[*name]).contains(feature),
      "`{name}` should enable `{feature}`"
    );
  }
  for (name, feature) in disabled {
    assert!(
      !features(&dependencies[*name]).contains(feature),
      "`{name}` should not enable `{feature}`"
    );
  }
  Ok(())
}

#[rstest]
fn manifest_is_emitted_only_for_a_package() -> anyhow::Result<()> {
  let orchestrator = Orchestrator::new(
    parse_spec(spec!("codegen/pets.json")),
    Visibility::Public,
    CodegenConfig::default(),
    None,
    None,
  );
  let without_package = orchestrator.generate(&ClientModMode::default(), "pets.json")?;
  assert_eq!(without_package.code.code(&GeneratedFileType::Manifest), None);

  let package = CratePackage::new("pets-client", "0.0.0".to_string())?;
  let with_package = orchestrator.generate(&ClientModMode::with_package(Some(package)), "pets.json")?;
  let manifest = with_package
    .code
    .code(&GeneratedFileType::Manifest)
    .map(|manifest| manifest.parse::<Table>())
    .transpose()?;
  assert_eq!(
    manifest.and_then(|manifest| manifest["package"]["name"].as_str().map(str::to_string)),
    Some("pets-client".to_string())
  );
  Ok(())
}

proptest! {
  #![proptest_config(ProptestConfig::with_cases(256))]

  #[test]
  fn crate_package_names_are_valid_cargo_names(
    raw in "[0-9A-Za-z _.-]{0,16}|\\PC{0,16}",
    words in prop::collection::vec("[a-z]{1,8}", 1..4),
  ) {
    if let Ok(package) = CratePackage::new(&raw, "0.0.0".to_string()) {
      prop_assert!(CARGO_PACKAGE_NAME.is_match(package.name()), "{raw:?} became {:?}", package.name());
    }
    let package = CratePackage::new(&words.join(" "), "0.0.0".to_string());
    prop_assert_eq!(package.ok().map(|package| package.name().to_string()), Some(words.join("-")));
  }

  #[test]
  fn manifest_description_round_trips_any_title(
    title in any::<String>(),
    kind in prop_oneof![Just(ModFileKind::Client), Just(ModFileKind::Server)],
  ) {
    let manifest = render(kind, &title, CollectionTypePolicy::Ordered, &["pub struct Pet;"])
      .map_err(|error| TestCaseError::fail(format!("{error:#}")))?;
    let normalized = title.split_whitespace().collect::<Vec<_>>().join(" ");
    let expected = format!("Rust {} generated from the {normalized} OpenAPI document", kind.label());
    prop_assert_eq!(manifest["package"]["description"].as_str(), Some(expected.as_str()));
    prop_assert_eq!(manifest["package"]["name"].as_str(), Some("demo-api"));
    prop_assert_eq!(manifest["package"]["version"].as_str(), Some("3.2.1"));
  }
}

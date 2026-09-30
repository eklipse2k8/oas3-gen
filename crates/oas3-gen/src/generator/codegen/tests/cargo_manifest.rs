use std::{collections::HashMap, path::Path};

use crate::generator::{
  ClientModMode, ServerModMode,
  codegen::{
    GeneratedFileType, Visibility,
    cargo_manifest::{CargoManifest, CratePackage, pinned_versions},
    mod_file::ModFileKind,
  },
  converter::{CodegenConfig, CollectionTypePolicy},
  orchestrator::Orchestrator,
};

const WORKSPACE_MANIFEST: &str = include_str!("../../../../../../Cargo.toml");

const MINIMAL_SPEC: &str = r##"{
  "openapi": "3.1.0",
  "info": { "title": "Demo API", "version": "2.1.0" },
  "paths": {
    "/pets": {
      "get": {
        "operationId": "listPets",
        "responses": {
          "200": {
            "description": "ok",
            "content": { "application/json": { "schema": { "$ref": "#/components/schemas/Pet" } } }
          }
        }
      }
    }
  },
  "components": {
    "schemas": {
      "Pet": {
        "type": "object",
        "required": ["name"],
        "properties": { "name": { "type": "string" } }
      }
    }
  }
}"##;

fn render_manifest(sources: &[&str], collection_types: CollectionTypePolicy) -> String {
  CargoManifest::new(
    CratePackage::new("demo-api", "0.0.0".to_string()).expect("valid package name"),
    ModFileKind::Client,
    "Demo API",
    "1.2.3".to_string(),
    collection_types,
    sources,
  )
  .expect("manifest generation should succeed")
  .render()
  .expect("manifest rendering should succeed")
}

#[test]
fn test_crate_package_sanitizes_names() {
  let cases = [
    ("petstore-api", "petstore-api"),
    ("PetStore", "pet-store"),
    ("my api v2", "my-api-v2"),
    ("event_stream", "event-stream"),
    ("--weird--name--", "weird-name"),
    ("café api", "cafe-api"),
  ];

  for (raw, expected) in cases {
    let package =
      CratePackage::new(raw, "0.0.0".to_string()).unwrap_or_else(|error| panic!("'{raw}' should sanitize: {error}"));
    assert_eq!(package.name(), expected, "unexpected package name for '{raw}'");
  }
}

#[test]
fn test_crate_package_rejects_invalid_names() {
  let cases = ["", "   ", "---", "9lives"];

  for raw in cases {
    assert!(
      CratePackage::new(raw, "0.0.0".to_string()).is_err(),
      "'{raw}' should be rejected"
    );
  }
}

#[test]
fn test_crate_package_uses_final_path_component() {
  let cases = [
    ("output/petstore-api", "petstore-api"),
    ("output/nested/Client Mod/", "client-mod"),
    ("generated/api/..", "generated"),
  ];

  for (path, expected) in cases {
    let package = CratePackage::from_output_dir(Path::new(path), "0.0.0".to_string())
      .unwrap_or_else(|error| panic!("'{path}': {error}"));
    assert_eq!(package.name(), expected, "unexpected package name for '{path}'");
  }
}

#[test]
fn test_dependencies_are_limited_to_referenced_crates() {
  let source = r#"
    use serde::{Deserialize, Serialize};
    use validator::Validate;
    pub const X_API_KEY: http::HeaderName = http::HeaderName::from_static("x-api-key");
    #[derive(Serialize, Deserialize, Validate)]
    pub struct Pet {
      pub name: String,
    }
  "#;

  let manifest = render_manifest(&[source], CollectionTypePolicy::Ordered);
  let expected = [
    "serde = { version = \"1.0\", features = [\"derive\"] }",
    "validator = { version = \"0.21\", features = [\"derive\"] }",
    "http = \"1.5\"",
  ];
  let unexpected = ["reqwest = ", "axum = ", "indexmap = ", "uuid = ", "chrono = ", "bon = "];

  for entry in expected {
    assert!(
      manifest.contains(entry),
      "missing dependency entry '{entry}' in:\n{manifest}"
    );
  }
  for entry in unexpected {
    assert!(
      !manifest.contains(entry),
      "unexpected dependency entry '{entry}' in:\n{manifest}"
    );
  }
}

#[test]
fn test_dependencies_cover_client_and_server_crates() {
  let cases = [
    (
      "let response: reqwest::Response = todo!();",
      "reqwest = { version = \"0.13\"",
    ),
    ("pub fn router() -> axum::Router { todo!() }", "axum = \"0.8\""),
    (
      "pub type Ids = indexmap::IndexSet<String>;",
      "indexmap = { version = \"2.14\"",
    ),
    ("pub struct A { pub id: uuid::Uuid }", "uuid = { version = \"1.26\""),
    (
      "pub struct A { pub at: chrono::NaiveDate }",
      "chrono = { version = \"0.4.42\"",
    ),
    (
      "#[derive(oas3_gen_support::Default)] pub struct A;",
      "oas3-gen-support = ",
    ),
    (
      "static P: std::sync::LazyLock<regex::Regex> = todo!();",
      "regex = \"1.13\"",
    ),
  ];

  for (source, expected) in cases {
    let manifest = render_manifest(&[source], CollectionTypePolicy::Ordered);
    assert!(
      manifest.contains(expected),
      "source '{source}' should require '{expected}' in:\n{manifest}"
    );
  }
}

#[test]
fn test_field_names_do_not_imply_dependencies() {
  let source = "pub struct Request { pub http: String, pub serde: String, pub axum: bool }";
  let manifest = render_manifest(&[source], CollectionTypePolicy::Ordered);

  assert!(
    manifest.trim_end().ends_with("[dependencies]"),
    "single-colon field names should not add dependencies:\n{manifest}"
  );
}

#[test]
fn test_collection_policy_controls_serde_json_features() {
  let source = "pub type Map = indexmap::IndexMap<String, serde_json::Value>;";

  let ordered = render_manifest(&[source], CollectionTypePolicy::Ordered);
  assert!(
    ordered.contains("serde_json = { version = \"1.0\", features = [\"preserve_order\"] }"),
    "ordered collections should preserve JSON key order:\n{ordered}"
  );

  let hashed = render_manifest(&[source], CollectionTypePolicy::Hashed);
  assert!(
    hashed.contains("serde_json = \"1.0\""),
    "hashed collections should drop preserve_order:\n{hashed}"
  );
}

#[test]
fn test_manifest_package_section() {
  let manifest = render_manifest(&["pub struct Pet;"], CollectionTypePolicy::Ordered);
  let expected = [
    "# AUTO-GENERATED CODE - DO NOT EDIT!",
    "# Generated by `oas3-gen v1.2.3`",
    "[package]",
    "name = \"demo-api\"",
    "version = \"0.0.0\"",
    "edition = \"2024\"",
    "rust-version = \"1.89\"",
    "description = \"Rust client generated from the Demo API OpenAPI document\"",
    "[dependencies]",
  ];

  for entry in expected {
    assert!(manifest.contains(entry), "missing '{entry}' in:\n{manifest}");
  }
}

#[test]
fn test_manifest_uses_package_version() {
  let manifest = CargoManifest::new(
    CratePackage::new("demo-api", "3.2.1".to_string()).expect("valid package name"),
    ModFileKind::Client,
    "Demo API",
    "1.2.3".to_string(),
    CollectionTypePolicy::Ordered,
    &["pub struct Pet;"],
  )
  .expect("manifest generation should succeed")
  .render()
  .expect("manifest rendering should succeed");

  assert!(
    manifest.contains("version = \"3.2.1\""),
    "manifest should carry the package version:\n{manifest}"
  );
}

#[test]
fn test_manifest_quotes_awkward_descriptions() {
  let cases = [
    ("Plain API", "Plain API"),
    ("The \"Big\" API", "The \"Big\" API"),
    ("Backslash \\ and 'quoted' API", "Backslash \\ and 'quoted' API"),
    ("Multi\nline\ttitle", "Multi line title"),
    ("  padded   title  ", "padded title"),
  ];

  for (title, normalized) in cases {
    let manifest = CargoManifest::new(
      CratePackage::new("demo", "0.0.0".to_string()).expect("valid package name"),
      ModFileKind::Server,
      title,
      "1.2.3".to_string(),
      CollectionTypePolicy::Ordered,
      &["pub struct Pet;"],
    )
    .expect("manifest generation should succeed")
    .render()
    .expect("manifest rendering should succeed");

    let parsed = manifest
      .parse::<toml::Table>()
      .unwrap_or_else(|error| panic!("title '{title}' should render valid TOML: {error}\n{manifest}"));
    let expected = format!("Rust server generated from the {normalized} OpenAPI document");

    assert_eq!(
      parsed["package"]["description"].as_str(),
      Some(expected.as_str()),
      "description round trip failed for title '{title}'"
    );
  }
}

#[test]
fn test_rendered_manifest_is_valid_toml() {
  let source = r#"
    use serde::{Deserialize, Serialize};
    use validator::Validate;
    pub const X: http::HeaderName = http::HeaderName::from_static("x");
    pub type Map = indexmap::IndexMap<String, serde_json::Value>;
    pub struct Pet { pub id: uuid::Uuid, pub at: chrono::NaiveDate }
    pub async fn call(r: reqwest::Response) -> anyhow::Result<()> { Ok(()) }
  "#;

  let manifest = render_manifest(&[source], CollectionTypePolicy::Ordered);
  let parsed = manifest
    .parse::<toml::Table>()
    .unwrap_or_else(|error| panic!("manifest should be valid TOML: {error}\n{manifest}"));

  let package = parsed["package"].as_table().expect("package table");
  assert_eq!(package["name"].as_str(), Some("demo-api"), "package name");
  assert_eq!(package["edition"].as_str(), Some("2024"), "package edition");

  let dependencies = parsed["dependencies"].as_table().expect("dependencies table");
  assert_eq!(dependencies["anyhow"].as_str(), Some("1.0"), "simple dependency");

  let reqwest = dependencies["reqwest"].as_table().expect("detailed dependency");
  assert_eq!(reqwest["version"].as_str(), Some("0.13"), "detailed version");
  assert_eq!(reqwest["default-features"].as_bool(), Some(false), "default features");
  assert!(
    reqwest["features"]
      .as_array()
      .is_some_and(|features| !features.is_empty()),
    "detailed features"
  );
}

#[test]
fn test_pinned_versions_match_workspace_manifest() {
  let pattern = regex::Regex::new(r#"(?m)^([a-z0-9_-]+)\s*=\s*\{[^}]*version\s*=\s*"([^"]+)""#)
    .expect("workspace dependency pattern should compile");
  let declared = pattern
    .captures_iter(WORKSPACE_MANIFEST)
    .map(|caps| (caps[1].to_string(), caps[2].to_string()))
    .collect::<HashMap<_, _>>();

  for (package, version) in pinned_versions() {
    let workspace_version = declared
      .get(package)
      .unwrap_or_else(|| panic!("'{package}' is not declared in [workspace.dependencies]"));
    let normalized = workspace_version.trim_start_matches(['>', '=', '^', '~', ' ']);
    assert_eq!(
      version, normalized,
      "pinned version for '{package}' drifted from the workspace manifest"
    );
  }
}

#[test]
fn test_module_generation_emits_manifest_only_when_requested() {
  let spec = oas3::from_json(MINIMAL_SPEC).expect("spec should parse");
  let orchestrator = Orchestrator::new(spec, Visibility::Public, CodegenConfig::default(), None, None);

  let without_package = orchestrator
    .generate(&ClientModMode::default(), "demo.json")
    .expect("generation should succeed");
  assert!(
    without_package.code.code(&GeneratedFileType::Manifest).is_none(),
    "no manifest should be emitted without a crate package"
  );

  let package = CratePackage::new("demo-api", "0.0.0".to_string()).expect("valid package name");
  let with_package = orchestrator
    .generate(&ClientModMode::with_package(Some(package)), "demo.json")
    .expect("generation should succeed");
  let manifest = with_package
    .code
    .code(&GeneratedFileType::Manifest)
    .expect("manifest should be emitted");

  assert!(manifest.contains("name = \"demo-api\""), "manifest names the crate");
  assert!(
    manifest.contains("Rust client generated from the Demo API OpenAPI document"),
    "manifest describes the source document"
  );
  assert!(manifest.contains("reqwest = "), "client crates depend on reqwest");
}

#[test]
fn test_server_module_manifest_excludes_reqwest() {
  let spec = oas3::from_json(MINIMAL_SPEC).expect("spec should parse");
  let config = CodegenConfig::builder()
    .target(crate::generator::GenerationTarget::Server)
    .build();
  let orchestrator = Orchestrator::new(spec, Visibility::Public, config, None, None);

  let package = CratePackage::new("demo-server", "0.0.0".to_string()).expect("valid package name");
  let output = orchestrator
    .generate(&ServerModMode::with_package(Some(package)), "demo.json")
    .expect("generation should succeed");
  let manifest = output
    .code
    .code(&GeneratedFileType::Manifest)
    .expect("manifest should be emitted");

  assert!(manifest.contains("axum = "), "server crates depend on axum");
  assert!(!manifest.contains("reqwest = "), "server crates do not call reqwest");
}

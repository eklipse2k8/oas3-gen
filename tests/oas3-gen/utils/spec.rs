use anyhow::Context as _;
use oas3::spec::{ObjectSchema, PathItem};
use oas3_gen::utils::{SchemaInspect, spec::SpecFormat};
use proptest::prelude::*;
use rstest::rstest;
use serde_json::json;

const VERSION: &str = "VERSION";

#[track_caller]
fn parse_alike(json: &str, yaml: &str) -> anyhow::Result<oas3::Spec> {
  let from_json = SpecFormat::Json.parse(json.as_bytes())?;
  let from_yaml = SpecFormat::Yaml.parse(yaml.as_bytes())?;
  assert_eq!(
    serde_json::to_value(&from_json)?,
    serde_json::to_value(&from_yaml)?,
    "the JSON and YAML documents parse to the same spec"
  );
  Ok(from_json)
}

fn inline_schema<'a>(spec: &'a oas3::Spec, name: &str) -> anyhow::Result<&'a ObjectSchema> {
  spec
    .components
    .as_ref()
    .and_then(|components| components.schemas.get(name))
    .and_then(SchemaInspect::as_inline)
    .with_context(|| format!("{name} is an inline schema"))
}

fn operations(items: &oas3::Map<String, PathItem>) -> Vec<String> {
  items
    .iter()
    .flat_map(|(name, item)| {
      let operations = item
        .methods()
        .into_iter()
        .map(|(method, operation)| {
          let security = operation
            .security
            .iter()
            .map(|requirement| requirement.0.keys().map(String::as_str).collect::<Vec<_>>())
            .collect::<Vec<_>>();
          format!(
            "{name} {method} {} {security:?}",
            operation.operation_id.as_deref().unwrap_or("-")
          )
        })
        .collect::<Vec<_>>();
      if operations.is_empty() {
        vec![format!("{name} -> {}", item.reference.as_deref().unwrap_or("-"))]
      } else {
        operations
      }
    })
    .collect()
}

#[rstest]
fn ref_defaults_lift_and_frame_data_decodes_alike_in_json_and_yaml() -> anyhow::Result<()> {
  let spec = parse_alike(spec!("utils/schema_rewrites.json"), spec!("utils/schema_rewrites.yaml"))?;
  let holder = inline_schema(&spec, "Holder")?;
  let property = |schema: &ObjectSchema, name: &str| {
    schema
      .properties
      .get(name)
      .cloned()
      .with_context(|| format!("property {name}"))
  };

  let lifted = property(holder, "lifted")?;
  let lifted = lifted
    .as_inline()
    .context("an enum reference with a default becomes an inline wrapper")?;
  assert_eq!(
    (&lifted.default, lifted.description.as_deref(), lifted.all_of.len()),
    (&Some(json!("a")), Some("kept"), 1)
  );
  let shape = property(holder, "shape")?;
  let shape = shape
    .as_inline()
    .context("a union reference with a default becomes an inline wrapper")?;
  assert_eq!((&shape.default, shape.all_of.len()), (&Some(json!({ "s": 2 })), 1));
  for unchanged in ["struct", "plain"] {
    assert!(
      property(holder, unchanged)?.ref_path().is_some(),
      "{unchanged} stays a reference"
    );
  }
  assert_eq!(
    holder
      .example
      .as_ref()
      .and_then(|example| example.pointer("/properties/x/$ref")),
    Some(&json!("#/components/schemas/Closed")),
    "example data is untouched"
  );

  assert_eq!(
    property(inline_schema(&spec, "Frame")?, "data")?.ref_path(),
    Some("#/components/schemas/Payload"),
    "a frame type's data reads as its contentSchema"
  );
  assert_eq!(
    property(inline_schema(&spec, "Document")?, "data")?.ref_path(),
    None,
    "a JSON body, and a member of a stream union no event keys, keep their string"
  );
  Ok(())
}

#[rstest]
fn path_item_refs_inline_and_keep_operation_security() -> anyhow::Result<()> {
  let spec = parse_alike(spec!("utils/path_items.json"), spec!("utils/path_items.yaml"))?;
  let paths = spec.paths.as_ref().context("paths")?;

  assert_eq!(
    operations(paths),
    [
      "/cleared GET - [[]]",
      "/anonymous GET - [[]]",
      "/inherited GET - []",
      r#"/own GET - [["Other"]]"#,
      "/direct GET readDirect []",
      "/chained GET readDirect []",
      "/chained POST writeAlias []",
      "/missing -> #/components/pathItems/Missing",
      "/looping -> #/components/pathItems/Loop",
    ]
  );
  assert_eq!(operations(&spec.webhooks), ["ping POST ping [[]]"]);
  assert_eq!(
    paths.get("/direct").and_then(|item| item.summary.as_deref()),
    Some("local"),
    "a field set next to the reference wins"
  );
  Ok(())
}

#[rstest]
#[case::null_paths_with_top_level_security(spec!("utils/null_paths.json"), None)]
#[case::type_error(spec!("utils/type_error.json"), Some(" at line "))]
#[case::duplicate_key(spec!("utils/duplicate_key.json"), Some("duplicate field `title`"))]
fn json_documents_parse_or_fail_with_a_located_error(#[case] json: &str, #[case] expected_error: Option<&str>) {
  let error = SpecFormat::Json
    .parse(json.as_bytes())
    .err()
    .map(|error| error.to_string());
  match (error, expected_error) {
    (None, None) => {}
    (Some(error), Some(expected)) => assert!(error.contains(expected), "`{error}` should contain `{expected}`"),
    (error, expected) => panic!("parse error {error:?}, expected {expected:?}"),
  }
}

fn yaml_scalar() -> impl Strategy<Value = String> {
  prop_oneof![
    "[0-9]{1,2}\\.[0-9]{0,2}0",
    "[0-9]{1,3}",
    "[0-9]\\.[0-9]\\.[0-9]",
    "[a-z][a-z0-9]{0,7}",
    Just("~".to_string()),
    Just("true".to_string()),
    Just("1e3".to_string()),
    Just("0x1F".to_string()),
    Just(".inf".to_string()),
  ]
}

fn parse(format: SpecFormat, document: &str) -> Result<oas3::Spec, TestCaseError> {
  format
    .parse(document.as_bytes())
    .map_err(|error| TestCaseError::fail(format!("{format:?}: {error}")))
}

proptest! {
  #![proptest_config(ProptestConfig::with_cases(64))]

  #[test]
  fn yaml_scalars_read_as_written_with_and_without_rewrites(version in yaml_scalar()) {
    let untouched_yaml = spec!("utils/untouched.yaml").replace(VERSION, &version);
    let direct = yaml_serde::from_str::<oas3::Spec>(&untouched_yaml)?;
    prop_assert_eq!(serde_json::to_value(parse(SpecFormat::Yaml, &untouched_yaml)?)?, serde_json::to_value(&direct)?);

    let untouched_json = spec!("utils/untouched.json").replace(&format!("\"{VERSION}\""), &serde_json::to_string(&version)?);
    prop_assert_eq!(
      serde_json::to_value(parse(SpecFormat::Json, &untouched_json)?)?,
      serde_json::to_value(oas3::from_json(&untouched_json)?)?
    );

    let rewritten = parse(SpecFormat::Yaml, &spec!("utils/yaml_scalars.yaml").replace(VERSION, &version))?;
    prop_assert_eq!(&rewritten.info.version, &direct.info.version);
    prop_assert_eq!(rewritten.info.title.as_str(), "true");
    let listed = rewritten.paths.as_ref().and_then(|paths| paths.get("/pets")).and_then(|item| item.get.as_ref());
    prop_assert_eq!(listed.and_then(|operation| operation.operation_id.as_deref()), Some("listPets"));
    let lifted = rewritten
      .components
      .as_ref()
      .and_then(|components| components.schemas.get("Holder"))
      .and_then(SchemaInspect::as_inline)
      .and_then(|holder| holder.properties.get("lifted"))
      .and_then(SchemaInspect::as_inline);
    prop_assert!(lifted.is_some(), "the enum reference default is lifted");
  }
}

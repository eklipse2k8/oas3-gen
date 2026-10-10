use std::collections::BTreeSet;

use oas3_gen::generator::{TypesMode, codegen::GeneratedFileType};
use proptest::{collection::vec, prelude::*};
use rstest::rstest;
use serde_json::{Map, json};

use super::orchestrator::{defined_types, struct_fields};
use crate::{assert_contains_all, assert_not_contains, generate_types, generated_code, make_orchestrator, parse_spec};

const NODES: [&str; 5] = ["Alpha", "Beta", "Gamma", "Delta", "Epsilon"];

fn graph_spec(edges: &[Vec<bool>]) -> serde_json::Result<oas3::Spec> {
  let schemas = edges
    .iter()
    .zip(NODES)
    .map(|(targets, name)| {
      let mut properties = Map::new();
      properties.insert("label".to_string(), json!({ "type": "string" }));
      for (_, target) in targets.iter().zip(NODES).filter(|(linked, _)| **linked) {
        properties.insert(
          target.to_lowercase(),
          json!({ "$ref": format!("#/components/schemas/{target}") }),
        );
      }
      (name.to_string(), json!({ "type": "object", "properties": properties }))
    })
    .collect::<Map<_, _>>();
  super::skeleton_spec(json!({}), json!({ "schemas": schemas }))
}

fn reaches_itself(edges: &[Vec<bool>], start: usize) -> bool {
  let successors = |node: usize| {
    edges[node]
      .iter()
      .enumerate()
      .filter(|(_, linked)| **linked)
      .map(|(target, _)| target)
  };
  let mut visited = vec![false; edges.len()];
  let mut pending = successors(start).collect::<Vec<_>>();
  while let Some(node) = pending.pop() {
    if node == start {
      return true;
    }
    if !std::mem::replace(&mut visited[node], true) {
      pending.extend(successors(node));
    }
  }
  false
}

#[rstest]
fn referenced_only_scope_generates_every_schema_an_operation_reaches() {
  let spec = parse_spec(spec!("generator/reachability.json"));
  let names = spec
    .components
    .iter()
    .flat_map(|components| components.schemas.keys().cloned())
    .collect::<BTreeSet<_>>();
  let output = generate_types(&make_orchestrator(spec, false), "reachability.json");

  let generated = defined_types(&output.code)
    .into_iter()
    .filter(|name| names.contains(*name))
    .collect::<BTreeSet<_>>();
  let expected = names
    .iter()
    .map(String::as_str)
    .filter(|name| !name.starts_with("Orphan"))
    .collect::<BTreeSet<_>>();
  assert_eq!(generated, expected);
  assert_eq!(output.orphaned_schemas_count, 2);
}

#[rstest]
fn all_of_children_inherit_parent_fields_in_order_and_override_redeclared_ones() {
  let spec = parse_spec(spec!("generator/all_of_merge.json"));
  let types = generate_types(&make_orchestrator(spec, true), "all_of_merge.json").code;

  assert_eq!(
    struct_fields(&types, "Composite"),
    [
      "pub corgi_prop: String,",
      "pub shared: Option<i64>,",
      "pub fluff_prop: Option<i64>,",
      "pub own_prop: bool,",
      "pub additional_properties: indexmap::IndexMap<String, i64>,",
    ]
  );
}

#[rstest]
fn discriminators_route_by_explicit_mappings_and_warn_when_consts_cannot_route() -> anyhow::Result<()> {
  let spec = parse_spec(spec!("generator/discriminator_mappings.json"));
  let output = make_orchestrator(spec, true).generate(&TypesMode, "discriminator_mappings.json")?;

  let warnings = output
    .stats
    .warnings
    .iter()
    .map(ToString::to_string)
    .collect::<Vec<_>>();
  assert_eq!(
    warnings,
    [
      "Schema 'Missing': cannot build implicit discriminator mapping: variant 'MissingB' has no string const value for property 'kind'",
      "Schema 'Duplicate': cannot build implicit discriminator mapping: duplicate const value 'same' on variant 'DuplicateB'",
    ]
  );
  let types = generated_code(&output, &GeneratedFileType::Types);
  assert_contains_all(
    &types,
    &[
      (
        "Some(\"a_override\") =>",
        "the explicit mapping value routes the first variant",
      ),
      (
        "Some(\"b_override\") =>",
        "the explicit mapping value routes the second variant",
      ),
      (
        "#[serde(untagged)]\npub enum Missing {",
        "a variant without a const falls back to untagged",
      ),
      (
        "#[serde(untagged)]\npub enum Duplicate {",
        "variants sharing a const fall back to untagged",
      ),
    ],
  );
  assert_not_contains(&types, "Some(\"a\") =>", "a const never overrides an explicit mapping");
  Ok(())
}

proptest! {
  #![proptest_config(ProptestConfig::with_cases(32))]

  #[test]
  fn references_to_schemas_in_a_cycle_are_boxed(
    edges in (1..=NODES.len()).prop_flat_map(|nodes| vec(vec(any::<bool>(), nodes), nodes))
  ) {
    let types = generate_types(&make_orchestrator(graph_spec(&edges)?, true), "graph.json").code;

    for (target, name) in NODES.iter().enumerate().take(edges.len()) {
      if !edges.iter().any(|targets| targets[target]) {
        continue;
      }
      let field = name.to_lowercase();
      let declaration = if reaches_itself(&edges, target) {
        format!("pub {field}: Option<Box<{name}>>,")
      } else {
        format!("pub {field}: Option<{name}>,")
      };
      prop_assert!(types.contains(&declaration), "expected `{}` for edges {:?}", declaration, edges);
    }
  }
}

use std::collections::BTreeSet;

use quote::ToTokens as _;

use crate::generator::codegen::types::{ModuleUsesFragment, UseFragment};

fn set_from<const N: usize>(items: [&str; N]) -> BTreeSet<String> {
  items.into_iter().map(String::from).collect::<BTreeSet<_>>()
}

#[test]
fn empty_set_produces_no_output() {
  let fragment = ModuleUsesFragment::new(BTreeSet::new());
  let code = fragment.into_token_stream().to_string();
  assert!(code.is_empty(), "empty set should produce empty output, got: {code}");
}

#[test]
fn single_path_without_separator_is_skipped() {
  let fragment = ModuleUsesFragment::new(set_from(["standalone"]));
  let code = fragment.into_token_stream().to_string();
  assert!(code.is_empty(), "path without :: should be skipped, got: {code}");
}

#[test]
fn single_item_from_single_module() {
  let fragment = ModuleUsesFragment::new(set_from(["serde::Serialize"]));
  let code = fragment.into_token_stream().to_string();
  assert!(
    code.contains("use serde :: Serialize ;"),
    "single item should produce simple use statement, got: {code}"
  );
  assert!(!code.contains('{'), "single item should not use braces, got: {code}");
}

#[test]
fn multiple_items_from_same_module_grouped() {
  let fragment = ModuleUsesFragment::new(set_from(["serde::Deserialize", "serde::Serialize"]));
  let code = fragment.into_token_stream().to_string();
  assert!(
    code.contains("use serde :: { Deserialize , Serialize } ;"),
    "multiple items from same module should be grouped, got: {code}"
  );
}

#[test]
fn multiple_modules_produce_separate_statements() {
  let fragment = ModuleUsesFragment::new(set_from(["serde::Serialize", "std::collections::BTreeMap"]));
  let code = fragment.into_token_stream().to_string();
  assert!(
    code.contains("use serde :: Serialize ;"),
    "should have serde import, got: {code}"
  );
  assert!(
    code.contains("use std :: collections :: BTreeMap ;"),
    "should have std::collections import, got: {code}"
  );
}

#[test]
fn deeply_nested_module_paths() {
  let fragment = ModuleUsesFragment::new(set_from([
    "std::collections::BTreeMap",
    "std::collections::BTreeSet",
    "std::collections::HashMap",
  ]));
  let code = fragment.into_token_stream().to_string();
  assert!(
    code.contains("use std :: collections :: { BTreeMap , BTreeSet , HashMap } ;"),
    "deeply nested paths should group by full module path, got: {code}"
  );
}

#[test]
fn btreeset_ordering_deterministic() {
  let fragment = ModuleUsesFragment::new(set_from(["serde::Serialize", "anyhow::Result", "quote::ToTokens"]));
  let code = fragment.into_token_stream().to_string();

  let anyhow_pos = code.find("anyhow").expect("should contain anyhow");
  let quote_pos = code.find("quote").expect("should contain quote");
  let serde_pos = code.find("serde").expect("should contain serde");
  assert!(
    anyhow_pos < quote_pos && quote_pos < serde_pos,
    "imports should be in alphabetical order by module: anyhow < quote < serde, got: {code}"
  );
}

#[test]
fn mixed_module_depths() {
  let fragment = ModuleUsesFragment::new(set_from([
    "serde::Deserialize",
    "serde::Serialize",
    "std::collections::BTreeMap",
    "std::io::Write",
  ]));
  let code = fragment.into_token_stream().to_string();

  assert!(
    code.contains("use serde :: { Deserialize , Serialize } ;"),
    "serde items should be grouped, got: {code}"
  );
  assert!(
    code.contains("use std :: collections :: BTreeMap ;"),
    "collections should have single import, got: {code}"
  );
  assert!(
    code.contains("use std :: io :: Write ;"),
    "io should have single import, got: {code}"
  );
}

#[test]
fn use_fragment_single_item_no_braces() {
  let fragment = UseFragment::new("serde".to_string(), vec!["Serialize".to_string()]);
  let code = fragment.into_token_stream().to_string();
  assert!(
    code.contains("use serde :: Serialize ;"),
    "single item should not have braces, got: {code}"
  );
  assert!(!code.contains('{'), "should not contain opening brace, got: {code}");
}

#[test]
fn use_fragment_multiple_items_with_braces() {
  let fragment = UseFragment::new(
    "serde".to_string(),
    vec!["Deserialize".to_string(), "Serialize".to_string()],
  );
  let code = fragment.into_token_stream().to_string();
  assert!(
    code.contains("use serde :: { Deserialize , Serialize } ;"),
    "multiple items should use braces, got: {code}"
  );
}

#[test]
fn use_fragment_invalid_module_path_skipped() {
  let fragment = UseFragment::new("not a valid path".to_string(), vec!["Item".to_string()]);
  let code = fragment.into_token_stream().to_string();
  assert!(
    code.is_empty(),
    "invalid module path should produce no output, got: {code}"
  );
}

#[test]
fn use_fragment_invalid_item_filtered() {
  let fragment = UseFragment::new(
    "serde".to_string(),
    vec!["Serialize".to_string(), "not valid".to_string()],
  );
  let code = fragment.into_token_stream().to_string();
  assert!(
    code.contains("use serde :: Serialize ;"),
    "valid item should still be included, got: {code}"
  );
  assert!(
    !code.contains("not valid"),
    "invalid item should be filtered out, got: {code}"
  );
}

#[test]
fn items_within_module_alphabetically_sorted() {
  let fragment = ModuleUsesFragment::new(set_from([
    "serde::Serialize",
    "serde::Deserialize",
    "serde::de::Visitor",
  ]));
  let code = fragment.into_token_stream().to_string();

  assert!(
    code.contains("use serde :: { Deserialize , Serialize } ;"),
    "serde items should be grouped alphabetically, got: {code}"
  );
  assert!(
    code.contains("use serde :: de :: Visitor ;"),
    "serde::de should be separate module, got: {code}"
  );
}

#[test]
fn same_prefix_different_submodules_not_grouped() {
  let fragment = ModuleUsesFragment::new(set_from([
    "std::collections::BTreeMap",
    "std::io::Read",
    "std::io::Write",
  ]));
  let code = fragment.into_token_stream().to_string();

  assert!(
    code.contains("use std :: collections :: BTreeMap ;"),
    "collections should be single import, got: {code}"
  );
  assert!(
    code.contains("use std :: io :: { Read , Write } ;"),
    "io items should be grouped, got: {code}"
  );
}

#[test]
fn many_items_from_same_module_stress_test() {
  let fragment = ModuleUsesFragment::new(set_from([
    "std::collections::BTreeMap",
    "std::collections::BTreeSet",
    "std::collections::HashMap",
    "std::collections::HashSet",
    "std::collections::LinkedList",
    "std::collections::VecDeque",
  ]));
  let code = fragment.into_token_stream().to_string();

  assert!(
    code.contains("use std :: collections :: { BTreeMap , BTreeSet , HashMap , HashSet , LinkedList , VecDeque } ;"),
    "all collections items should be grouped in order, got: {code}"
  );
}

#[test]
fn interleaved_modules_maintain_separation() {
  let fragment = ModuleUsesFragment::new(set_from(["a::Z", "b::A", "a::A", "b::Z"]));
  let code = fragment.into_token_stream().to_string();

  assert!(
    code.contains("use a :: { A , Z } ;"),
    "module 'a' items should be grouped, got: {code}"
  );
  assert!(
    code.contains("use b :: { A , Z } ;"),
    "module 'b' items should be grouped, got: {code}"
  );

  let a_pos = code.find("use a").expect("should contain 'use a'");
  let b_pos = code.find("use b").expect("should contain 'use b'");
  assert!(a_pos < b_pos, "module 'a' should come before 'b', got: {code}");
}

#[test]
fn real_world_codegen_imports() {
  let fragment = ModuleUsesFragment::new(set_from([
    "proc_macro2::TokenStream",
    "quote::ToTokens",
    "quote::quote",
    "serde::Deserialize",
    "serde::Serialize",
    "std::collections::BTreeMap",
    "std::collections::BTreeSet",
    "validator::Validate",
  ]));
  let code = fragment.into_token_stream().to_string();

  let assertions = [
    ("use proc_macro2 :: TokenStream ;", "proc_macro2 single import"),
    ("use quote :: { ToTokens , quote } ;", "quote grouped imports"),
    ("use serde :: { Deserialize , Serialize } ;", "serde grouped imports"),
    (
      "use std :: collections :: { BTreeMap , BTreeSet } ;",
      "collections grouped imports",
    ),
    ("use validator :: Validate ;", "validator single import"),
  ];

  for (expected, description) in assertions {
    assert!(code.contains(expected), "{description} missing, got: {code}");
  }
}

#[test]
fn crate_relative_paths() {
  let fragment = ModuleUsesFragment::new(set_from([
    "crate::generator::ast::TypeRef",
    "crate::generator::ast::StructDef",
    "crate::utils::SchemaExt",
  ]));
  let code = fragment.into_token_stream().to_string();

  assert!(
    code.contains("use crate :: generator :: ast :: { StructDef , TypeRef } ;"),
    "crate-relative ast imports should be grouped, got: {code}"
  );
  assert!(
    code.contains("use crate :: utils :: SchemaExt ;"),
    "crate-relative utils import should work, got: {code}"
  );
}

#[test]
fn triple_colon_edge_case() {
  let fragment = ModuleUsesFragment::new(set_from(["a::b::c::d::E"]));
  let code = fragment.into_token_stream().to_string();
  assert!(
    code.contains("use a :: b :: c :: d :: E ;"),
    "deeply nested path should split on last ::, got: {code}"
  );
}

#[test]
fn empty_items_after_filtering_produces_empty_braces() {
  let fragment = UseFragment::new(
    "serde".to_string(),
    vec!["not valid".to_string(), "also not valid".to_string()],
  );
  let code = fragment.into_token_stream().to_string();
  assert!(
    code.contains("use serde :: { } ;"),
    "all-invalid items produce empty braces (edge case), got: {code}"
  );
}

macro_rules! generated {
  ($spec:literal, $mode:expr, $file_type:ident $(, $config:expr)?) => {
    crate::generator::codegen::generate_file(
      spec!($spec),
      oas3_gen::generator::codegen::Visibility::Public,
      generated!(@config $($config)?),
      &$mode,
      &oas3_gen::generator::codegen::GeneratedFileType::$file_type,
    )
  };
  (@config) => {
    oas3_gen::generator::CodegenConfig::default()
  };
  (@config $config:expr) => {
    $config
  };
}

mod cargo_manifest;
mod client;
mod coercion;
mod constants;
mod enums;
mod multipart;
mod structs;

use oas3_gen::generator::{
  ClientModMode, CodegenConfig, GenerationMode,
  codegen::{GeneratedFileType, Visibility},
  orchestrator::Orchestrator,
};
use quote::ToTokens as _;
use rstest::rstest;
use syn::{ImplItem, Item};

use crate::parse_spec;

fn compact(code: &str) -> String {
  code.split_whitespace().collect()
}

fn generate_file(
  spec_json: &str,
  visibility: Visibility,
  config: CodegenConfig,
  mode: &dyn GenerationMode,
  file_type: &GeneratedFileType,
) -> anyhow::Result<String> {
  let output = Orchestrator::new(parse_spec(spec_json), visibility, config, None, None).generate(mode, "spec.json")?;
  output
    .code
    .code(file_type)
    .cloned()
    .ok_or_else(|| anyhow::anyhow!("{file_type:?} output should exist"))
}

#[track_caller]
fn assert_snippets(code: &str, snippets: &[&str]) {
  let code = compact(code);
  for snippet in snippets {
    let snippet = compact(snippet);
    assert!(code.contains(&snippet), "missing `{snippet}` in:\n{code}");
  }
}

fn visibility_of(vis: &syn::Visibility) -> String {
  compact(&vis.to_token_stream().to_string())
}

fn reexport_visibilities(code: &str) -> anyhow::Result<Vec<String>> {
  Ok(
    syn::parse_file(code)?
      .items
      .iter()
      .filter_map(|item| match item {
        Item::Use(item) => Some(visibility_of(&item.vis)),
        _ => None,
      })
      .collect(),
  )
}

fn declared_visibilities(code: &str) -> anyhow::Result<Vec<(String, String)>> {
  let file = syn::parse_file(code)?;
  let mut declared = vec![];
  for item in &file.items {
    match item {
      Item::Struct(item) => {
        declared.push((item.ident.to_string(), visibility_of(&item.vis)));
        declared.extend(item.fields.iter().filter_map(|field| {
          field
            .ident
            .as_ref()
            .map(|ident| (format!("{}.{ident}", item.ident), visibility_of(&field.vis)))
        }));
      }
      Item::Enum(item) => declared.push((item.ident.to_string(), visibility_of(&item.vis))),
      Item::Type(item) => declared.push((item.ident.to_string(), visibility_of(&item.vis))),
      Item::Impl(item) if item.trait_.is_none() => {
        declared.extend(item.items.iter().filter_map(|member| match member {
          ImplItem::Fn(method) => Some((method.sig.ident.to_string(), visibility_of(&method.vis))),
          _ => None,
        }));
      }
      _ => {}
    }
  }
  Ok(declared)
}

#[rstest]
#[case::crate_visibility(Visibility::Crate, "pub(crate)", 2)]
#[case::file_visibility(Visibility::File, "pub(super)", 0)]
fn generated_items_follow_the_requested_visibility(
  #[case] visibility: Visibility,
  #[case] expected: &str,
  #[case] reexported: usize,
) -> anyhow::Result<()> {
  let generate = |file_type| {
    generate_file(
      spec!("codegen/pets.json"),
      visibility,
      CodegenConfig::default(),
      &ClientModMode::default(),
      &file_type,
    )
  };
  let reexports = reexport_visibilities(&generate(GeneratedFileType::Module)?)?;
  assert_eq!(reexports, vec![expected; reexported], "module re-exports");

  let declared = [GeneratedFileType::Types, GeneratedFileType::Client]
    .into_iter()
    .map(|file_type| declared_visibilities(&generate(file_type)?))
    .collect::<anyhow::Result<Vec<_>>>()?
    .concat();
  assert!(
    declared.len() > 10,
    "expected structs, enums, aliases, fields, and methods: {declared:?}"
  );
  for (name, actual) in &declared {
    assert_eq!(actual, expected, "visibility of `{name}`");
  }
  Ok(())
}

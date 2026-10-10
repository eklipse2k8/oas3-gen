use oas3_gen::generator::{CodegenConfig, SchemaScope, TypesMode};
use rstest::rstest;

use super::assert_snippets;

#[rstest]
#[case::mistyped_scalars_coerce_to_the_field_type(&[
  ("count", "Some(123i64)"),
  ("label", r#"Some("42".to_string())"#),
  ("flag", "Some(true)"),
  ("ratio", "Some(10f64)"),
])]
#[case::values_outside_the_type_fall_back_to_default(&[
  ("size", "Some(Default::default())"),
  ("limit", "Some(Default::default())"),
  ("tags", "Some(Default::default())"),
])]
fn spec_defaults_become_field_type_literals(#[case] fields: &[(&str, &str)]) -> anyhow::Result<()> {
  let types = generated!(
    "codegen/defaults.json",
    TypesMode,
    Types,
    CodegenConfig::builder().schema_scope(SchemaScope::All).build()
  )?;
  for (field, literal) in fields {
    assert_snippets(&types, &[&format!("#[default({literal})] pub {field}:")]);
  }
  Ok(())
}

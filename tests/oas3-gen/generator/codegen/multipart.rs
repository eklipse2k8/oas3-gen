use oas3_gen::generator::ClientMode;
use rstest::rstest;

use super::assert_snippets;

#[rstest]
#[case::braced_part_name_is_escaped_in_the_format_string(&[
  r#"form = form.text(format!("we{{ird}}[{key}]"), text);"#,
])]
#[case::deprecated_parts_are_read_under_allow_deprecated(&[
  "#[allow(deprecated)] let has_parts = body.we_ird.is_some() || body.legacy.is_some();",
  r#"#[allow(deprecated)] { if let Some(value) = body.legacy { form = form.text("legacy", value); } }"#,
])]
fn client_writes_multipart_edge_parts(#[case] snippets: &[&str]) -> anyhow::Result<()> {
  let client = generated!("codegen/multipart.json", ClientMode, Client)?;
  assert_snippets(&client, snippets);
  Ok(())
}

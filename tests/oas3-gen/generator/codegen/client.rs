use oas3_gen::generator::ClientMode;
use rstest::rstest;

use super::assert_snippets;

#[rstest]
fn url_keeps_a_query_string_embedded_in_the_path() -> anyhow::Result<()> {
  let client = generated!("codegen/messages.json", ClientMode, Client)?;
  assert_snippets(
    &client,
    &[
      r#".push("v1").push("messages").push(&format!("{}.json", request.path.id));
        url.set_query(Some("beta=true"));
        serde::Serialize::serialize(&request.query, serde_html_form::Serializer::new(&mut url.query_pairs_mut())"#,
    ],
  );
  Ok(())
}

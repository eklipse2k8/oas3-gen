use oas3_gen::generator::TypesMode;
use rstest::rstest;

use super::assert_snippets;

#[rstest]
#[case::ranges_accept_what_a_server_sends_under_them(&[
  r#"if status == http::StatusCode::OK {
    if content_type.ends_with("/json") || content_type.ends_with("+json") {"#,
  r#"return Ok(ApiResponse::Other(Box::new(req))); }
  if status == http::StatusCode::CREATED { if content_type.starts_with("text/") {"#,
])]
#[case::exact_type_decoded_as_json_accepts_only_itself(&[
  r#"if status == http::StatusCode::ACCEPTED { if content_type == "font/woff2" {"#,
])]
#[case::binary_type_with_a_schema_also_accepts_json(&[
  r#"if status == http::StatusCode::NOT_FOUND {
    if content_type == "application/octet-stream" || content_type.ends_with("/json") || content_type.ends_with("+json") {"#,
])]
#[case::unparseable_type_reads_every_response(&[
  "if status == http::StatusCode::NON_AUTHORITATIVE_INFORMATION { let data =",
])]
fn parse_response_guards_bodies_by_declared_media_type(#[case] snippets: &[&str]) -> anyhow::Result<()> {
  let types = generated!("codegen/messages.json", TypesMode, Types)?;
  assert_snippets(&types, snippets);
  Ok(())
}

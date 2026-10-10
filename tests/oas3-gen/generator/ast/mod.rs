mod documentation;
mod parsed_path;
mod status_codes;
mod types;
mod validation_attrs;

use oas3_gen::generator::ast::ContentCategory;
use rstest::rstest;

#[rstest]
fn media_types_pick_the_body_format_their_structured_suffix_names() {
  for (content_type, expected) in [
    ("application/json", ContentCategory::Json),
    ("application/json; charset=utf-8", ContentCategory::Json),
    ("application/vnd.api+json", ContentCategory::Json),
    ("application/vnd.atl.bitbucket.bulk+json", ContentCategory::Json),
    ("application/xml", ContentCategory::Xml),
    ("text/xml", ContentCategory::Xml),
    ("application/atom+xml", ContentCategory::Xml),
    ("application/vnd.ms-excel+xml", ContentCategory::Xml),
    ("application/x-www-form-urlencoded", ContentCategory::FormUrlEncoded),
    ("multipart/form-data", ContentCategory::Multipart),
    ("multipart/mixed", ContentCategory::Multipart),
    ("text/event-stream", ContentCategory::EventStream),
    ("text/plain", ContentCategory::Text),
    ("text/csv", ContentCategory::Text),
    ("application/javascript", ContentCategory::Text),
    ("application/octet-stream", ContentCategory::Binary),
    ("application/pdf", ContentCategory::Binary),
    ("image/png", ContentCategory::Binary),
    ("audio/mpeg", ContentCategory::Binary),
    ("video/mp4", ContentCategory::Binary),
    ("not a media type", ContentCategory::Json),
  ] {
    assert_eq!(
      ContentCategory::from_content_type(content_type),
      expected,
      "category of {content_type}"
    );
  }
}

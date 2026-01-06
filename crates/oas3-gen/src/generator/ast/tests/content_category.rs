use crate::generator::ast::ContentCategory;

#[test]
fn vendor_json_suffix() {
  assert_eq!(
    ContentCategory::from_content_type("application/vnd.atl.bitbucket.bulk+json"),
    ContentCategory::Json
  );
  assert_eq!(
    ContentCategory::from_content_type("application/vnd.api+json"),
    ContentCategory::Json
  );
}

#[test]
fn vendor_xml_suffix() {
  assert_eq!(
    ContentCategory::from_content_type("application/vnd.ms-excel+xml"),
    ContentCategory::Xml
  );
  assert_eq!(
    ContentCategory::from_content_type("application/atom+xml"),
    ContentCategory::Xml
  );
}

#[test]
fn standard_types() {
  assert_eq!(
    ContentCategory::from_content_type("application/json"),
    ContentCategory::Json
  );
  assert_eq!(
    ContentCategory::from_content_type("application/xml"),
    ContentCategory::Xml
  );
  assert_eq!(ContentCategory::from_content_type("text/xml"), ContentCategory::Xml);
  assert_eq!(
    ContentCategory::from_content_type("application/x-www-form-urlencoded"),
    ContentCategory::FormUrlEncoded
  );
  assert_eq!(
    ContentCategory::from_content_type("multipart/form-data"),
    ContentCategory::Multipart
  );
  assert_eq!(
    ContentCategory::from_content_type("text/event-stream"),
    ContentCategory::EventStream
  );
  assert_eq!(ContentCategory::from_content_type("text/plain"), ContentCategory::Text);
  assert_eq!(
    ContentCategory::from_content_type("application/octet-stream"),
    ContentCategory::Binary
  );
  assert_eq!(ContentCategory::from_content_type("image/png"), ContentCategory::Binary);
}

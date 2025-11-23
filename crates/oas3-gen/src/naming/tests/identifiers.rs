use crate::naming::identifiers::{header_const_name, regex_const_name, to_rust_field_name, to_rust_type_name};

#[test]
fn test_field_names() {
  assert_eq!(to_rust_field_name("foo-bar"), "foo_bar");
  assert_eq!(to_rust_field_name("match"), "r#match");
  assert_eq!(to_rust_field_name("self"), "self_");
  assert_eq!(to_rust_field_name("123name"), "_123name");
  assert_eq!(to_rust_field_name(""), "_");
  assert_eq!(to_rust_field_name("  "), "_");
}

#[test]
fn test_field_names_negative_prefix() {
  assert_eq!(to_rust_field_name("-created-date"), "negative_created_date");
  assert_eq!(to_rust_field_name("-id"), "negative_id");
  assert_eq!(to_rust_field_name("-modified-date"), "negative_modified_date");
  assert_eq!(to_rust_field_name("-"), "_");
}

#[test]
fn test_type_names() {
  assert_eq!(to_rust_type_name("oAuth"), "OAuth");
  assert_eq!(to_rust_type_name("-INF"), "NegativeInf");
  assert_eq!(to_rust_type_name("123Response"), "T123Response");
  assert_eq!(to_rust_type_name(""), "Unnamed");
  assert_eq!(to_rust_type_name("  "), "Unnamed");
}

#[test]
fn test_type_names_preserve_pascal_case() {
  assert_eq!(
    to_rust_type_name("BetaResponseMCPToolUseBlock"),
    "BetaResponseMCPToolUseBlock"
  );
  assert_eq!(to_rust_type_name("XMLHttpRequest"), "XMLHttpRequest");
  assert_eq!(to_rust_type_name("IOError"), "IOError");
  assert_eq!(to_rust_type_name("HTTPSConnection"), "HTTPSConnection");
  assert_eq!(
    to_rust_type_name("betaResponseMCPToolUseBlock"),
    "BetaResponseMCPToolUseBlock"
  );
  assert_eq!(to_rust_type_name("xmlHttpRequest"), "XmlHttpRequest");
  assert_eq!(
    to_rust_type_name("beta_response_mcp_tool_use_block"),
    "BetaResponseMcpToolUseBlock"
  );
  assert_eq!(
    to_rust_type_name("beta-response-mcp-tool-use-block"),
    "BetaResponseMcpToolUseBlock"
  );
  assert_eq!(to_rust_type_name("beta_ResponseMCP"), "BetaResponseMcp");
  assert_eq!(to_rust_type_name("Beta-Response-MCP"), "BetaResponseMcp");
}

#[test]
fn test_type_names_normalize_separated_uppercase() {
  assert_eq!(to_rust_type_name("NOT_FORCED"), "NotForced");
  assert_eq!(to_rust_type_name("ADD"), "Add");
  assert_eq!(to_rust_type_name("DELETE"), "Delete");
  assert_eq!(to_rust_type_name("PDF_FILE"), "PdfFile");
  assert_eq!(to_rust_type_name("HTTP_URL"), "HttpUrl");
}

#[test]
fn test_type_names_preserve_mixed_case_no_separators() {
  assert_eq!(to_rust_type_name("PDFFile"), "PDFFile");
  assert_eq!(to_rust_type_name("HTTPConnection"), "HTTPConnection");
  assert_eq!(to_rust_type_name("URLPath"), "URLPath");
}

#[test]
fn test_type_names_negative_prefix() {
  assert_eq!(to_rust_type_name("-created-date"), "NegativeCreatedDate");
  assert_eq!(to_rust_type_name("-id"), "NegativeId");
  assert_eq!(to_rust_type_name("-modified-date"), "NegativeModifiedDate");
  assert_eq!(to_rust_type_name("-child-position"), "NegativeChildPosition");
  assert_eq!(to_rust_type_name("-"), "Unnamed");
}

#[test]
fn test_type_name_reserved_pascal() {
  assert_eq!(to_rust_type_name("clone"), "r#Clone");
  assert_eq!(to_rust_type_name("Vec"), "r#Vec");
}

#[test]
fn test_const_name() {
  assert_eq!(regex_const_name(&["foo.bar", "baz"]), "REGEX_FOO_BAR_BAZ");
  assert_eq!(regex_const_name(&["1a", "2b"]), "REGEX__1A_2B");
}

#[test]
fn test_header_const_name() {
  assert_eq!(header_const_name("x-my-header"), "X_MY_HEADER");
  assert_eq!(header_const_name("Content-Type"), "CONTENT_TYPE");
  assert_eq!(header_const_name("123-custom"), "_123_CUSTOM");
  assert_eq!(header_const_name(""), "HEADER");
}

#[test]
fn test_header_const_name_case_insensitive() {
  assert_eq!(
    header_const_name("X-API-Key"),
    header_const_name("x-api-key"),
    "header constant names should be case-insensitive"
  );
  assert_eq!(
    header_const_name("Content-Type"),
    header_const_name("content-type"),
    "header constant names should be case-insensitive"
  );
  assert_eq!(
    header_const_name("AUTHORIZATION"),
    header_const_name("authorization"),
    "header constant names should be case-insensitive"
  );
}

use std::collections::BTreeSet;

use crate::generator::naming::identifiers::{
  ensure_unique, header_const_name, regex_const_name, split_pascal_case, to_rust_field_name, to_rust_type_name,
};

#[test]
fn test_field_names() {
  let cases = [
    // Basic transformations
    ("foo-bar", "foo_bar"),
    ("match", "r#match"),
    ("self", "self_"),
    ("123name", "_123name"),
    ("", "_"),
    ("  ", "_"),
    // Negative prefix handling
    ("-created-date", "negative_created_date"),
    ("-id", "negative_id"),
    ("-modified-date", "negative_modified_date"),
    ("-", "_"),
  ];
  for (input, expected) in cases {
    assert_eq!(to_rust_field_name(input), expected, "failed for input {input:?}");
  }
}

#[test]
fn test_type_names() {
  let cases = [
    // Basic transformations
    ("oAuth", "OAuth"),
    ("-INF", "NegativeInf"),
    ("123Response", "T123Response"),
    ("", "Unnamed"),
    ("  ", "Unnamed"),
    // Preserve pascal case with uppercase sequences
    ("BetaResponseMCPToolUseBlock", "BetaResponseMCPToolUseBlock"),
    ("XMLHttpRequest", "XMLHttpRequest"),
    ("IOError", "IOError"),
    ("HTTPSConnection", "HTTPSConnection"),
    ("betaResponseMCPToolUseBlock", "BetaResponseMCPToolUseBlock"),
    ("xmlHttpRequest", "XmlHttpRequest"),
    ("beta_response_mcp_tool_use_block", "BetaResponseMcpToolUseBlock"),
    ("beta-response-mcp-tool-use-block", "BetaResponseMcpToolUseBlock"),
    ("beta_ResponseMCP", "BetaResponseMcp"),
    ("Beta-Response-MCP", "BetaResponseMcp"),
    // Normalize separated uppercase
    ("NOT_FORCED", "NotForced"),
    ("ADD", "Add"),
    ("DELETE", "Delete"),
    ("PDF_FILE", "PdfFile"),
    ("HTTP_URL", "HttpUrl"),
    // Preserve mixed case without separators
    ("PDFFile", "PDFFile"),
    ("HTTPConnection", "HTTPConnection"),
    ("URLPath", "URLPath"),
    // Negative prefix handling
    ("-created-date", "NegativeCreatedDate"),
    ("-id", "NegativeId"),
    ("-modified-date", "NegativeModifiedDate"),
    ("-child-position", "NegativeChildPosition"),
    ("-", "Unnamed"),
    // Reserved words in pascal case
    ("clone", "r#Clone"),
    ("Vec", "r#Vec"),
  ];
  for (input, expected) in cases {
    assert_eq!(to_rust_type_name(input), expected, "failed for input {input:?}");
  }
}

#[test]
fn test_regex_const_name() {
  let cases = [
    (vec!["foo.bar", "baz"], "REGEX_FOO_BAR_BAZ"),
    (vec!["1a", "2b"], "REGEX__1A_2B"),
  ];
  for (input, expected) in cases {
    assert_eq!(regex_const_name(&input), expected, "failed for input {input:?}");
  }
}

#[test]
fn test_header_const_name() {
  let cases = [
    ("x-my-header", "X_MY_HEADER"),
    ("Content-Type", "CONTENT_TYPE"),
    ("123-custom", "_123_CUSTOM"),
    ("", "HEADER"),
  ];
  for (input, expected) in cases {
    assert_eq!(header_const_name(input), expected, "failed for input {input:?}");
  }
}

#[test]
fn test_header_const_name_case_insensitive() {
  let case_pairs = [
    ("X-API-Key", "x-api-key"),
    ("Content-Type", "content-type"),
    ("AUTHORIZATION", "authorization"),
  ];
  for (upper, lower) in case_pairs {
    assert_eq!(
      header_const_name(upper),
      header_const_name(lower),
      "header constant names should be case-insensitive for {upper:?} vs {lower:?}"
    );
  }
}

#[test]
fn test_ensure_unique() {
  let cases = vec![
    (vec!["UserResponse"], "UserResponse", "UserResponse2"),
    (
      vec!["UserResponse", "UserResponse2", "UserResponse3"],
      "UserResponse",
      "UserResponse4",
    ),
    (vec![], "", ""),
    (vec!["Name2"], "Name", "Name"),
    (vec![], "UniqueName", "UniqueName"),
    (vec!["Value", "Value3"], "Value", "Value2"),
  ];

  for (used_list, input, expected) in cases {
    let used: BTreeSet<String> = used_list.into_iter().map(String::from).collect();
    assert_eq!(
      ensure_unique(input, &used),
      expected,
      "Failed for input '{input}' with used {used:?}"
    );
  }
}

#[test]
fn test_split_pascal_case() {
  let cases = vec![
    ("UserName", vec!["User", "Name"]),
    ("SimpleTest", vec!["Simple", "Test"]),
    ("HTTPSConnection", vec!["HTTPS", "Connection"]),
    ("XMLParser", vec!["XML", "Parser"]),
    ("JSONResponse", vec!["JSON", "Response"]),
    ("HTTPStatus", vec!["HTTP", "Status"]),
    ("HTTPS", vec!["HTTPS"]),
    ("XML", vec!["XML"]),
    ("User", vec!["User"]),
    ("Status", vec!["Status"]),
    ("", vec![]),
  ];

  for (input, expected) in cases {
    assert_eq!(split_pascal_case(input), expected, "Failed for input '{input}'");
  }
}

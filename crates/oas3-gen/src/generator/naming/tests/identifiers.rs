use std::collections::BTreeSet;

use crate::generator::{
  ast::{RegexKey, StructToken, tokens::ConstToken},
  naming::identifiers::{ensure_unique, split_pascal_case, strip_parent_prefix, to_rust_field_name, to_rust_type_name},
};

#[test]
fn test_field_names() {
  let cases = [
    // Basic transformations
    ("foo-bar", "foo_bar"),
    ("match", "r#match"),
    ("static", "r#static"),
    ("type", "r#type"),
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
    // Prelude types get Type suffix to avoid shadowing
    ("clone", "CloneType"),
    ("Vec", "VecType"),
    ("Result", "ResultType"),
    ("Option", "OptionType"),
    ("Copy", "CopyType"),
    ("Display", "DisplayType"),
    ("Send", "SendType"),
    ("Sync", "SyncType"),
    ("Type", "TypeType"),
    // Self is a keyword, so it gets raw identifier prefix
    ("Self", "r#Self"),
    // Raw identifier prefixes should be stripped and PascalCased
    ("r#move", "Move"),
    ("r#static", "Static"),
    ("r#type", "TypeType"),
    ("r#match", "Match"),
    // Keywords become proper PascalCase (no r# needed for type names)
    ("move", "Move"),
    ("static", "Static"),
    ("type", "TypeType"),
    ("match", "Match"),
  ];
  for (input, expected) in cases {
    assert_eq!(to_rust_type_name(input), expected, "failed for input {input:?}");
  }
}

#[test]
fn test_const_token_from_regex_key() {
  let cases = [
    (("foo.bar", "baz"), "REGEX_FOO_BAR_BAZ"),
    (("1a", "2b"), "REGEX_T1A_2B"),
  ];
  for ((type_name, field_name), expected) in cases {
    let type_token = StructToken::from_raw(type_name);
    let key = RegexKey::for_struct(&type_token, field_name);
    let token = ConstToken::from(&key);
    assert_eq!(
      token.to_string(),
      expected,
      "failed for type={type_name:?}, field={field_name:?}"
    );
  }
}

#[test]
fn test_const_token_from_raw() {
  let cases = [
    ("x-my-header", "X_MY_HEADER"),
    ("Content-Type", "CONTENT_TYPE"),
    ("123-custom", "_123_CUSTOM"),
    ("", "UNNAMED"),
    ("  ", "UNNAMED"),
  ];
  for (input, expected) in cases {
    let token = ConstToken::from_raw(input);
    assert_eq!(token.to_string(), expected, "failed for input {input:?}");
  }
}

#[test]
fn test_const_token_case_insensitive() {
  let case_pairs = [
    ("X-API-Key", "x-api-key"),
    ("Content-Type", "content-type"),
    ("AUTHORIZATION", "authorization"),
  ];
  for (upper, lower) in case_pairs {
    assert_eq!(
      ConstToken::from_raw(upper).to_string(),
      ConstToken::from_raw(lower).to_string(),
      "constant identifiers should be case-insensitive for {upper:?} vs {lower:?}"
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

#[test]
fn test_strip_parent_prefix_basic() {
  let cases = [
    ("User", "UserProfile", "Profile"),
    ("User", "UserSettings", "Settings"),
    ("Event", "EventMessage", "Message"),
    ("Event", "EventData", "Data"),
    ("API", "APIResponse", "Response"),
    ("HTTP", "HTTPRequest", "Request"),
  ];

  for (parent, child, expected) in cases {
    assert_eq!(
      strip_parent_prefix(parent, child),
      expected,
      "Failed for parent='{parent}', child='{child}'"
    );
  }
}

#[test]
fn test_strip_parent_prefix_no_common_prefix() {
  let cases = [
    ("User", "Profile", "Profile"),
    ("Event", "Message", "Message"),
    ("Foo", "Bar", "Bar"),
    ("Request", "Response", "Response"),
  ];

  for (parent, child, expected) in cases {
    assert_eq!(
      strip_parent_prefix(parent, child),
      expected,
      "Failed for parent='{parent}', child='{child}'"
    );
  }
}

#[test]
fn test_strip_parent_prefix_word_boundary_respected() {
  let cases = [
    ("User", "Username", "Username"),
    ("User", "Userdata", "Userdata"),
    ("API", "Apiary", "Apiary"),
    ("Pet", "Peter", "Peter"),
  ];

  for (parent, child, expected) in cases {
    assert_eq!(
      strip_parent_prefix(parent, child),
      expected,
      "Failed for parent='{parent}', child='{child}'"
    );
  }
}

#[test]
fn test_strip_parent_prefix_complete_match() {
  let cases = [
    ("User", "User", "Item"),
    ("Event", "Event", "Item"),
    ("API", "API", "Item"),
  ];

  for (parent, child, expected) in cases {
    assert_eq!(
      strip_parent_prefix(parent, child),
      expected,
      "Failed for parent='{parent}', child='{child}'"
    );
  }
}

#[test]
fn test_strip_parent_prefix_multi_word() {
  let cases = [
    ("UserProfile", "UserProfileSettings", "Settings"),
    ("UserProfile", "UserSettings", "Settings"),
    ("EventMessage", "EventMessageData", "Data"),
    ("EventMessage", "EventData", "Data"),
    ("APIResponse", "APIResponseError", "Error"),
    ("HTTPRequest", "HTTPRequestBody", "Body"),
  ];

  for (parent, child, expected) in cases {
    assert_eq!(
      strip_parent_prefix(parent, child),
      expected,
      "Failed for parent='{parent}', child='{child}'"
    );
  }
}

#[test]
fn test_strip_parent_prefix_discriminated_enum_scenarios() {
  let cases = [
    ("Pet", "PetCat", "Cat"),
    ("Pet", "PetDog", "Dog"),
    ("Pet", "PetBird", "Bird"),
    ("Vehicle", "VehicleCar", "Car"),
    ("Vehicle", "VehicleTruck", "Truck"),
    ("Shape", "ShapeCircle", "Circle"),
    ("Shape", "ShapeSquare", "Square"),
    ("Message", "MessageText", "Text"),
    ("Message", "MessageImage", "Image"),
    ("Message", "MessageAudio", "Audio"),
  ];

  for (parent, child, expected) in cases {
    assert_eq!(
      strip_parent_prefix(parent, child),
      expected,
      "Failed for discriminated enum: parent='{parent}', child='{child}'"
    );
  }
}

#[test]
fn test_strip_parent_prefix_preserves_unrelated_names() {
  let cases = [
    ("Pet", "Cat", "Cat"),
    ("Pet", "Dog", "Dog"),
    ("Vehicle", "Sedan", "Sedan"),
    ("Shape", "Triangle", "Triangle"),
    ("Message", "Email", "Email"),
  ];

  for (parent, child, expected) in cases {
    assert_eq!(
      strip_parent_prefix(parent, child),
      expected,
      "Failed: parent='{parent}', child='{child}' should remain unchanged"
    );
  }
}

use std::collections::BTreeSet;

use oas3_gen::generator::{
  ast::{RegexKey, StructToken, tokens::ConstToken},
  naming::identifiers::{
    builder_setter_name, ensure_unique, split_pascal_case, strip_parent_prefix, to_rust_field_name, to_rust_type_name,
  },
};
use proptest::{collection::btree_set, prelude::*};
use rstest::rstest;

use super::assert_converts;

const NAME_WITH_IDENTIFIER_CHARACTER: &str = "[A-Za-z0-9 _.$éü日-]{0,8}[A-Za-z0-9][A-Za-z0-9 _.$éü日-]{0,8}";
const HEADER_NAME: &str = "[A-Za-z0-9][A-Za-z0-9-]{0,15}";
const RESERVED_TYPE_NAMES: [&str; 11] = [
  "Clone", "Copy", "Display", "Option", "Result", "S", "Self", "Send", "Sync", "Type", "Vec",
];
const HIGHEST_SUFFIX: u32 = 8;
const BON_RESERVED_SETTERS: [&str; 2] = ["build", "builder"];

fn is_identifier(name: &str) -> bool {
  syn::parse_str::<syn::Ident>(name).is_ok()
}

fn regex_const_name((type_name, field_name): (&str, &str)) -> String {
  ConstToken::from(&RegexKey::for_struct(&StructToken::from_raw(type_name), field_name)).to_string()
}

#[rstest]
fn identifiers_follow_rust_naming_conventions() {
  assert_converts(
    to_rust_type_name,
    &[
      ("oAuth", "OAuth"),
      ("-INF", "NegativeInf"),
      ("123Response", "T123Response"),
      ("", "Unnamed"),
      ("  ", "Unnamed"),
      ("-", "Unnamed"),
      ("BetaResponseMCPToolUseBlock", "BetaResponseMCPToolUseBlock"),
      ("betaResponseMCPToolUseBlock", "BetaResponseMCPToolUseBlock"),
      ("XMLHttpRequest", "XMLHttpRequest"),
      ("xmlHttpRequest", "XmlHttpRequest"),
      ("IOError", "IOError"),
      ("HTTPSConnection", "HTTPSConnection"),
      ("PDFFile", "PDFFile"),
      ("URLPath", "URLPath"),
      ("beta_response_mcp_tool_use_block", "BetaResponseMcpToolUseBlock"),
      ("beta-response-mcp-tool-use-block", "BetaResponseMcpToolUseBlock"),
      ("beta_ResponseMCP", "BetaResponseMcp"),
      ("Beta-Response-MCP", "BetaResponseMcp"),
      ("NOT_FORCED", "NotForced"),
      ("ADD", "Add"),
      ("DELETE", "Delete"),
      ("PDF_FILE", "PdfFile"),
      ("HTTP_URL", "HttpUrl"),
      ("-created-date", "NegativeCreatedDate"),
      ("-id", "NegativeId"),
      ("-child-position", "NegativeChildPosition"),
      ("clone", "CloneType"),
      ("Vec", "VecType"),
      ("Result", "ResultType"),
      ("Option", "OptionType"),
      ("Copy", "CopyType"),
      ("Display", "DisplayType"),
      ("Send", "SendType"),
      ("Sync", "SyncType"),
      ("Type", "TypeType"),
      ("Self", "SelfType"),
      ("self", "SelfType"),
      ("S", "SType"),
      ("s", "SType"),
      ("r#move", "Move"),
      ("r#static", "Static"),
      ("r#type", "TypeType"),
      ("r#match", "Match"),
      ("move", "Move"),
      ("static", "Static"),
      ("type", "TypeType"),
      ("match", "Match"),
    ],
  );
  assert_converts(
    to_rust_field_name,
    &[
      ("foo-bar", "foo_bar"),
      ("match", "r#match"),
      ("static", "r#static"),
      ("type", "r#type"),
      ("r#type", "r#type"),
      ("self", "self_"),
      ("crate", "crate_"),
      ("super", "super_"),
      ("r#crate", "crate_"),
      ("123name", "_123name"),
      ("", "_"),
      ("  ", "_"),
      ("-", "_"),
      ("-created-date", "negative_created_date"),
      ("-id", "negative_id"),
      ("-modified-date", "negative_modified_date"),
    ],
  );
  assert_converts(
    |raw: &str| ConstToken::from_raw(raw).to_string(),
    &[
      ("x-my-header", "X_MY_HEADER"),
      ("Content-Type", "CONTENT_TYPE"),
      ("content-type", "CONTENT_TYPE"),
      ("X-API-Key", "X_API_KEY"),
      ("x-api-key", "X_API_KEY"),
      ("AUTHORIZATION", "AUTHORIZATION"),
      ("123-custom", "_123_CUSTOM"),
      ("", "UNNAMED"),
      ("  ", "UNNAMED"),
    ],
  );
  assert_converts(
    regex_const_name,
    &[
      (("foo.bar", "baz"), "REGEX_FOO_BAR_BAZ"),
      (("1a", "2b"), "REGEX_T1A_2B"),
    ],
  );
  assert_converts(
    builder_setter_name,
    &[
      ("build", Some("build_value".to_owned())),
      ("builder", Some("builder_value".to_owned())),
      ("_2fa", Some("value_2fa".to_owned())),
      ("__3d", Some("value_3d".to_owned())),
      ("_private", None),
      ("name", None),
      ("r#type", None),
    ],
  );
}

#[rstest]
fn strip_parent_prefix_drops_whole_words_shared_with_parent() {
  assert_converts(
    |name: &str| split_pascal_case(name),
    &[
      ("UserName", vec!["User", "Name"]),
      ("HTTPSConnection", vec!["HTTPS", "Connection"]),
      ("XMLParser", vec!["XML", "Parser"]),
      ("HTTPS", vec!["HTTPS"]),
      ("", vec![]),
    ],
  );
  assert_converts(
    |(parent, child): (&str, &str)| strip_parent_prefix(parent, child),
    &[
      (("User", "UserProfile"), "Profile"),
      (("Pet", "PetCat"), "Cat"),
      (("API", "APIResponse"), "Response"),
      (("HTTP", "HTTPRequest"), "Request"),
      (("UserProfile", "UserProfileSettings"), "Settings"),
      (("UserProfile", "UserSettings"), "Settings"),
      (("APIResponse", "APIResponseError"), "Error"),
      (("User", "Username"), "Username"),
      (("API", "Apiary"), "Apiary"),
      (("Pet", "Peter"), "Peter"),
      (("User", "User"), "Item"),
      (("API", "API"), "Item"),
      (("Request", "Response"), "Response"),
      (("Pet", "Cat"), "Cat"),
    ],
  );
}

proptest! {
  #![proptest_config(ProptestConfig::with_cases(256))]

  #[test]
  fn identifiers_from_any_name_are_valid_rust(name in NAME_WITH_IDENTIFIER_CHARACTER, header in HEADER_NAME) {
    let type_name = to_rust_type_name(&name);
    prop_assert!(is_identifier(&type_name), "type name {type_name:?} from {name:?}");
    prop_assert!(type_name.starts_with(|c: char| c.is_ascii_uppercase()), "type name {type_name:?} from {name:?}");
    prop_assert!(type_name.chars().all(|c| c.is_ascii_alphanumeric()), "type name {type_name:?} from {name:?}");
    prop_assert!(!RESERVED_TYPE_NAMES.contains(&type_name.as_str()), "type name {type_name:?} from {name:?}");

    let field_name = to_rust_field_name(&name);
    prop_assert!(is_identifier(&field_name), "field name {field_name:?} from {name:?}");
    prop_assert!(
      field_name.trim_start_matches("r#").chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_'),
      "field name {field_name:?} from {name:?}"
    );

    let setter = builder_setter_name(&field_name).unwrap_or_else(|| field_name.trim_start_matches('_').to_string());
    prop_assert!(is_identifier(&setter), "setter {setter:?} for field {field_name:?}");
    prop_assert!(!BON_RESERVED_SETTERS.contains(&setter.as_str()), "setter {setter:?} for field {field_name:?}");

    let const_name = ConstToken::from_raw(&name).to_string();
    prop_assert!(is_identifier(&const_name), "const name {const_name:?} from {name:?}");
    prop_assert!(
      const_name.chars().all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_'),
      "const name {const_name:?} from {name:?}"
    );
    prop_assert_eq!(
      ConstToken::from_raw(header.to_ascii_uppercase()),
      ConstToken::from_raw(header.to_ascii_lowercase()),
      "header constant case sensitivity for {:?}",
      header
    );
  }

  #[test]
  fn ensure_unique_appends_smallest_free_number(
    base in "[A-Z][a-z]{0,6}",
    base_taken in any::<bool>(),
    taken_numbers in btree_set(2..HIGHEST_SUFFIX, 0..6),
  ) {
    let used = taken_numbers
      .iter()
      .map(|number| format!("{base}{number}"))
      .chain(base_taken.then(|| base.clone()))
      .collect::<BTreeSet<_>>();
    let expected = if base_taken {
      (2..=HIGHEST_SUFFIX).find(|number| !taken_numbers.contains(number)).map(|number| format!("{base}{number}"))
    } else {
      Some(base.clone())
    };
    prop_assert_eq!(Some(ensure_unique(&base, &used)), expected);
  }
}

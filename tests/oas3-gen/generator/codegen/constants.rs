use http::{HeaderName, header};
use oas3_gen::generator::ast::constants::HttpHeaderRef;
use proptest::prelude::*;

use super::compact;

const STANDARD_HEADERS: &[HeaderName] = &[
  header::ACCEPT,
  header::AUTHORIZATION,
  header::CACHE_CONTROL,
  header::CONTENT_TYPE,
  header::IF_NONE_MATCH,
  header::PROXY_AUTHORIZATION,
  header::SEC_WEBSOCKET_KEY,
  header::WWW_AUTHENTICATE,
  header::X_CONTENT_TYPE_OPTIONS,
];

fn constant_name(header: &str) -> String {
  header.to_ascii_uppercase().replace('-', "_")
}

fn with_random_case(name: &str, mask: u64) -> String {
  name
    .chars()
    .enumerate()
    .map(|(index, character)| {
      if mask >> (index % 64) & 1 == 1 {
        character.to_ascii_uppercase()
      } else {
        character
      }
    })
    .collect()
}

fn with_title_case(name: &str) -> String {
  name
    .split('-')
    .map(|segment| {
      let mut characters = segment.chars();
      characters
        .next()
        .map(|first| first.to_ascii_uppercase().to_string() + characters.as_str())
        .unwrap_or_default()
    })
    .collect::<Vec<_>>()
    .join("-")
}

proptest! {
  #![proptest_config(ProptestConfig::with_cases(128))]

  #[test]
  fn header_constants_reuse_http_constants_in_any_case(
    standard in prop::sample::select(STANDARD_HEADERS),
    mask in any::<u64>(),
    custom in "[a-z]{1,8}(-[a-z0-9]{1,8}){0,2}",
    custom_casing in prop_oneof![
      Just(str::to_string as fn(&str) -> String),
      Just(str::to_ascii_uppercase as fn(&str) -> String),
      Just(with_title_case as fn(&str) -> String),
    ],
  ) {
    let standard_ref = HttpHeaderRef::from(with_random_case(standard.as_str(), mask).as_str());
    prop_assert!(standard_ref.is_declared_by_http());
    prop_assert_eq!(
      compact(&standard_ref.path().to_string()),
      format!("http::header::{}", constant_name(standard.as_str()))
    );

    let custom = format!("x-oas3-{custom}");
    let custom_ref = HttpHeaderRef::from(custom_casing(&custom).as_str());
    prop_assert!(!custom_ref.is_declared_by_http());
    prop_assert_eq!(compact(&custom_ref.path().to_string()), constant_name(&custom));
  }
}

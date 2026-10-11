use std::sync::LazyLock;

use proptest::{collection, prelude::*};
use reqwest::{
  Method, RequestBuilder, StatusCode,
  header::{AUTHORIZATION, CONTENT_TYPE},
};
use rstest::rstest;
use secrecy::ExposeSecret;
use serde_json::json;

use crate::{
  Keyed as _, Raw as _, property_config, send,
  support::{Petstore, Received, STAFF_TOKEN, petstore, run_case, start_shared},
};

const KEY_PATH: &str = "/staff/keys/key_1";
const MALFORMED_JSON: &str = r#"{"name": "#;

#[rstest]
#[case::inherited_global_key_before_a_malformed_body(
  |petstore: &Petstore| petstore.raw(Method::POST, "/pets").header(CONTENT_TYPE, "application/json").body(MALFORMED_JSON),
  &["`X-Api-Key`"],
)]
#[case::either_api_key_scheme_before_a_malformed_body(
  |petstore: &Petstore| petstore.raw(Method::POST, "/staff/keys").header(CONTENT_TYPE, "application/json").body(MALFORMED_JSON),
  &["`QueryKey`", "`SessionCookie`"],
)]
#[case::both_api_key_schemes_with_only_the_header(
  |petstore: &Petstore| petstore.raw(Method::GET, KEY_PATH).api_key(),
  &["`session`"],
)]
#[case::bearer_scheme_without_a_separator(
  |petstore: &Petstore| petstore.raw(Method::DELETE, KEY_PATH).header(AUTHORIZATION, format!("Bearer{STAFF_TOKEN}")),
  &["`ApiKeyAuth`", "`BearerAuth`"],
)]
#[tokio::test]
async fn a_missing_required_credential_is_refused_before_the_service_runs(
  #[future(awt)] petstore: Petstore,
  #[case] request: fn(&Petstore) -> RequestBuilder,
  #[case] named: &[&str],
) -> anyhow::Result<()> {
  let reply = send(request(&petstore)).await?;

  let reason = reply.text();
  assert_eq!(reply.status, StatusCode::UNAUTHORIZED, "{reason}");
  assert_eq!(reply.media_type.as_deref(), Some("text/plain"));
  assert!(
    named.iter().all(|name| reason.contains(name)),
    "{reason} names {named:?}"
  );
  assert!(petstore.service.received().is_empty());
  Ok(())
}

#[rstest]
#[case::optional_security_ignoring_unsupported_basic_credentials(
  |petstore: &Petstore| petstore
    .raw(Method::PUT, KEY_PATH)
    .basic_auth("staff", Some("hunter2"))
    .json(&json!({ "name": "rotated", "value": "sk_rotated" })),
  StatusCode::OK,
  |received: &Received| matches!(received, Received::ReplaceApiKey(request) if request.credentials.api_key_auth.is_none()),
)]
#[case::webhook(
  |petstore: &Petstore| petstore
    .raw(Method::POST, "/webhooks/inventoryLow")
    .json(&json!({ "species": "cat", "remaining": 2 })),
  StatusCode::NO_CONTENT,
  |received: &Received| matches!(received, Received::InventoryLow(_)),
)]
#[case::callback(
  |petstore: &Petstore| petstore
    .raw(Method::POST, "/callbacks/adoptPet/statusChanged")
    .json(&json!({ "adoptionId": 7, "status": "approved", "occurredAt": "2026-10-09T18:00:00Z" })),
  StatusCode::NO_CONTENT,
  |received: &Received| matches!(received, Received::AdoptPetStatusChanged(_)),
)]
#[tokio::test]
async fn requests_no_supported_requirement_covers_reach_the_service_without_credentials(
  #[future(awt)] petstore: Petstore,
  #[case] request: fn(&Petstore) -> RequestBuilder,
  #[case] expected: StatusCode,
  #[case] admitted: fn(&Received) -> bool,
) -> anyhow::Result<()> {
  let reply = send(request(&petstore)).await?;

  assert_eq!(reply.status, expected, "{}", reply.text());
  let received = petstore.service.received();
  assert!(matches!(received.as_slice(), [only] if admitted(only)), "{received:?}");
  Ok(())
}

fn bearer_scheme() -> impl Strategy<Value = String> {
  (collection::vec(any::<bool>(), 6), 1..4_usize).prop_map(|(uppercase, spaces)| {
    "bearer"
      .chars()
      .zip(uppercase)
      .map(|(letter, upper)| if upper { letter.to_ascii_uppercase() } else { letter })
      .chain(std::iter::repeat_n(' ', spaces))
      .collect()
  })
}

proptest! {
  #![proptest_config(property_config(48))]

  #[test]
  fn any_casing_and_spacing_of_the_bearer_scheme_delivers_the_token(
    scheme in bearer_scheme(),
    token in "[A-Za-z0-9._~+/-]{1,32}={0,2}",
  ) {
    static PETSTORE: LazyLock<Petstore> = LazyLock::new(start_shared);
    let petstore = &*PETSTORE;
    run_case(async || {
      petstore
        .raw(Method::DELETE, KEY_PATH)
        .header(AUTHORIZATION, format!("{scheme}{token}"))
        .send()
        .await?;
      let Some(Received::RevokeApiKey(request)) = petstore.service.last_received() else {
        return Err(TestCaseError::fail("the service did not receive revokeApiKey"));
      };
      prop_assert_eq!(
        request.credentials.bearer_auth.as_ref().map(ExposeSecret::expose_secret),
        Some(token.as_str())
      );
      Ok(())
    })?;
  }
}

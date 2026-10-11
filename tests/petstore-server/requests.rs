use std::{fmt::Write as _, sync::LazyLock};

use indexmap::IndexMap;
use petstore_server as server;
use proptest::{collection, prelude::*, sample::Index};
use reqwest::{
  Method, RequestBuilder, StatusCode,
  header::{CONTENT_TYPE, COOKIE},
  multipart::{Form, Part},
};
use rstest::rstest;
use secrecy::ExposeSecret as _;
use serde_json::json;

use crate::{
  Keyed as _, PET_ID, PNG_SIGNATURE, Raw as _, property_config, seed_pet, send,
  support::{Petstore, Received, petstore, run_case, start_shared},
};

const MERGE_PATCH: &str = "application/merge-patch+json";

fn intake_form(omitted: Option<&str>, count: &str) -> Form {
  [
    ("name", Part::text("Biscuit")),
    (
      "thumbnail",
      Part::bytes(PNG_SIGNATURE.to_vec()).file_name("thumbnail.png"),
    ),
    ("encoded", Part::text("aGk=")),
    ("count", Part::text(count.to_string())),
  ]
  .into_iter()
  .filter(|(name, _)| Some(*name) != omitted)
  .fold(Form::new(), |form, (name, part)| form.part(name, part))
}

#[rstest]
#[case::missing_required_header(
  |petstore: &Petstore| petstore.raw(Method::GET, "/pets"),
  "missing required header `x-compatibility-date`",
)]
#[case::missing_required_cookie(
  |petstore: &Petstore| petstore.raw(Method::GET, "/assistant/events").api_key(),
  "missing required cookie `conversation`",
)]
#[case::unlisted_enum_value_in_a_cookie(
  |petstore: &Petstore| petstore.raw(Method::GET, &format!("/pets/{PET_ID}/card")).api_key().header(COOKIE, "cardTheme=sepia"),
  "invalid value for cookie `cardTheme`",
)]
#[case::member_a_closed_schema_does_not_name(
  |petstore: &Petstore| petstore
    .raw(Method::PATCH, &format!("/pets/{PET_ID}"))
    .api_key()
    .header(CONTENT_TYPE, MERGE_PATCH)
    .body(r#"{"name":"Rex","favoriteColor":"red"}"#),
  "unknown field `favoriteColor`",
)]
#[case::repeated_unique_items(
  |petstore: &Petstore| petstore
    .raw(Method::POST, "/pets")
    .api_key()
    .json(&json!({ "name": "Rex", "species": "dog", "tags": ["calm", "shy", "calm"] })),
  "tags: ",
)]
#[case::missing_multipart_part(
  |petstore: &Petstore| petstore.raw(Method::POST, "/intake/form-7").api_key().multipart(intake_form(Some("thumbnail"), "7")),
  "missing multipart part `thumbnail`",
)]
#[case::unparseable_multipart_part(
  |petstore: &Petstore| petstore.raw(Method::POST, "/intake/form-7").api_key().multipart(intake_form(None, "seven")),
  "invalid multipart part `count`",
)]
#[case::json_for_an_optional_multipart_body(
  |petstore: &Petstore| petstore.raw(Method::POST, "/media").api_key().json(&json!({ "note": "sent as JSON" })),
  "the body must be `multipart/form-data`",
)]
#[case::json_for_an_optional_form_body(
  |petstore: &Petstore| petstore.raw(Method::POST, "/store/newsletter/unsubscribe").json(&json!({ "email": "grace@example.com" })),
  "application/x-www-form-urlencoded",
)]
#[tokio::test]
async fn a_request_the_server_cannot_decode_is_a_bad_request_the_service_never_sees(
  #[future(awt)] petstore: Petstore,
  #[case] request: fn(&Petstore) -> RequestBuilder,
  #[case] named: &str,
) -> anyhow::Result<()> {
  let reply = send(request(&petstore)).await?;

  let reason = reply.text();
  assert_eq!(reply.status, StatusCode::BAD_REQUEST, "{reason}");
  assert_eq!(reply.media_type.as_deref(), Some("text/plain"));
  assert!(reason.starts_with("Bad request: "), "{reason}");
  assert!(reason.contains(named), "{reason} names {named}");
  assert!(petstore.service.received().is_empty());
  Ok(())
}

fn recipes_without_null_members(received: &Received) -> bool {
  let Received::SendAssistantMessage(request) = received else {
    return false;
  };
  let [server::ContentBlock::Text(text)] = request.body.blocks.as_slice() else {
    return false;
  };
  text.recipes
    == Some(vec![server::RecipeList {
      ingredients: None,
      required: None,
      r#type: "object".to_string(),
      additional_properties: IndexMap::from([("servings".to_string(), json!(2))]),
    }])
}

#[rstest]
#[case::type_arrays_in_a_merge_patch(
  |petstore: &Petstore| petstore
    .raw(Method::PATCH, &format!("/pets/{PET_ID}"))
    .api_key()
    .header(CONTENT_TYPE, MERGE_PATCH)
    .body(r#"{"nickname":null,"weightKg":null}"#),
  |received: &Received| matches!(
    received,
    Received::UpdatePet(request) if request.body.nickname.is_none() && request.body.weight_kg.is_none()
  ),
)]
#[case::unions_with_a_null_branch(
  |petstore: &Petstore| petstore
    .raw(Method::PUT, "/assistant/settings")
    .api_key()
    .json(&json!({ "modelFilter": { "model": null }, "temperature": null, "citation": null })),
  |received: &Received| matches!(
    received,
    Received::UpdateAssistantSettings(request)
      if request.body.model_filter == Some(server::ModelFilter { model: None })
        && request.body.temperature.is_none()
        && request.body.citation.is_none()
  ),
)]
#[case::members_of_an_open_object(
  |petstore: &Petstore| petstore
    .raw(Method::POST, "/assistant/messages")
    .api_key()
    .json(&json!({
      "blocks": [{
        "type": "text",
        "text": "Dinner ideas",
        "recipes": [{ "type": "object", "ingredients": null, "required": null, "servings": 2 }],
      }],
    })),
  recipes_without_null_members,
)]
#[tokio::test]
async fn an_explicit_null_in_a_nullable_member_reaches_the_service_as_none(
  #[future(awt)] petstore: Petstore,
  #[case] request: fn(&Petstore) -> RequestBuilder,
  #[case] decoded: fn(&Received) -> bool,
) -> anyhow::Result<()> {
  seed_pet(&petstore);

  let reply = send(request(&petstore)).await?;

  assert!(reply.status.is_success(), "{} {}", reply.status, reply.text());
  let received = petstore.service.last_received();
  assert!(received.as_ref().is_some_and(decoded), "{received:?}");
  Ok(())
}

#[rstest]
#[tokio::test]
async fn schema_constraints_are_left_for_the_service_to_enforce(
  #[future(awt)] petstore: Petstore,
) -> anyhow::Result<()> {
  let reply = send(
    petstore
      .raw(Method::POST, "/pets")
      .api_key()
      .json(&json!({ "name": "Rex", "species": "dog", "microchip": "12345" })),
  )
  .await?;

  assert_eq!(reply.status, StatusCode::BAD_REQUEST);
  assert_eq!(reply.media_type.as_deref(), Some("application/json"));
  let error = serde_json::from_slice::<serde_json::Value>(&reply.body)?;
  assert_eq!(error["code"], 400);
  assert!(
    error["message"]
      .as_str()
      .is_some_and(|message| message.contains("microchip")),
    "{error}"
  );
  assert!(matches!(
    petstore.service.last_received(),
    Some(Received::CreatePet(request)) if request.body.microchip.as_deref() == Some("12345")
  ));
  Ok(())
}

fn percent_encoded(value: &str) -> String {
  value.bytes().fold(String::new(), |mut encoded, byte| {
    if byte.is_ascii_alphanumeric() {
      encoded.push(char::from(byte));
    } else {
      write!(encoded, "%{byte:02X}").expect("writing to a String succeeds");
    }
    encoded
  })
}

fn cookie_header(name: &str, value: &str, quoted: bool, neighbors: &[(String, String)], position: Index) -> String {
  let encoded = percent_encoded(value);
  let target = if quoted {
    format!("{name}=\"{encoded}\"")
  } else {
    format!("{name}={encoded}")
  };
  let mut pairs = neighbors
    .iter()
    .map(|(neighbor, value)| format!("{neighbor}={value}"))
    .collect::<Vec<_>>();
  pairs.insert(position.index(pairs.len() + 1), target);
  pairs.join("; ")
}

fn neighbor_cookies() -> impl Strategy<Value = Vec<(String, String)>> {
  collection::vec(
    (
      "[a-z]{1,8}".prop_filter("not the cookie under test", |name| name != "session"),
      "[A-Za-z0-9]{0,8}",
    ),
    0..3,
  )
}

proptest! {
  #![proptest_config(property_config(48))]

  #[test]
  fn any_cookie_value_arrives_percent_decoded_and_unquoted(
    value in "\\PC{0,16}".prop_filter("quotes delimit cookie values", |value| !value.contains('"')),
    quoted in any::<bool>(),
    neighbors in neighbor_cookies(),
    position in any::<Index>(),
  ) {
    static PETSTORE: LazyLock<Petstore> = LazyLock::new(start_shared);
    let petstore = &*PETSTORE;
    run_case(async || {
      petstore
        .raw(Method::GET, "/assistant/events")
        .api_key()
        .header(COOKIE, cookie_header("conversation", &value, quoted, &neighbors, position))
        .send()
        .await?;
      let Some(Received::ListAssistantEvents(parameter)) = petstore.service.last_received() else {
        return Err(TestCaseError::fail("the service did not receive listAssistantEvents"));
      };
      prop_assert_eq!(&parameter.cookie.conversation, &value);

      petstore
        .raw(Method::GET, "/staff/keys/key_1")
        .api_key()
        .header(COOKIE, cookie_header("session", &value, quoted, &neighbors, position))
        .send()
        .await?;
      let Some(Received::GetApiKey(credential)) = petstore.service.last_received() else {
        return Err(TestCaseError::fail("the service did not receive getApiKey"));
      };
      prop_assert_eq!(credential.credentials.session_cookie.expose_secret(), value.as_str());
      Ok(())
    })?;
  }
}

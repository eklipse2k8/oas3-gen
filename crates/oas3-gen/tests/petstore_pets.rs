//! End-to-end tests for the pet-record operations (tag `pets`) of the petstore document.
//!
//! Requests are built with the generated `petstore-client`, served by the generated
//! `petstore-server` router backed by the in-memory `PetstoreService`, and parsed by the
//! generated client. Each test asserts both directions: what the server extracted from the
//! wire (`Received`) and what the client made of the response. Raw `reqwest` requests and a
//! wiretap layered on the router cover what the client deliberately can't express:
//! spec-invalid input, missing headers or credentials, and the exact query strings, headers,
//! and media types on the wire.

mod support;

use std::sync::{
  Arc, Mutex,
  atomic::{AtomicU64, Ordering},
};

use chrono::NaiveDate;
use indexmap::{IndexMap, IndexSet};
use petstore_client as client;
use petstore_server as server;
use proptest::prelude::*;
use rstest::{fixture, rstest};
use secrecy::{ExposeSecret, SecretString};
use serde_json::json;
use support::{
  API_KEY, API_VERSION, COMPATIBILITY_DATE, Petstore, PetstoreService, Received, SESSION, STAFF_TOKEN, now, petstore,
  run_case, runtime, start_shared,
};
use uuid::Uuid;

/// The media type `updatePet` declares for its request body.
const MERGE_PATCH: &str = "application/merge-patch+json";
/// The media type of the `NotFound` and `422` problem responses.
const PROBLEM_JSON: &str = "application/problem+json";
/// The tag every pet seeded for search carries, so one `q` matches them all.
const RESIDENT: &str = "resident";
/// An id no test ever stores.
const UNKNOWN_PET: Uuid = Uuid::from_u128(0xDEAD_BEEF);

/// Numbers each pagination case so its pets carry a tag no other case uses.
static PAGINATION_RUN: AtomicU64 = AtomicU64::new(0);

type PetPage = client::WithHeaders<client::XNextAndXTotalCountAndXRequestIdHeaders, client::Pets>;
type ShownPet = client::WithHeaders<client::ETagAndXCacheHeaders, client::Pet>;

/// One request exactly as it reached the router, before the generated handlers parsed it.
#[derive(Debug, Clone)]
struct WireRequest {
  method: http::Method,
  uri: http::Uri,
  headers: http::HeaderMap,
}

impl WireRequest {
  /// The query string as sent, still percent-encoded.
  fn raw_query(&self) -> &str {
    self.uri.query().unwrap_or_default()
  }

  /// Every form-decoded value of the `name` query parameter, in wire order.
  fn query_values(&self, name: &str) -> Vec<String> {
    let url = reqwest::Url::parse(&format!("http://wire{}", self.uri)).expect("the request target parses");
    url
      .query_pairs()
      .filter(|(key, _)| key == name)
      .map(|(_, value)| value.into_owned())
      .collect::<Vec<_>>()
  }

  fn header(&self, name: http::HeaderName) -> Option<&str> {
    self.headers.get(name).and_then(|value| value.to_str().ok())
  }
}

/// A fresh store whose router records every raw request it receives.
struct Wiretapped {
  petstore: Petstore,
  requests: Arc<Mutex<Vec<WireRequest>>>,
}

impl Wiretapped {
  fn last(&self) -> WireRequest {
    self
      .requests
      .lock()
      .expect("the wiretap lock is never poisoned")
      .last()
      .cloned()
      .expect("the router received a request")
  }
}

/// A fresh store per test, served behind a layer that records each raw request.
#[fixture]
async fn wiretapped() -> Wiretapped {
  let service = PetstoreService::default();
  let requests = Arc::new(Mutex::new(vec![]));
  let recorder = Arc::clone(&requests);
  let router = server::router(service.clone()).layer(axum::middleware::from_fn(
    move |request: axum::extract::Request, next: axum::middleware::Next| {
      let wire_request = WireRequest {
        method: request.method().clone(),
        uri: request.uri().clone(),
        headers: request.headers().clone(),
      };
      recorder
        .lock()
        .expect("the wiretap lock is never poisoned")
        .push(wire_request);
      next.run(request)
    },
  ));
  let base_url = support::serve(router).await;
  let client = client::PetstoreClient::with_base_url(&base_url)
    .expect("the base URL parses")
    .with_api_key_auth(API_KEY);
  Wiretapped {
    petstore: Petstore {
      base_url,
      client,
      service,
    },
    requests,
  }
}

fn date(year: i32, month: u32, day: u32) -> NaiveDate {
  NaiveDate::from_ymd_opt(year, month, day).expect("a valid calendar date")
}

fn tag_set(tags: &[&str]) -> IndexSet<String> {
  tags.iter().copied().map(String::from).collect::<IndexSet<_>>()
}

fn numbered_tags(count: usize) -> IndexSet<String> {
  (0..count).map(|index| format!("tag-{index}")).collect::<IndexSet<_>>()
}

/// Tags in the order they were sent or stored.
fn ordered(tags: Option<&IndexSet<String>>) -> Option<Vec<&str>> {
  tags.map(|tags| tags.iter().map(String::as_str).collect::<Vec<_>>())
}

fn names(pets: &[client::Pet]) -> Vec<&str> {
  pets.iter().map(|pet| pet.name.as_str()).collect::<Vec<_>>()
}

fn exposed(secret: Option<&SecretString>) -> Option<&str> {
  secret.map(ExposeSecret::expose_secret)
}

fn wire(value: &impl serde::Serialize) -> serde_json::Value {
  serde_json::to_value(value).expect("the value serializes to JSON")
}

fn new_pet(name: &str, species: client::Species) -> client::NewPet {
  client::NewPet {
    name: name.to_string(),
    species,
    ..client::NewPet::default()
  }
}

/// A `NewPet` that sets every field to a valid value.
fn complete_new_pet() -> client::NewPet {
  client::NewPet {
    name: "Biscuit".to_string(),
    species: client::Species::Dog,
    nickname: Some("Bis".to_string()),
    birth_date: Some(date(2021, 4, 1)),
    weight_kg: Some(12.5),
    microchip: Some("985112345678901".to_string()),
    tags: Some(tag_set(&["friendly", "house-trained"])),
  }
}

/// A server-side pet with a deterministic id, for seeding the store directly.
fn seeded_pet(serial: u16, name: &str, species: server::Species) -> server::Pet {
  server::Pet {
    id: Uuid::from_u128(0x5EED_0000 + u128::from(serial)),
    name: name.to_string(),
    species,
    ..server::Pet::default()
  }
}

fn list_request() -> client::ListPetsRequest {
  client::ListPetsRequest::builder()
    .x_compatibility_date(COMPATIBILITY_DATE)
    .build()
    .expect("a valid listPets request")
}

fn show_request(pet_id: Uuid) -> client::ShowPetByIdRequest {
  client::ShowPetByIdRequest::builder()
    .pet_id(pet_id)
    .x_api_version(API_VERSION.to_string())
    .build()
    .expect("a valid showPetById request")
}

/// Creates `pet` through the client and returns the stored record.
async fn create(petstore: &Petstore, pet: client::NewPet) -> client::Pet {
  let request = client::CreatePetRequest::builder()
    .body(pet)
    .build()
    .expect("a valid NewPet");
  let response = petstore.client.create_pet(request).await.expect("createPet round trip");
  let client::ApiResponse::Created(client::WithHeaders { body, .. }) = response else {
    panic!("createPet answered {response:?}");
  };
  body
}

async fn list_pets(caller: &client::PetstoreClient, request: client::ListPetsRequest) -> PetPage {
  let response = caller.list_pets(request).await.expect("listPets round trip");
  let client::ApiResponse::Ok(page) = response else {
    panic!("listPets answered {response:?}");
  };
  page
}

async fn show(petstore: &Petstore, request: client::ShowPetByIdRequest) -> ShownPet {
  let response = petstore
    .client
    .show_pet_by_id(request)
    .await
    .expect("showPetById round trip");
  let client::ApiResponse::Ok(shown) = response else {
    panic!("showPetById answered {response:?}");
  };
  shown
}

async fn update(petstore: &Petstore, pet_id: Uuid, patch: client::PetPatch) -> client::Pet {
  let request = client::UpdatePetRequest::builder()
    .pet_id(pet_id)
    .body(patch)
    .build()
    .expect("a valid PetPatch");
  let response = petstore.client.update_pet(request).await.expect("updatePet round trip");
  let client::ApiResponse::Ok(pet) = response else {
    panic!("updatePet answered {response:?}");
  };
  pet
}

/// Sends `body` as a merge patch without going through the client's validation.
async fn patch_raw(petstore: &Petstore, pet_id: Uuid, body: &serde_json::Value) -> reqwest::Response {
  reqwest::Client::new()
    .patch(petstore.url(&format!("/pets/{pet_id}")))
    .header(client::X_API_KEY, API_KEY)
    .header(http::header::CONTENT_TYPE, MERGE_PATCH)
    .body(body.to_string())
    .send()
    .await
    .expect("the raw PATCH reaches the server")
}

/// Posts `body` as JSON to `path` without going through the client's validation.
async fn post_raw(petstore: &Petstore, path: &str, body: &serde_json::Value) -> reqwest::Response {
  reqwest::Client::new()
    .post(petstore.url(path))
    .header(client::X_API_KEY, API_KEY)
    .json(body)
    .send()
    .await
    .expect("the raw POST reaches the server")
}

/// The number of nodes in each generation of `lineage`, the pet itself first.
fn generation_sizes(lineage: &client::Lineage) -> Vec<usize> {
  let mut sizes = vec![];
  let mut generation = vec![lineage];
  while !generation.is_empty() {
    sizes.push(generation.len());
    generation = generation
      .into_iter()
      .flat_map(|node| node.parents.as_deref().unwrap_or_default())
      .collect::<Vec<_>>();
  }
  sizes
}

fn content_type(response: &reqwest::Response) -> Option<&str> {
  response
    .headers()
    .get(http::header::CONTENT_TYPE)
    .and_then(|value| value.to_str().ok())
}

#[rstest]
#[case::anonymous(false)]
#[case::with_api_key(true)]
#[tokio::test]
async fn listing_is_open_to_anonymous_callers_and_forwards_a_present_key(
  #[future(awt)] petstore: Petstore,
  #[case] sends_api_key: bool,
) {
  create(&petstore, new_pet("Biscuit", client::Species::Dog)).await;
  let caller = if sends_api_key {
    petstore.client.clone()
  } else {
    petstore.anonymous()
  };

  let page = list_pets(&caller, list_request()).await;

  assert_eq!(names(&page.body), ["Biscuit"]);
  let Some(Received::ListPets(received)) = petstore.service.last_received() else {
    panic!("the server did not record listPets");
  };
  assert_eq!(
    exposed(received.credentials.api_key_auth.as_ref()),
    sends_api_key.then_some(API_KEY)
  );
}

#[rstest]
#[case::zero(0)]
#[case::negative(-5)]
#[case::above_one_hundred(101)]
fn listing_rejects_a_limit_outside_1_to_100(#[case] limit: i32) {
  let error = client::ListPetsRequest::builder()
    .x_compatibility_date(COMPATIBILITY_DATE)
    .limit(limit)
    .build()
    .expect_err("a limit outside 1..=100 must not build");
  assert!(error.to_string().contains("limit"), "{error}");
}

#[rstest]
#[case::one_per_page(1)]
#[case::uneven_last_page(3)]
#[case::exact_fit(7)]
#[case::maximum_limit(100)]
#[tokio::test]
async fn listing_walks_every_page_through_x_next(#[future(awt)] petstore: Petstore, #[case] limit: i32) {
  let expected = ["Ace", "Bolt", "Cleo", "Dot", "Echo", "Fig", "Gus"];
  for name in expected {
    create(&petstore, new_pet(name, client::Species::Dog)).await;
  }
  let page_size = usize::try_from(limit).expect("a positive limit");

  let mut walked = vec![];
  let mut cursor = None;
  loop {
    let request = client::ListPetsRequest::builder()
      .x_compatibility_date(COMPATIBILITY_DATE)
      .limit(limit)
      .maybe_cursor(cursor.clone())
      .build()
      .expect("a valid listPets request");
    let page = list_pets(&petstore.client, request).await;

    let Some(Received::ListPets(received)) = petstore.service.last_received() else {
      panic!("the server did not record listPets");
    };
    assert_eq!(received.query.limit, Some(limit));
    assert_eq!(received.query.cursor, cursor);
    assert_eq!(page.headers.x_total_count, Some(7));
    assert!(
      page.body.len() <= page_size,
      "{} pets exceed limit {limit}",
      page.body.len()
    );
    walked.extend(page.body.into_iter().map(|pet| pet.name));
    assert!(walked.len() <= expected.len(), "the cursors revisit pets: {walked:?}");
    let Some(next) = page.headers.x_next else {
      break;
    };
    cursor = Some(next);
  }

  assert_eq!(walked, expected);
}

#[rstest]
#[case::sent(Some(Uuid::from_u128(0x0192_f0c1_7a6e_7c4e_9d2a_5b1e_8f3c_2a10)))]
#[case::omitted(None)]
#[tokio::test]
async fn listing_echoes_the_request_id(#[future(awt)] petstore: Petstore, #[case] request_id: Option<Uuid>) {
  let request = client::ListPetsRequest::builder()
    .x_compatibility_date(COMPATIBILITY_DATE)
    .maybe_x_request_id(request_id)
    .build()
    .expect("a valid listPets request");

  let page = list_pets(&petstore.client, request).await;

  assert_eq!(page.headers.x_request_id, request_id);
  let Some(Received::ListPets(received)) = petstore.service.last_received() else {
    panic!("the server did not record listPets");
  };
  assert_eq!(received.header.x_request_id, request_id);
}

#[rstest]
#[case::available(client::PetStatus::Available, server::PetStatus::Available, &["Ace"])]
#[case::pending(client::PetStatus::Pending, server::PetStatus::Pending, &["Bolt"])]
#[case::adopted(client::PetStatus::Adopted, server::PetStatus::Adopted, &["Cleo"])]
#[tokio::test]
async fn listing_filters_by_status(
  #[future(awt)] petstore: Petstore,
  #[case] status: client::PetStatus,
  #[case] received_status: server::PetStatus,
  #[case] expected: &[&str],
) {
  for (serial, name, pet_status) in [
    (1, "Ace", server::PetStatus::Available),
    (2, "Bolt", server::PetStatus::Pending),
    (3, "Cleo", server::PetStatus::Adopted),
  ] {
    petstore.service.insert_pet(server::Pet {
      status: Some(pet_status),
      ..seeded_pet(serial, name, server::Species::Dog)
    });
  }
  let request = client::ListPetsRequest::builder()
    .x_compatibility_date(COMPATIBILITY_DATE)
    .status(status)
    .build()
    .expect("a valid listPets request");

  let page = list_pets(&petstore.client, request).await;

  assert_eq!(names(&page.body), expected);
  let Some(Received::ListPets(received)) = petstore.service.last_received() else {
    panic!("the server did not record listPets");
  };
  assert_eq!(received.query.status, Some(received_status));
}

#[rstest]
#[tokio::test]
async fn listing_sends_tags_as_one_comma_joined_value(#[future(awt)] wiretapped: Wiretapped) {
  let petstore = &wiretapped.petstore;
  for (name, tags) in [
    ("Ace", ["friendly", "small"].as_slice()),
    ("Bolt", ["friendly"].as_slice()),
    ("Cleo", ["loud", "small", "friendly"].as_slice()),
  ] {
    let pet = client::NewPet {
      tags: Some(tag_set(tags)),
      ..new_pet(name, client::Species::Dog)
    };
    create(petstore, pet).await;
  }
  let request = client::ListPetsRequest::builder()
    .x_compatibility_date(COMPATIBILITY_DATE)
    .tags(vec!["friendly".to_string(), "small".to_string()])
    .build()
    .expect("a valid listPets request");

  let page = list_pets(&petstore.client, request).await;

  assert_eq!(names(&page.body), ["Ace", "Cleo"]);
  let wire = wiretapped.last();
  assert_eq!(
    wire.query_values("tags"),
    ["friendly,small"],
    "form with explode=false sends one `tags` pair, got `{}`",
    wire.raw_query()
  );
  let Some(Received::ListPets(received)) = petstore.service.last_received() else {
    panic!("the server did not record listPets");
  };
  assert_eq!(
    received.query.tags,
    Some(vec!["friendly".to_string(), "small".to_string()])
  );
}

#[rstest]
#[case::ascending(
  client::ListPetsRequestHeaderXSortOrder::Asc,
  server::ListPetsRequestHeaderXSortOrder::Asc,
  &["Ace", "Bolt", "Cleo"],
)]
#[case::descending(
  client::ListPetsRequestHeaderXSortOrder::Desc,
  server::ListPetsRequestHeaderXSortOrder::Desc,
  &["Cleo", "Bolt", "Ace"],
)]
#[tokio::test]
async fn listing_sorts_by_the_x_sort_order_header(
  #[future(awt)] petstore: Petstore,
  #[case] order: client::ListPetsRequestHeaderXSortOrder,
  #[case] received_order: server::ListPetsRequestHeaderXSortOrder,
  #[case] expected: &[&str],
) {
  for name in ["Bolt", "Cleo", "Ace"] {
    create(&petstore, new_pet(name, client::Species::Dog)).await;
  }
  let request = client::ListPetsRequest::builder()
    .x_compatibility_date(COMPATIBILITY_DATE)
    .x_sort_order(order)
    .build()
    .expect("a valid listPets request");

  let page = list_pets(&petstore.client, request).await;

  assert_eq!(names(&page.body), expected);
  let Some(Received::ListPets(received)) = petstore.service.last_received() else {
    panic!("the server did not record listPets");
  };
  assert_eq!(received.header.x_sort_order, Some(received_order));
}

#[rstest]
#[tokio::test]
async fn listing_only_includes_species_named_in_x_only(#[future(awt)] wiretapped: Wiretapped) {
  let petstore = &wiretapped.petstore;
  for (name, species) in [
    ("Ace", client::Species::Dog),
    ("Bolt", client::Species::Cat),
    ("Cleo", client::Species::Bird),
    ("Dory", client::Species::Fish),
  ] {
    create(petstore, new_pet(name, species)).await;
  }
  let request = client::ListPetsRequest::builder()
    .x_compatibility_date(COMPATIBILITY_DATE)
    .x_only(vec![client::Species::Cat, client::Species::Bird])
    .build()
    .expect("a valid listPets request");

  let page = list_pets(&petstore.client, request).await;

  assert_eq!(names(&page.body), ["Bolt", "Cleo"]);
  assert_eq!(wiretapped.last().header(client::X_ONLY), Some("cat,bird"));
  let Some(Received::ListPets(received)) = petstore.service.last_received() else {
    panic!("the server did not record listPets");
  };
  assert_eq!(
    received.header.x_only,
    Some(vec![server::Species::Cat, server::Species::Bird])
  );
}

#[rstest]
#[tokio::test]
async fn listing_with_another_compatibility_date_is_the_default_error(#[future(awt)] petstore: Petstore) {
  let unsupported = date(2025, 1, 1);
  let request = client::ListPetsRequest::builder()
    .x_compatibility_date(unsupported)
    .build()
    .expect("a valid listPets request");

  let response = petstore.client.list_pets(request).await.expect("listPets round trip");

  let client::ApiResponse::Unknown(status, error) = response else {
    panic!("listPets answered {response:?}");
  };
  assert_eq!(status, http::StatusCode::BAD_REQUEST);
  assert_eq!(error.code, 400);
  let Some(Received::ListPets(received)) = petstore.service.last_received() else {
    panic!("the server did not record listPets");
  };
  assert_eq!(received.header.x_compatibility_date, unsupported);
}

#[rstest]
#[tokio::test]
async fn listing_without_the_compatibility_date_header_is_rejected(#[future(awt)] petstore: Petstore) {
  let response = reqwest::Client::new()
    .get(petstore.url("/pets"))
    .send()
    .await
    .expect("the raw GET reaches the server");

  assert_eq!(response.status(), http::StatusCode::BAD_REQUEST);
  let body = response.text().await.expect("a text body");
  assert!(body.contains("x-compatibility-date"), "{body}");
  assert!(petstore.service.received().is_empty());
}

#[rstest]
#[tokio::test]
async fn creating_a_pet_answers_201_with_its_location_and_record(#[future(awt)] petstore: Petstore) {
  let pet = complete_new_pet();
  let request = client::CreatePetRequest::builder()
    .body(pet.clone())
    .build()
    .expect("a valid NewPet");

  let response = petstore.client.create_pet(request).await.expect("createPet round trip");

  let client::ApiResponse::Created(client::WithHeaders { headers, body: created }) = response else {
    panic!("createPet answered {response:?}");
  };
  assert_eq!(headers.location, format!("/pets/{}", created.id));
  assert_eq!(created.name, pet.name);
  assert_eq!(created.species, pet.species);
  assert_eq!(created.nickname, pet.nickname);
  assert_eq!(created.birth_date, pet.birth_date);
  assert_eq!(created.weight_kg, pet.weight_kg);
  assert_eq!(created.microchip, pet.microchip);
  assert_eq!(ordered(created.tags.as_ref()), ordered(pet.tags.as_ref()));
  assert_eq!(created.status, Some(client::PetStatus::Available));
  assert_eq!(created.created_at, Some(now()));

  let Some(Received::CreatePet(received)) = petstore.service.last_received() else {
    panic!("the server did not record createPet");
  };
  assert_eq!(received.credentials.api_key_auth.expose_secret(), API_KEY);
  assert_eq!(received.body.name, pet.name);
  assert_eq!(received.body.species, server::Species::Dog);
  assert_eq!(received.body.nickname, pet.nickname);
  assert_eq!(received.body.birth_date, pet.birth_date);
  assert_eq!(received.body.weight_kg, pet.weight_kg);
  assert_eq!(received.body.microchip, pet.microchip);
  assert_eq!(ordered(received.body.tags.as_ref()), ordered(pet.tags.as_ref()));
}

#[rstest]
#[case::empty_name(client::NewPet { name: String::new(), ..complete_new_pet() }, "name")]
#[case::name_over_64_chars(client::NewPet { name: "n".repeat(65), ..complete_new_pet() }, "name")]
#[case::short_microchip(client::NewPet { microchip: Some("12345".to_string()), ..complete_new_pet() }, "microchip")]
#[case::long_microchip(
  client::NewPet { microchip: Some("9851123456789012".to_string()), ..complete_new_pet() },
  "microchip",
)]
#[case::non_digit_microchip(
  client::NewPet { microchip: Some("98511234567890X".to_string()), ..complete_new_pet() },
  "microchip",
)]
#[case::zero_weight(client::NewPet { weight_kg: Some(0.0), ..complete_new_pet() }, "weight")]
#[case::negative_weight(client::NewPet { weight_kg: Some(-3.0), ..complete_new_pet() }, "weight")]
#[case::weight_over_500(client::NewPet { weight_kg: Some(500.5), ..complete_new_pet() }, "weight")]
#[case::eleven_tags(client::NewPet { tags: Some(numbered_tags(11)), ..complete_new_pet() }, "tags")]
fn new_pet_builder_rejects_spec_invalid_bodies(#[case] pet: client::NewPet, #[case] field: &str) {
  let error = client::CreatePetRequest::builder()
    .body(pet)
    .build()
    .expect_err("a spec-invalid NewPet must not build");
  assert!(error.to_string().contains(field), "{error}");
}

#[rstest]
#[case::one_char_name(client::NewPet { name: "A".to_string(), ..complete_new_pet() })]
#[case::sixty_four_char_name(client::NewPet { name: "n".repeat(64), ..complete_new_pet() })]
#[case::smallest_weight(client::NewPet { weight_kg: Some(0.001), ..complete_new_pet() })]
#[case::maximum_weight(client::NewPet { weight_kg: Some(500.0), ..complete_new_pet() })]
#[case::ten_tags(client::NewPet { tags: Some(numbered_tags(10)), ..complete_new_pet() })]
#[case::required_fields_only(new_pet("Biscuit", client::Species::SmallMammal))]
#[tokio::test]
async fn new_pet_at_the_spec_boundaries_is_created(#[future(awt)] petstore: Petstore, #[case] pet: client::NewPet) {
  let expected_tags = pet.tags.clone();
  let expected_weight = pet.weight_kg;
  let expected_name = pet.name.clone();

  let created = create(&petstore, pet).await;

  assert_eq!(created.name, expected_name);
  assert_eq!(created.weight_kg, expected_weight);
  assert_eq!(ordered(created.tags.as_ref()), ordered(expected_tags.as_ref()));
}

#[rstest]
#[case::empty_name(json!({ "name": "", "species": "dog" }))]
#[case::name_over_64_chars(json!({ "name": "n".repeat(65), "species": "dog" }))]
#[case::short_microchip(json!({ "name": "Rex", "species": "dog", "microchip": "12345" }))]
#[case::non_digit_microchip(json!({ "name": "Rex", "species": "dog", "microchip": "98511234567890X" }))]
#[case::zero_weight(json!({ "name": "Rex", "species": "dog", "weightKg": 0 }))]
#[case::weight_over_500(json!({ "name": "Rex", "species": "dog", "weightKg": 500.5 }))]
#[case::eleven_tags(json!({ "name": "Rex", "species": "dog", "tags": numbered_tags(11) }))]
#[tokio::test]
async fn server_answers_bad_request_for_invalid_bodies_that_bypass_the_client(
  #[future(awt)] petstore: Petstore,
  #[case] body: serde_json::Value,
) {
  let response = post_raw(&petstore, "/pets", &body).await;

  let parsed = client::CreatePetRequest::parse_response(response)
    .await
    .expect("a declared createPet response");
  let client::ApiResponse::BadRequest(error) = parsed else {
    panic!("{body} was answered with {parsed:?}");
  };
  assert_eq!(error.code, 400);
  assert!(petstore.service.with_state(|state| state.pets.is_empty()));
}

/// `uniqueItems: true` makes a repeated tag invalid, so the generated server rejects the body
/// before the service runs, naming the duplicate.
#[rstest]
#[tokio::test]
async fn duplicate_tags_that_bypass_the_client_are_rejected(#[future(awt)] petstore: Petstore) {
  let body = json!({ "name": "Rex", "species": "dog", "tags": ["calm", "shy", "calm"] });

  let response = post_raw(&petstore, "/pets", &body).await;

  assert_eq!(response.status(), reqwest::StatusCode::BAD_REQUEST);
  let reason = response.text().await.expect("a plain-text reason");
  assert!(reason.starts_with("Bad request: "), "{reason}");
  assert!(reason.contains("tags"), "{reason}");
  assert!(
    !matches!(petstore.service.last_received(), Some(Received::CreatePet(_))),
    "the service never sees the body"
  );
}

/// Every tag in a set the client built arrives, in order, and a set can't repeat a tag.
#[rstest]
#[case::one(&["calm"])]
#[case::several(&["calm", "shy", "good-boy"])]
#[tokio::test]
async fn unique_tags_round_trip_in_order(#[future(awt)] petstore: Petstore, #[case] tags: &[&str]) {
  let pet = client::NewPet {
    tags: Some(tag_set(tags)),
    ..new_pet("Rex", client::Species::Dog)
  };

  let created = create(&petstore, pet).await;

  assert_eq!(ordered(created.tags.as_ref()), Some(tags.to_vec()));
}

#[rstest]
#[tokio::test]
async fn describing_pets_lists_the_allowed_methods_without_credentials(#[future(awt)] wiretapped: Wiretapped) {
  let response = wiretapped
    .petstore
    .client
    .describe_pets(client::DescribePetsRequest {})
    .await
    .expect("describePets round trip");

  let client::ApiResponse::NoContent(client::WithHeaders { headers, body: () }) = response else {
    panic!("describePets answered {response:?}");
  };
  assert_eq!(headers.allow, "GET, POST, OPTIONS");
  let wire = wiretapped.last();
  assert_eq!(wire.method, http::Method::OPTIONS);
  assert_eq!(
    wire.header(client::X_API_KEY),
    None,
    "`security: []` sends no credentials"
  );
  assert!(matches!(
    wiretapped.petstore.service.last_received(),
    Some(Received::DescribePets(_))
  ));
}

#[rstest]
#[tokio::test]
async fn searching_delivers_every_query_parameter(#[future(awt)] wiretapped: Wiretapped) {
  let text = "salt & pepper = 100% cute+";
  let request = client::SearchPetsRequest::builder()
    .q(text.to_string())
    .filter(json!("coat:short"))
    .species(vec!["dog".to_string(), "cat".to_string()])
    .colors(vec!["brown".to_string(), "white".to_string()])
    .min_weight_kg(2.5)
    .vaccinated(true)
    .born_after(date(2020, 1, 31))
    .build()
    .expect("a valid searchPets request");

  let response = wiretapped
    .petstore
    .client
    .search_pets(request)
    .await
    .expect("searchPets round trip");

  assert!(matches!(response, client::ApiResponse::Ok(_)), "{response:?}");
  let wire = wiretapped.last();
  assert_eq!(wire.query_values("q"), [text]);
  assert_eq!(wire.query_values("filter"), ["coat:short"]);
  assert_eq!(wire.query_values("species"), ["dog cat"]);
  assert_eq!(wire.query_values("colors"), ["brown|white"]);
  assert_eq!(wire.query_values("minWeightKg"), ["2.5"]);
  assert_eq!(wire.query_values("vaccinated"), ["true"]);
  assert_eq!(wire.query_values("bornAfter"), ["2020-01-31"]);

  let Some(Received::SearchPets(received)) = wiretapped.petstore.service.last_received() else {
    panic!("the server did not record searchPets");
  };
  let query = received.query;
  assert_eq!(query.q, text);
  assert_eq!(query.filter, Some(json!("coat:short")));
  assert_eq!(query.species, Some(vec!["dog".to_string(), "cat".to_string()]));
  assert_eq!(query.colors, Some(vec!["brown".to_string(), "white".to_string()]));
  assert_eq!(query.min_weight_kg, Some(2.5));
  assert_eq!(query.vaccinated, Some(true));
  assert_eq!(query.born_after, Some(date(2020, 1, 31)));
}

/// Arrays in `OpenAPI`'s default `form` style explode into one pair per item: strings, enum
/// values, and integers alike, in order, and each arrives as the array the client built.
#[rstest]
#[tokio::test]
async fn searching_explodes_array_parameters_into_repeated_pairs(#[future(awt)] wiretapped: Wiretapped) {
  let request = client::SearchPetsRequest::builder()
    .q(RESIDENT.to_string())
    .tags(vec!["calm".to_string(), "salt & pepper".to_string()])
    .statuses(vec![client::PetStatus::Pending, client::PetStatus::Adopted])
    .born_in(vec![2019, 2022])
    .build()
    .expect("a valid searchPets request");

  let response = wiretapped
    .petstore
    .client
    .search_pets(request)
    .await
    .expect("searchPets round trip");

  assert!(matches!(response, client::ApiResponse::Ok(_)), "{response:?}");
  let wire = wiretapped.last();
  assert_eq!(wire.query_values("tags"), ["calm", "salt & pepper"]);
  assert_eq!(wire.query_values("statuses"), ["pending", "adopted"]);
  assert_eq!(wire.query_values("bornIn"), ["2019", "2022"]);
  assert!(
    wire.raw_query().contains("bornIn=2019&bornIn=2022"),
    "each year is its own pair: {}",
    wire.raw_query()
  );

  let Some(Received::SearchPets(received)) = wiretapped.petstore.service.last_received() else {
    panic!("the server did not record searchPets");
  };
  assert_eq!(
    received.query.tags,
    Some(vec!["calm".to_string(), "salt & pepper".to_string()])
  );
  assert_eq!(
    received.query.statuses,
    Some(vec![server::PetStatus::Pending, server::PetStatus::Adopted])
  );
  assert_eq!(received.query.born_in, Some(vec![2019, 2022]));
}

/// The client doesn't apply `allowReserved`: reqwest form-encodes reserved characters like any
/// other query value, and the server decodes them back into the text the client sent.
#[rstest]
#[case::path_like("golden/retriever:mix@shelter", "q=golden%2Fretriever%3Amix%40shelter")]
#[case::sub_delimiters("!$()*,;'", "q=%21%24%28%29*%2C%3B%27")]
#[case::pair_delimiters("salt & pepper = 100% cute+", "q=salt+%26+pepper+%3D+100%25+cute%2B")]
#[case::fragment_and_brackets("#tag [1]", "q=%23tag+%5B1%5D")]
#[tokio::test]
async fn search_text_with_reserved_characters_arrives_intact(
  #[future(awt)] wiretapped: Wiretapped,
  #[case] text: &str,
  #[case] raw_query: &str,
) {
  let request = client::SearchPetsRequest::builder()
    .q(text.to_string())
    .build()
    .expect("a valid searchPets request");

  let response = wiretapped
    .petstore
    .client
    .search_pets(request)
    .await
    .expect("searchPets round trip");

  assert!(matches!(response, client::ApiResponse::Ok(_)), "{response:?}");
  assert_eq!(wiretapped.last().raw_query(), raw_query);
  let Some(Received::SearchPets(received)) = wiretapped.petstore.service.last_received() else {
    panic!("the server did not record searchPets");
  };
  assert_eq!(received.query.q, text);
}

/// Requests `getPetCard` for `pet_id` with the given cookie parameters.
fn card_request(
  pet_id: Uuid,
  theme: Option<client::GetPetCardRequestCookieCardTheme>,
  recently_viewed: Option<Vec<Uuid>>,
) -> client::GetPetCardRequest {
  client::GetPetCardRequest::builder()
    .pet_id(pet_id)
    .maybe_card_theme(theme)
    .maybe_recently_viewed(recently_viewed)
    .build()
    .expect("a valid getPetCard request")
}

/// Cookie parameters travel in one `Cookie` header, an array's items comma-joined and each
/// value then percent-encoded by the `cookie` crate, so the commas travel as `%2C`. They
/// arrive as the typed values the client set.
#[rstest]
#[case::none(None, None, None)]
#[case::theme(Some(client::GetPetCardRequestCookieCardTheme::Dark), None, Some("cardTheme=dark"))]
#[case::hyphenated_theme(
  Some(client::GetPetCardRequestCookieCardTheme::HighContrast),
  None,
  Some("cardTheme=high-contrast")
)]
#[case::recently_viewed(
  None,
  Some(vec![Uuid::from_u128(1), Uuid::from_u128(2)]),
  Some("recentlyViewed=00000000-0000-0000-0000-000000000001%2C00000000-0000-0000-0000-000000000002")
)]
#[case::both(
  Some(client::GetPetCardRequestCookieCardTheme::Light),
  Some(vec![Uuid::from_u128(3)]),
  Some("cardTheme=light; recentlyViewed=00000000-0000-0000-0000-000000000003")
)]
#[tokio::test]
async fn card_cookie_parameters_travel_in_the_cookie_header(
  #[future(awt)] wiretapped: Wiretapped,
  #[case] theme: Option<client::GetPetCardRequestCookieCardTheme>,
  #[case] recently_viewed: Option<Vec<Uuid>>,
  #[case] cookie_header: Option<&str>,
) {
  let petstore = &wiretapped.petstore;
  let pet = create(petstore, new_pet("Biscuit", client::Species::Dog)).await;

  let response = petstore
    .client
    .get_pet_card(card_request(pet.id, theme.clone(), recently_viewed.clone()))
    .await
    .expect("getPetCard round trip");

  assert!(matches!(response, client::ApiResponse::Ok(_)), "{response:?}");
  assert_eq!(wiretapped.last().header(http::header::COOKIE), cookie_header);
  let Some(Received::GetPetCard(received)) = petstore.service.last_received() else {
    panic!("the server did not record getPetCard");
  };
  assert_eq!(
    received.cookie.card_theme.map(|theme| theme.to_string()),
    theme.map(|theme| theme.to_string())
  );
  assert_eq!(received.cookie.recently_viewed, recently_viewed);
}

/// A request that carries its own `Cookie` header keeps the client's cookie store from adding
/// its cookies, so the generated client merges the stored session into the header itself.
#[rstest]
#[tokio::test]
async fn card_cookies_keep_the_stored_session_cookie(#[future(awt)] wiretapped: Wiretapped) {
  let petstore = &wiretapped.petstore;
  let pet = create(petstore, new_pet("Biscuit", client::Species::Dog)).await;
  let client = client::PetstoreClient::with_base_url(&petstore.base_url)
    .expect("the base URL parses")
    .with_api_key_auth(API_KEY)
    .with_session_cookie(SESSION);

  client
    .get_pet_card(card_request(
      pet.id,
      Some(client::GetPetCardRequestCookieCardTheme::Dark),
      None,
    ))
    .await
    .expect("getPetCard round trip");

  assert_eq!(
    wiretapped.last().header(http::header::COOKIE),
    Some(format!("session={SESSION}; cardTheme=dark").as_str())
  );
}

/// A cookie value that doesn't parse as its declared type is rejected before the service runs.
#[rstest]
#[case::unknown_theme("cardTheme=sepia", "invalid value for cookie `cardTheme`")]
#[case::bad_uuid("recentlyViewed=not-a-uuid", "invalid value for cookie `recentlyViewed`")]
#[tokio::test]
async fn invalid_card_cookies_are_bad_requests(
  #[future(awt)] petstore: Petstore,
  #[case] cookie: &str,
  #[case] reason: &str,
) {
  let pet = create(&petstore, new_pet("Biscuit", client::Species::Dog)).await;

  let response = reqwest::Client::new()
    .get(petstore.url(&format!("/pets/{}/card", pet.id)))
    .header(client::X_API_KEY, API_KEY)
    .header(reqwest::header::COOKIE, cookie)
    .send()
    .await
    .expect("the raw request completes");

  assert_eq!(response.status(), reqwest::StatusCode::BAD_REQUEST);
  let text = response.text().await.expect("a plain-text reason");
  assert!(text.contains(reason), "{text}");
  assert!(!matches!(
    petstore.service.last_received(),
    Some(Received::GetPetCard(_))
  ));
}

/// The query every seeded search pet matches.
fn resident() -> client::SearchPetsRequestQuery {
  client::SearchPetsRequestQuery {
    q: RESIDENT.to_string(),
    ..client::SearchPetsRequestQuery::default()
  }
}

/// Seeds five pets, each tagged [`RESIDENT`], that differ in every searchable field.
fn seed_search_pets(service: &PetstoreService) {
  let pets = [
    server::Pet {
      tags: Some(tag_set(&[RESIDENT, "brown"])),
      weight_kg: Some(12.0),
      birth_date: Some(date(2019, 4, 1)),
      status: Some(server::PetStatus::Available),
      ..seeded_pet(1, "Biscuit", server::Species::Dog)
    },
    server::Pet {
      tags: Some(tag_set(&[RESIDENT, "white"])),
      weight_kg: Some(4.0),
      birth_date: Some(date(2021, 6, 15)),
      status: Some(server::PetStatus::Pending),
      ..seeded_pet(2, "Mittens", server::Species::Cat)
    },
    server::Pet {
      tags: Some(tag_set(&[RESIDENT, "brown", "white"])),
      weight_kg: Some(5.5),
      birth_date: Some(date(2022, 2, 2)),
      status: Some(server::PetStatus::Available),
      ..seeded_pet(3, "Tiger", server::Species::Cat)
    },
    server::Pet {
      tags: Some(tag_set(&[RESIDENT, "green"])),
      status: Some(server::PetStatus::Adopted),
      ..seeded_pet(4, "Kiwi", server::Species::Bird)
    },
    server::Pet {
      nickname: Some("Bubbles".to_string()),
      tags: Some(tag_set(&[RESIDENT])),
      ..seeded_pet(5, "Nemo", server::Species::Fish)
    },
  ];
  let vaccinated = [pets[0].id, pets[2].id];
  for pet in pets {
    service.insert_pet(pet);
  }
  service.with_state(|state| {
    for id in vaccinated {
      let record = server::VaccineRecord {
        vaccine: "rabies".to_string(),
        date_administered: date(2026, 1, 10),
        ..server::VaccineRecord::default()
      };
      state.vaccinations.insert(id, vec![record]);
    }
  });
}

#[rstest]
#[case::name_ignoring_case(client::SearchPetsRequestQuery { q: "BIS".to_string(), ..resident() }, &["Biscuit"])]
#[case::nickname(client::SearchPetsRequestQuery { q: "bubbles".to_string(), ..resident() }, &["Nemo"])]
#[case::every_resident(resident(), &["Biscuit", "Mittens", "Tiger", "Kiwi", "Nemo"])]
#[case::one_species(
  client::SearchPetsRequestQuery { species: Some(vec!["cat".to_string()]), ..resident() },
  &["Mittens", "Tiger"],
)]
#[case::several_species(
  client::SearchPetsRequestQuery { species: Some(vec!["cat".to_string(), "bird".to_string()]), ..resident() },
  &["Mittens", "Tiger", "Kiwi"],
)]
#[case::one_color(
  client::SearchPetsRequestQuery { colors: Some(vec!["brown".to_string()]), ..resident() },
  &["Biscuit", "Tiger"],
)]
#[case::any_of_several_colors(
  client::SearchPetsRequestQuery { colors: Some(vec!["green".to_string(), "white".to_string()]), ..resident() },
  &["Mittens", "Tiger", "Kiwi"],
)]
#[case::minimum_weight(
  client::SearchPetsRequestQuery { min_weight_kg: Some(5.5), ..resident() },
  &["Biscuit", "Tiger"],
)]
#[case::vaccinated(
  client::SearchPetsRequestQuery { vaccinated: Some(true), ..resident() },
  &["Biscuit", "Tiger"],
)]
#[case::unvaccinated(
  client::SearchPetsRequestQuery { vaccinated: Some(false), ..resident() },
  &["Mittens", "Kiwi", "Nemo"],
)]
#[case::born_after(
  client::SearchPetsRequestQuery { born_after: Some(date(2021, 1, 1)), ..resident() },
  &["Mittens", "Tiger"],
)]
#[case::any_of_several_tags(
  client::SearchPetsRequestQuery { tags: Some(vec!["green".to_string(), "white".to_string()]), ..resident() },
  &["Mittens", "Tiger", "Kiwi"],
)]
#[case::one_status(
  client::SearchPetsRequestQuery { statuses: Some(vec![client::PetStatus::Pending]), ..resident() },
  &["Mittens"],
)]
#[case::any_of_several_statuses(
  client::SearchPetsRequestQuery {
    statuses: Some(vec![client::PetStatus::Pending, client::PetStatus::Adopted]),
    ..resident()
  },
  &["Mittens", "Kiwi"],
)]
#[case::born_in_any_of_several_years(
  client::SearchPetsRequestQuery { born_in: Some(vec![2019, 2022]), ..resident() },
  &["Biscuit", "Tiger"],
)]
#[case::species_and_color(
  client::SearchPetsRequestQuery {
    species: Some(vec!["cat".to_string()]),
    colors: Some(vec!["brown".to_string()]),
    ..resident()
  },
  &["Tiger"],
)]
#[tokio::test]
async fn searching_filters_by_every_criterion(
  #[future(awt)] petstore: Petstore,
  #[case] query: client::SearchPetsRequestQuery,
  #[case] expected: &[&str],
) {
  seed_search_pets(&petstore.service);

  let response = petstore
    .client
    .search_pets(client::SearchPetsRequest { query })
    .await
    .expect("searchPets round trip");

  let client::ApiResponse::Ok(results) = response else {
    panic!("searchPets answered {response:?}");
  };
  assert_eq!(names(&results.items), expected);
  assert_eq!(results.total, i64::try_from(expected.len()).expect("a small count"));
}

#[rstest]
#[tokio::test]
async fn showing_a_pet_reports_its_etag_and_a_cache_miss_then_hit(#[future(awt)] petstore: Petstore) {
  let created = create(&petstore, complete_new_pet()).await;

  let first = show(&petstore, show_request(created.id)).await;
  let second = show(&petstore, show_request(created.id)).await;

  assert_eq!(first.body, created);
  assert_eq!(second.body, created);
  assert_eq!(
    first.headers.x_cache,
    Some(client::ShowPetByIdResponseHeaderXCache::Miss)
  );
  assert_eq!(
    second.headers.x_cache,
    Some(client::ShowPetByIdResponseHeaderXCache::Hit)
  );
  assert!(first.headers.etag.is_some());
  assert_eq!(first.headers.etag, second.headers.etag);
  let Some(Received::ShowPetById(received)) = petstore.service.last_received() else {
    panic!("the server did not record showPetById");
  };
  assert_eq!(received.path.pet_id, created.id);
  assert_eq!(received.header.x_api_version, API_VERSION);
  assert_eq!(received.header.if_none_match, None);
}

#[rstest]
#[tokio::test]
async fn showing_with_the_current_etag_is_not_modified(#[future(awt)] petstore: Petstore) {
  let created = create(&petstore, new_pet("Biscuit", client::Species::Dog)).await;
  let etag = show(&petstore, show_request(created.id))
    .await
    .headers
    .etag
    .expect("showPetById sends an ETag");
  let conditional = |etag: &str| {
    client::ShowPetByIdRequest::builder()
      .pet_id(created.id)
      .x_api_version(API_VERSION.to_string())
      .if_none_match(etag.to_string())
      .build()
      .expect("a valid conditional showPetById request")
  };

  let unchanged = petstore
    .client
    .show_pet_by_id(conditional(&etag))
    .await
    .expect("showPetById round trip");

  assert!(matches!(unchanged, client::ApiResponse::NotModified), "{unchanged:?}");
  let Some(Received::ShowPetById(received)) = petstore.service.last_received() else {
    panic!("the server did not record showPetById");
  };
  assert_eq!(received.header.if_none_match.as_deref(), Some(etag.as_str()));

  update(
    &petstore,
    created.id,
    client::PetPatch::builder().name("Sir Biscuit".to_string()).build(),
  )
  .await;
  let changed = show(&petstore, conditional(&etag)).await;
  assert_eq!(changed.body.name, "Sir Biscuit");
  assert_ne!(changed.headers.etag.as_deref(), Some(etag.as_str()));
}

#[rstest]
#[tokio::test]
async fn showing_a_missing_pet_is_not_found_with_a_problem(#[future(awt)] petstore: Petstore) {
  let response = petstore
    .client
    .show_pet_by_id(show_request(UNKNOWN_PET))
    .await
    .expect("showPetById round trip");

  let client::ApiResponse::NotFound(client::ErrorOrProblem::Problem(problem)) = response else {
    panic!("showPetById answered {response:?}");
  };
  assert_eq!(problem.status, Some(404));
  assert_eq!(problem.instance, Some(format!("/pets/{UNKNOWN_PET}")));
}

#[rstest]
#[tokio::test]
async fn a_missing_pet_is_sent_as_problem_json(#[future(awt)] petstore: Petstore) {
  let response = reqwest::Client::new()
    .get(petstore.url(&format!("/pets/{UNKNOWN_PET}")))
    .header(client::X_API_KEY, API_KEY)
    .header(client::X_API_VERSION, API_VERSION)
    .send()
    .await
    .expect("the raw GET reaches the server");

  assert_eq!(response.status(), http::StatusCode::NOT_FOUND);
  assert_eq!(content_type(&response), Some(PROBLEM_JSON));
}

#[rstest]
#[tokio::test]
async fn showing_with_an_unsupported_api_version_is_a_4xx_client_error(#[future(awt)] petstore: Petstore) {
  let created = create(&petstore, new_pet("Biscuit", client::Species::Dog)).await;
  let request = client::ShowPetByIdRequest::builder()
    .pet_id(created.id)
    .x_api_version("1999-01".to_string())
    .build()
    .expect("a valid showPetById request");

  let response = petstore
    .client
    .show_pet_by_id(request)
    .await
    .expect("showPetById round trip");

  let client::ApiResponse::ClientError(status, client::ErrorOrProblem::Error(error)) = response else {
    panic!("showPetById answered {response:?}");
  };
  assert_eq!(status, http::StatusCode::BAD_REQUEST);
  assert_eq!(error.code, 400);
  assert!(error.message.contains("1999-01"), "{}", error.message);
}

#[rstest]
#[tokio::test]
async fn showing_without_the_api_version_header_is_rejected(#[future(awt)] petstore: Petstore) {
  let created = create(&petstore, new_pet("Biscuit", client::Species::Dog)).await;
  let received_before = petstore.service.received().len();

  let response = reqwest::Client::new()
    .get(petstore.url(&format!("/pets/{}", created.id)))
    .header(client::X_API_KEY, API_KEY)
    .send()
    .await
    .expect("the raw GET reaches the server");

  assert_eq!(response.status(), http::StatusCode::BAD_REQUEST);
  let body = response.text().await.expect("a text body");
  assert!(body.contains("x-api-version"), "{body}");
  assert_eq!(petstore.service.received().len(), received_before);
}

#[rstest]
#[tokio::test]
async fn updating_applies_the_given_fields(#[future(awt)] petstore: Petstore) {
  let created = create(&petstore, complete_new_pet()).await;
  let patch = client::PetPatch::builder()
    .name("Sir Biscuit".to_string())
    .nickname("Biscuit the Brave".to_string())
    .weight_kg(13.25)
    .tags(vec!["friendly".to_string(), "brave".to_string()])
    .build();

  let updated = update(&petstore, created.id, patch).await;

  let Some(Received::UpdatePet(received)) = petstore.service.last_received() else {
    panic!("the server did not record updatePet");
  };
  assert_eq!(received.path.pet_id, created.id);
  assert_eq!(received.body.name.as_deref(), Some("Sir Biscuit"));
  assert_eq!(received.body.nickname.as_deref(), Some("Biscuit the Brave"));
  assert_eq!(received.body.weight_kg, Some(13.25));
  assert_eq!(
    received.body.tags,
    Some(vec!["friendly".to_string(), "brave".to_string()])
  );
  assert_eq!(updated.name, "Sir Biscuit");
  assert_eq!(updated.nickname.as_deref(), Some("Biscuit the Brave"));
  assert_eq!(updated.weight_kg, Some(13.25));
  assert_eq!(ordered(updated.tags.as_ref()), Some(vec!["friendly", "brave"]));
  assert_eq!(updated.species, created.species);
  assert_eq!(updated.birth_date, created.birth_date);
  assert_eq!(updated.microchip, created.microchip);
  assert_eq!(show(&petstore, show_request(created.id)).await.body, updated);
}

#[rstest]
#[case::built_with_the_builder(client::PetPatch::builder().name("Renamed".to_string()).build())]
#[case::struct_literal_over_default(client::PetPatch {
  name: Some("Renamed".to_string()),
  ..client::PetPatch::default()
})]
#[tokio::test]
async fn a_patch_leaves_omitted_fields_unchanged(#[future(awt)] petstore: Petstore, #[case] rename: client::PetPatch) {
  let created = create(&petstore, complete_new_pet()).await;
  let pending = update(
    &petstore,
    created.id,
    client::PetPatch::builder().status(client::PetStatus::Pending).build(),
  )
  .await;
  assert_eq!(pending.status, Some(client::PetStatus::Pending));

  let renamed = update(&petstore, created.id, rename).await;

  assert_eq!(renamed.name, "Renamed");
  assert_eq!(
    renamed.status,
    Some(client::PetStatus::Pending),
    "a merge patch that omits `status` must leave it unchanged"
  );
  assert_eq!(renamed.nickname, created.nickname);
  assert_eq!(renamed.weight_kg, created.weight_kg);
  assert_eq!(renamed.tags, created.tags);
}

#[rstest]
#[tokio::test]
async fn updating_sends_the_merge_patch_media_type(#[future(awt)] wiretapped: Wiretapped) {
  let petstore = &wiretapped.petstore;
  let created = create(petstore, new_pet("Biscuit", client::Species::Dog)).await;

  update(
    petstore,
    created.id,
    client::PetPatch::builder().nickname("Bis".to_string()).build(),
  )
  .await;

  let wire = wiretapped.last();
  assert_eq!(wire.method, http::Method::PATCH);
  assert_eq!(wire.header(http::header::CONTENT_TYPE), Some(MERGE_PATCH));
}

#[rstest]
#[tokio::test]
async fn a_patch_accepts_null_for_nullable_fields(#[future(awt)] petstore: Petstore) {
  let created = create(&petstore, complete_new_pet()).await;

  let response = patch_raw(&petstore, created.id, &json!({ "nickname": null, "weightKg": null })).await;

  let parsed = client::UpdatePetRequest::parse_response(response)
    .await
    .expect("a declared updatePet response");
  assert!(matches!(parsed, client::ApiResponse::Ok(_)), "{parsed:?}");
  let Some(Received::UpdatePet(received)) = petstore.service.last_received() else {
    panic!("the server did not record updatePet");
  };
  assert_eq!(received.body.nickname, None);
  assert_eq!(received.body.weight_kg, None);
}

/// Known generator limitation: optional nullable fields are `Option<T>`, so JSON `null` and an absent member both decode as `None` and a merge patch cannot clear a field.
#[rstest]
#[tokio::test]
async fn a_null_in_a_merge_patch_leaves_the_field_unchanged(#[future(awt)] petstore: Petstore) {
  let created = create(&petstore, complete_new_pet()).await;

  let response = patch_raw(&petstore, created.id, &json!({ "nickname": null })).await;

  let parsed = client::UpdatePetRequest::parse_response(response)
    .await
    .expect("a declared updatePet response");
  let client::ApiResponse::Ok(updated) = parsed else {
    panic!("updatePet answered {parsed:?}");
  };
  assert_eq!(updated.nickname, created.nickname);
  assert_eq!(
    show(&petstore, show_request(created.id)).await.body.nickname.as_deref(),
    Some("Bis")
  );
}

#[rstest]
#[case::empty_name(String::new())]
#[case::name_over_64_chars("n".repeat(65))]
#[tokio::test]
async fn an_invalid_patch_fails_to_build_and_is_unprocessable_on_the_server(
  #[future(awt)] petstore: Petstore,
  #[case] name: String,
) {
  let created = create(&petstore, new_pet("Biscuit", client::Species::Dog)).await;
  let built = client::UpdatePetRequest::builder()
    .pet_id(created.id)
    .body(client::PetPatch::builder().name(name.clone()).build())
    .build();
  assert!(built.is_err(), "a spec-invalid PetPatch must not build");

  let response = patch_raw(&petstore, created.id, &json!({ "name": name })).await;

  assert_eq!(content_type(&response), Some(PROBLEM_JSON));
  let parsed = client::UpdatePetRequest::parse_response(response)
    .await
    .expect("a declared updatePet response");
  let client::ApiResponse::UnprocessableEntity(problem) = parsed else {
    panic!("updatePet answered {parsed:?}");
  };
  assert_eq!(problem.status, Some(422));
  assert_eq!(show(&petstore, show_request(created.id)).await.body.name, "Biscuit");
}

#[rstest]
#[tokio::test]
async fn patching_an_unknown_pet_is_not_found(#[future(awt)] petstore: Petstore) {
  let request = client::UpdatePetRequest::builder()
    .pet_id(UNKNOWN_PET)
    .body(client::PetPatch::builder().name("Ghost".to_string()).build())
    .build()
    .expect("a valid PetPatch");

  let response = petstore.client.update_pet(request).await.expect("updatePet round trip");

  let client::ApiResponse::NotFound(problem) = response else {
    panic!("updatePet answered {response:?}");
  };
  assert_eq!(problem.status, Some(404));
  assert_eq!(problem.instance, Some(format!("/pets/{UNKNOWN_PET}")));
}

/// Known generator limitation: bodies rejected before the service runs answer 400 with a `text/plain` reason, not the spec's error schema.
#[rstest]
#[tokio::test]
async fn a_patch_with_an_unknown_field_is_a_plain_text_bad_request(#[future(awt)] petstore: Petstore) {
  let created = create(&petstore, new_pet("Biscuit", client::Species::Dog)).await;
  let received_before = petstore.service.received().len();

  let response = patch_raw(&petstore, created.id, &json!({ "name": "Rex", "favoriteColor": "red" })).await;

  assert_eq!(response.status(), http::StatusCode::BAD_REQUEST);
  assert!(
    content_type(&response).is_some_and(|media_type| media_type.starts_with("text/plain")),
    "{:?}",
    content_type(&response)
  );
  let reason = response.text().await.expect("a text body");
  assert!(reason.starts_with("Bad request: "), "{reason}");
  assert!(reason.contains("favoriteColor"), "{reason}");
  assert_eq!(petstore.service.received().len(), received_before);
  assert_eq!(show(&petstore, show_request(created.id)).await.body.name, "Biscuit");
}

#[rstest]
#[case::api_key_only(
  |anonymous: client::PetstoreClient| anonymous.with_api_key_auth(API_KEY),
  Some(API_KEY),
  None,
)]
#[case::bearer_only(
  |anonymous: client::PetstoreClient| anonymous.with_bearer_auth(STAFF_TOKEN),
  None,
  Some(STAFF_TOKEN),
)]
#[tokio::test]
async fn deleting_a_pet_accepts_either_credential_and_then_reports_it_gone(
  #[future(awt)] petstore: Petstore,
  #[case] authenticate: fn(client::PetstoreClient) -> client::PetstoreClient,
  #[case] expected_api_key: Option<&str>,
  #[case] expected_bearer: Option<&str>,
) {
  let created = create(&petstore, new_pet("Biscuit", client::Species::Dog)).await;
  let caller = authenticate(petstore.anonymous());
  let delete = || {
    client::DeletePetRequest::builder()
      .pet_id(created.id)
      .build()
      .expect("a valid deletePet request")
  };

  let deleted = caller.delete_pet(delete()).await.expect("deletePet round trip");

  assert!(matches!(deleted, client::ApiResponse::NoContent(())), "{deleted:?}");
  let Some(Received::DeletePet(received)) = petstore.service.last_received() else {
    panic!("the server did not record deletePet");
  };
  assert_eq!(received.path.pet_id, created.id);
  assert_eq!(exposed(received.credentials.api_key_auth.as_ref()), expected_api_key);
  assert_eq!(exposed(received.credentials.bearer_auth.as_ref()), expected_bearer);

  let again = caller.delete_pet(delete()).await.expect("deletePet round trip");
  let client::ApiResponse::NotFound(problem) = again else {
    panic!("a second deletePet answered {again:?}");
  };
  assert_eq!(problem.status, Some(404));
  let shown = petstore
    .client
    .show_pet_by_id(show_request(created.id))
    .await
    .expect("showPetById round trip");
  assert!(
    matches!(shown, client::ApiResponse::NotFound(client::ErrorOrProblem::Problem(_))),
    "{shown:?}"
  );
}

#[rstest]
#[tokio::test]
async fn deleting_without_credentials_is_unauthorized(#[future(awt)] petstore: Petstore) {
  let created = create(&petstore, new_pet("Biscuit", client::Species::Dog)).await;
  let request = client::DeletePetRequest::builder()
    .pet_id(created.id)
    .build()
    .expect("a valid deletePet request");

  let response = petstore
    .anonymous()
    .delete_pet(request)
    .await
    .expect("deletePet round trip");

  let client::ApiResponse::Other(status, _) = response else {
    panic!("an anonymous deletePet answered {response:?}");
  };
  assert_eq!(status, http::StatusCode::UNAUTHORIZED);
  assert!(
    petstore
      .service
      .with_state(|state| state.pets.contains_key(&created.id))
  );
}

#[rstest]
#[tokio::test]
async fn checking_existence_reports_the_etag_or_not_found(#[future(awt)] petstore: Petstore) {
  let created = create(&petstore, new_pet("Biscuit", client::Species::Dog)).await;
  let etag = show(&petstore, show_request(created.id)).await.headers.etag;
  let exists = |pet_id: Uuid| {
    client::PetExistsRequest::builder()
      .pet_id(pet_id)
      .build()
      .expect("a valid petExists request")
  };

  let present = petstore
    .client
    .pet_exists(exists(created.id))
    .await
    .expect("petExists round trip");
  let absent = petstore
    .client
    .pet_exists(exists(UNKNOWN_PET))
    .await
    .expect("petExists round trip");

  let client::ApiResponse::Ok(client::WithHeaders { headers, body: () }) = present else {
    panic!("petExists answered {present:?}");
  };
  assert!(headers.etag.is_some());
  assert_eq!(headers.etag, etag);
  assert!(matches!(absent, client::ApiResponse::NotFound(())), "{absent:?}");
  let Some(Received::PetExists(received)) = petstore.service.last_received() else {
    panic!("the server did not record petExists");
  };
  assert_eq!(received.path.pet_id, UNKNOWN_PET);
}

#[rstest]
#[tokio::test]
async fn lineage_defaults_to_two_generations_of_ancestors(#[future(awt)] petstore: Petstore) {
  let created = create(&petstore, new_pet("Biscuit", client::Species::Dog)).await;
  let request = client::GetPetLineageRequest::builder()
    .pet_id(created.id)
    .build()
    .expect("a valid getPetLineage request");

  let response = petstore
    .client
    .get_pet_lineage(request)
    .await
    .expect("getPetLineage round trip");

  let client::ApiResponse::Ok(lineage) = response else {
    panic!("getPetLineage answered {response:?}");
  };
  assert_eq!(generation_sizes(&lineage), [1, 2, 4]);
  assert_eq!(lineage.pet_id, created.id);
  assert_eq!(lineage.name, "Biscuit");
  assert_eq!(lineage.species, Some(client::Species::Dog));
  let Some(Received::GetPetLineage(received)) = petstore.service.last_received() else {
    panic!("the server did not record getPetLineage");
  };
  assert_eq!(received.query.depth, None);
}

#[rstest]
#[case::negative(-1)]
#[case::above_five(6)]
fn lineage_rejects_a_depth_outside_0_to_5(#[case] depth: i32) {
  let error = client::GetPetLineageRequest::builder()
    .pet_id(UNKNOWN_PET)
    .depth(depth)
    .build()
    .expect_err("a depth outside 0..=5 must not build");
  assert!(error.to_string().contains("depth"), "{error}");
}

#[rstest]
#[tokio::test]
async fn lineage_of_an_unknown_pet_is_not_found(#[future(awt)] petstore: Petstore) {
  let request = client::GetPetLineageRequest::builder()
    .pet_id(UNKNOWN_PET)
    .depth(1)
    .build()
    .expect("a valid getPetLineage request");

  let response = petstore
    .client
    .get_pet_lineage(request)
    .await
    .expect("getPetLineage round trip");

  assert!(
    matches!(&response, client::ApiResponse::NotFound(problem) if problem.status == Some(404)),
    "{response:?}"
  );
}

fn rabies() -> client::VaccineRecord {
  client::VaccineRecord {
    vaccine: "rabies".to_string(),
    date_administered: date(2026, 1, 10),
    veterinarian: Some("Dr. Paws".to_string()),
    next_due: Some(date(2027, 1, 10)),
  }
}

fn distemper() -> client::VaccineRecord {
  client::VaccineRecord {
    vaccine: "distemper".to_string(),
    date_administered: date(2026, 2, 1),
    veterinarian: None,
    next_due: None,
  }
}

#[rstest]
#[tokio::test]
async fn vaccinations_are_recorded_and_listed_in_order(#[future(awt)] petstore: Petstore) {
  let created = create(&petstore, new_pet("Biscuit", client::Species::Dog)).await;
  let list = || {
    client::ListVaccinationsRequest::builder()
      .pet_id(created.id)
      .build()
      .expect("a valid listVaccinations request")
  };
  let before = petstore
    .client
    .list_vaccinations(list())
    .await
    .expect("listVaccinations round trip");
  assert!(
    matches!(&before, client::ApiResponse::Ok(records) if records.is_empty()),
    "{before:?}"
  );

  for record in [rabies(), distemper()] {
    let request = client::RecordVaccinationRequest::builder()
      .pet_id(created.id)
      .body(record.clone())
      .build()
      .expect("a valid VaccineRecord");
    let response = petstore
      .client
      .record_vaccination(request)
      .await
      .expect("recordVaccination round trip");
    let client::ApiResponse::Created(echoed) = response else {
      panic!("recordVaccination answered {response:?}");
    };
    assert_eq!(echoed, record);
    let Some(Received::RecordVaccination(received)) = petstore.service.last_received() else {
      panic!("the server did not record recordVaccination");
    };
    assert_eq!(received.path.pet_id, created.id);
    assert_eq!(wire(&received.body), wire(&record));
  }

  let after = petstore
    .client
    .list_vaccinations(list())
    .await
    .expect("listVaccinations round trip");
  let client::ApiResponse::Ok(records) = after else {
    panic!("listVaccinations answered {after:?}");
  };
  assert_eq!(records, [rabies(), distemper()]);
}

#[rstest]
#[tokio::test]
async fn a_vaccination_with_a_null_next_due_is_recorded(#[future(awt)] petstore: Petstore) {
  let created = create(&petstore, new_pet("Biscuit", client::Species::Dog)).await;
  let body = json!({ "vaccine": "bordetella", "date_administered": "2026-03-03", "nextDue": null });

  let response = post_raw(&petstore, &format!("/pets/{}/vaccinations", created.id), &body).await;

  let parsed = client::RecordVaccinationRequest::parse_response(response)
    .await
    .expect("a declared recordVaccination response");
  let client::ApiResponse::Created(record) = parsed else {
    panic!("recordVaccination answered {parsed:?}");
  };
  let expected = client::VaccineRecord {
    vaccine: "bordetella".to_string(),
    date_administered: date(2026, 3, 3),
    veterinarian: None,
    next_due: None,
  };
  assert_eq!(record, expected);
}

#[rstest]
#[tokio::test]
async fn vaccinations_of_an_unknown_pet_are_not_found(#[future(awt)] petstore: Petstore) {
  let list = client::ListVaccinationsRequest::builder()
    .pet_id(UNKNOWN_PET)
    .build()
    .expect("a valid listVaccinations request");
  let record = client::RecordVaccinationRequest::builder()
    .pet_id(UNKNOWN_PET)
    .body(rabies())
    .build()
    .expect("a valid VaccineRecord");

  let listed = petstore
    .client
    .list_vaccinations(list)
    .await
    .expect("listVaccinations round trip");
  let recorded = petstore
    .client
    .record_vaccination(record)
    .await
    .expect("recordVaccination round trip");

  assert!(
    matches!(&listed, client::ApiResponse::NotFound(problem) if problem.status == Some(404)),
    "{listed:?}"
  );
  assert!(
    matches!(&recorded, client::ApiResponse::NotFound(problem) if problem.status == Some(404)),
    "{recorded:?}"
  );
  assert!(petstore.service.with_state(|state| state.vaccinations.is_empty()));
}

#[rstest]
#[case::ascending(
  client::ListPetsRequestHeaderXSortOrder::Asc,
  server::ListPetsRequestHeaderXSortOrder::Asc,
  None,
  &["Felix", "Mittens", "Tiger"],
)]
#[case::descending(
  client::ListPetsRequestHeaderXSortOrder::Desc,
  server::ListPetsRequestHeaderXSortOrder::Desc,
  None,
  &["Tiger", "Mittens", "Felix"],
)]
#[case::limited(
  client::ListPetsRequestHeaderXSortOrder::Asc,
  server::ListPetsRequestHeaderXSortOrder::Asc,
  Some(2),
  &["Felix", "Mittens"],
)]
#[tokio::test]
async fn listing_cats_returns_only_cats_with_their_cat_fields(
  #[future(awt)] petstore: Petstore,
  #[case] order: client::ListPetsRequestHeaderXSortOrder,
  #[case] received_order: server::ListPetsRequestHeaderXSortOrder,
  #[case] limit: Option<i32>,
  #[case] expected: &[&str],
) {
  for pet in [
    client::NewPet {
      nickname: Some("Stripes".to_string()),
      tags: Some(tag_set(&["orange"])),
      ..new_pet("Tiger", client::Species::Cat)
    },
    new_pet("Biscuit", client::Species::Dog),
    new_pet("Felix", client::Species::Cat),
    new_pet("Kiwi", client::Species::Bird),
    new_pet("Mittens", client::Species::Cat),
  ] {
    create(&petstore, pet).await;
  }
  let request = client::ListCatsRequest::builder()
    .x_sort_order(order)
    .maybe_limit(limit)
    .build()
    .expect("a valid listCats request");

  #[expect(deprecated)]
  let response = petstore.client.list_cats(request).await.expect("listCats round trip");

  let client::ApiResponse::Ok(client::WithHeaders { headers, body: cats }) = response else {
    panic!("listCats answered {response:?}");
  };
  let cat_names = cats.iter().map(|cat| cat.name.as_str()).collect::<Vec<_>>();
  assert_eq!(cat_names, expected);
  assert!(
    cats
      .iter()
      .all(|cat| cat.species == client::Species::Cat && (1..=9).contains(&cat.lives)),
    "{cats:?}"
  );
  if let Some(tiger) = cats.iter().find(|cat| cat.name == "Tiger") {
    assert_eq!(tiger.nickname.as_deref(), Some("Stripes"));
    assert_eq!(ordered(tiger.tags.as_ref()), Some(vec!["orange"]));
  }
  if limit.is_none() {
    assert_eq!(headers.x_next, None, "every cat fits on one page");
  }
  let Some(Received::ListCats(received)) = petstore.service.last_received() else {
    panic!("the server did not record listCats");
  };
  assert_eq!(received.query.limit, limit);
  assert_eq!(received.header.x_sort_order, Some(received_order));
}

#[rstest]
#[case::allergies(server::Health::allergies(Some(vec!["pollen".to_string(), "chicken".to_string()])))]
#[case::diet(server::Health::diet(Some("grain-free kibble".to_string())))]
#[tokio::test]
async fn seeded_health_notes_and_vaccination_history_reach_the_client(
  #[future(awt)] petstore: Petstore,
  #[case] health: server::Health,
) {
  let record = |vaccine: &str, administered: NaiveDate, next_due: Option<NaiveDate>| server::VaccineRecord {
    vaccine: vaccine.to_string(),
    date_administered: administered,
    veterinarian: Some("Dr. Paws".to_string()),
    next_due,
  };
  let vaccinations = IndexMap::from([
    (
      "rabies".to_string(),
      vec![record("rabies", date(2025, 1, 10), Some(date(2026, 1, 10)))],
    ),
    (
      "distemper".to_string(),
      vec![
        record("DHPP", date(2024, 5, 2), Some(date(2025, 5, 2))),
        record("DHPP booster", date(2025, 5, 2), None),
      ],
    ),
  ]);
  let pet_id = petstore.service.insert_pet(server::Pet {
    allergies: Some(Box::new(health.clone())),
    vaccinations: Some(vaccinations.clone()),
    ..seeded_pet(1, "Biscuit", server::Species::Dog)
  });

  let shown = show(&petstore, show_request(pet_id)).await.body;

  let allergies = shown.allergies.expect("the pet has a health note");
  match (health, *allergies) {
    (server::Health::Allergies(sent), client::Health::Allergies(read)) => {
      assert_eq!(read.allergens, sent.allergens);
      assert_eq!(read.r#type, Some("allergies"));
    }
    (server::Health::Diet(sent), client::Health::Diet(read)) => {
      assert_eq!(read.food, sent.food);
      assert_eq!(read.r#type, Some("diet"));
    }
    (sent, read) => panic!("{sent:?} was read back as {read:?}"),
  }
  let read_vaccinations = shown.vaccinations.expect("the pet has a vaccination history");
  assert_eq!(read_vaccinations.keys().collect::<Vec<_>>(), ["rabies", "distemper"]);
  assert_eq!(wire(&read_vaccinations), wire(&vaccinations));
}

fn any_species() -> impl Strategy<Value = client::Species> {
  prop_oneof![
    Just(client::Species::Dog),
    Just(client::Species::Cat),
    Just(client::Species::Bird),
    Just(client::Species::Fish),
    Just(client::Species::Reptile),
    Just(client::Species::SmallMammal),
  ]
}

fn birth_dates() -> impl Strategy<Value = NaiveDate> {
  (0..=12_000u64).prop_map(|offset| date(1995, 1, 1) + chrono::Days::new(offset))
}

/// Weights in grams of precision, inside the `(0, 500]` kilogram range.
fn weights_kg() -> impl Strategy<Value = f64> {
  (1..=500_000u32).prop_map(|grams| f64::from(grams) / 1000.0)
}

fn sort_orders() -> impl Strategy<Value = client::ListPetsRequestHeaderXSortOrder> {
  prop_oneof![
    Just(client::ListPetsRequestHeaderXSortOrder::Asc),
    Just(client::ListPetsRequestHeaderXSortOrder::Desc),
  ]
}

prop_compose! {
  /// Any `NewPet` inside the document's constraints.
  fn valid_new_pets()(
    name in "[A-Za-z][A-Za-z '-]{0,63}",
    species in any_species(),
    nickname in proptest::option::of("[A-Za-z]{1,16}"),
    birth_date in proptest::option::of(birth_dates()),
    weight_kg in proptest::option::of(weights_kg()),
    microchip in proptest::option::of("[0-9]{15}"),
    tags in proptest::option::of(proptest::collection::vec("[a-z]{1,10}", 0..=10)),
  ) -> client::NewPet {
    client::NewPet {
      name,
      species,
      nickname,
      birth_date,
      weight_kg,
      microchip,
      tags: tags.map(|tags| tags.into_iter().collect::<IndexSet<_>>()),
    }
  }
}

#[test]
fn any_valid_new_pet_reads_back_unchanged() {
  let petstore = start_shared();
  proptest!(ProptestConfig::with_cases(48), |(pet in valid_new_pets())| {
    run_case(async || {
      let created = create(&petstore, pet.clone()).await;
      let shown = show(&petstore, show_request(created.id)).await.body;
      prop_assert_eq!(&shown.name, &pet.name);
      prop_assert_eq!(&shown.species, &pet.species);
      prop_assert_eq!(&shown.nickname, &pet.nickname);
      prop_assert_eq!(shown.birth_date, pet.birth_date);
      prop_assert_eq!(shown.weight_kg, pet.weight_kg);
      prop_assert_eq!(&shown.microchip, &pet.microchip);
      prop_assert_eq!(ordered(shown.tags.as_ref()), ordered(pet.tags.as_ref()));
      Ok::<_, TestCaseError>(())
    })?;
  });
}

/// An exploded array with no items sends no pairs, so it arrives as an absent parameter.
fn non_empty<T>(items: Vec<T>) -> Option<Vec<T>> {
  (!items.is_empty()).then_some(items)
}

fn any_card_theme() -> impl Strategy<Value = client::GetPetCardRequestCookieCardTheme> {
  prop_oneof![
    Just(client::GetPetCardRequestCookieCardTheme::Light),
    Just(client::GetPetCardRequestCookieCardTheme::Dark),
    Just(client::GetPetCardRequestCookieCardTheme::HighContrast),
  ]
}

#[test]
fn any_card_cookies_arrive_as_the_client_set_them() {
  let petstore = start_shared();
  let pet = runtime().block_on(create(&petstore, new_pet("Biscuit", client::Species::Dog)));
  proptest!(ProptestConfig::with_cases(48), |(
    theme in proptest::option::of(any_card_theme()),
    recently_viewed in proptest::option::of(proptest::collection::vec(any::<u128>().prop_map(Uuid::from_u128), 1..=5)),
  )| {
    run_case(async || {
      let response = petstore
        .client
        .get_pet_card(card_request(pet.id, theme.clone(), recently_viewed.clone()))
        .await
        .expect("getPetCard round trip");
      prop_assert!(matches!(response, client::ApiResponse::Ok(_)), "{response:?}");
      let Some(Received::GetPetCard(received)) = petstore
        .service
        .received()
        .into_iter()
        .rev()
        .find(|received| matches!(received, Received::GetPetCard(_)))
      else {
        panic!("the server did not record getPetCard");
      };
      prop_assert_eq!(
        received.cookie.card_theme.map(|theme| theme.to_string()),
        theme.map(|theme| theme.to_string())
      );
      prop_assert_eq!(received.cookie.recently_viewed, recently_viewed);
      Ok::<_, TestCaseError>(())
    })?;
  });
}

fn any_pet_status() -> impl Strategy<Value = (client::PetStatus, server::PetStatus)> {
  prop_oneof![
    Just((client::PetStatus::Available, server::PetStatus::Available)),
    Just((client::PetStatus::Pending, server::PetStatus::Pending)),
    Just((client::PetStatus::Adopted, server::PetStatus::Adopted)),
  ]
}

#[test]
fn any_exploded_arrays_arrive_as_the_arrays_the_client_built() {
  let petstore = start_shared();
  proptest!(ProptestConfig::with_cases(48), |(
    tags in proptest::collection::vec("\\PC{1,12}", 0..6),
    statuses in proptest::collection::vec(any_pet_status(), 0..4),
    born_in in proptest::collection::vec(1990..=2026i32, 0..5),
  )| {
    run_case(async || {
      let (sent_statuses, expected_statuses): (Vec<_>, Vec<_>) = statuses.into_iter().unzip();
      let request = client::SearchPetsRequest::builder()
        .q(RESIDENT.to_string())
        .tags(tags.clone())
        .statuses(sent_statuses)
        .born_in(born_in.clone())
        .build()
        .expect("a valid searchPets request");
      let response = petstore.client.search_pets(request).await.expect("searchPets round trip");
      prop_assert!(matches!(response, client::ApiResponse::Ok(_)), "{response:?}");

      let Some(Received::SearchPets(received)) = petstore
        .service
        .received()
        .into_iter()
        .rev()
        .find(|received| matches!(received, Received::SearchPets(_)))
      else {
        panic!("the server did not record searchPets");
      };
      prop_assert_eq!(received.query.tags, non_empty(tags));
      prop_assert_eq!(received.query.statuses, non_empty(expected_statuses));
      prop_assert_eq!(received.query.born_in, non_empty(born_in));
      Ok::<_, TestCaseError>(())
    })?;
  });
}

#[test]
fn walking_x_next_yields_every_matching_pet_once_in_sort_order() {
  let petstore = start_shared();
  proptest!(ProptestConfig::with_cases(48), |(
    pet_names in proptest::collection::vec("[a-z]{1,8}", 0..=24),
    limit in 1..=10i32,
    order in sort_orders(),
  )| {
    run_case(async || {
      let run_tag = format!("pagination-run-{}", PAGINATION_RUN.fetch_add(1, Ordering::Relaxed));
      let mut expected = vec![];
      for name in &pet_names {
        let pet = client::NewPet {
          tags: Some(tag_set(&[run_tag.as_str()])),
          ..new_pet(name, client::Species::Dog)
        };
        expected.push(create(&petstore, pet).await);
      }
      expected.sort_by(|left, right| match order {
        client::ListPetsRequestHeaderXSortOrder::Asc => left.name.cmp(&right.name),
        client::ListPetsRequestHeaderXSortOrder::Desc => right.name.cmp(&left.name),
      });
      let page_size = usize::try_from(limit).expect("a positive limit");
      let total = i64::try_from(pet_names.len()).expect("a small count");

      let mut walked = vec![];
      let mut cursor = None;
      loop {
        let request = client::ListPetsRequest::builder()
          .x_compatibility_date(COMPATIBILITY_DATE)
          .limit(limit)
          .tags(vec![run_tag.clone()])
          .x_sort_order(order.clone())
          .maybe_cursor(cursor.take())
          .build()
          .expect("a valid listPets request");
        let page = list_pets(&petstore.client, request).await;
        prop_assert!(page.body.len() <= page_size, "{} pets exceed limit {}", page.body.len(), limit);
        prop_assert_eq!(page.headers.x_total_count, Some(total));
        let page_was_empty = page.body.is_empty();
        walked.extend(page.body);
        let Some(next) = page.headers.x_next else {
          break;
        };
        prop_assert!(
          !page_was_empty && walked.len() < pet_names.len(),
          "x-next `{}` after {} of {} pets",
          next,
          walked.len(),
          pet_names.len()
        );
        cursor = Some(next);
      }

      prop_assert_eq!(names(&walked), names(&expected));
      let mut walked_ids = walked.iter().map(|pet| pet.id).collect::<Vec<_>>();
      let mut expected_ids = expected.iter().map(|pet| pet.id).collect::<Vec<_>>();
      walked_ids.sort_unstable();
      expected_ids.sort_unstable();
      prop_assert_eq!(walked_ids, expected_ids);
      Ok::<_, TestCaseError>(())
    })?;
  });
}

#[test]
fn lineage_has_the_requested_number_of_generations() {
  let petstore = start_shared();
  let pet = runtime().block_on(create(&petstore, new_pet("Biscuit", client::Species::Dog)));
  proptest!(ProptestConfig::with_cases(32), |(depth in 0..=5i32)| {
    run_case(async || {
      let request = client::GetPetLineageRequest::builder()
        .pet_id(pet.id)
        .depth(depth)
        .build()
        .expect("a depth inside 0..=5");
      let response = petstore.client.get_pet_lineage(request).await.expect("getPetLineage round trip");
      let client::ApiResponse::Ok(lineage) = response else {
        return Err(TestCaseError::fail(format!("getPetLineage answered {response:?}")));
      };
      let generations = usize::try_from(depth).expect("a non-negative depth") + 1;
      let expected = (0..generations).map(|generation| 1usize << generation).collect::<Vec<_>>();
      prop_assert_eq!(generation_sizes(&lineage), expected);
      prop_assert_eq!(lineage.pet_id, pet.id);
      let Some(Received::GetPetLineage(received)) = petstore.service.last_received() else {
        return Err(TestCaseError::fail("the server did not record getPetLineage"));
      };
      prop_assert_eq!(received.query.depth, Some(depth));
      Ok::<_, TestCaseError>(())
    })?;
  });
}

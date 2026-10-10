//! Store, adoption, telemetry, and system operations, plus both webhooks, end to end.
//!
//! Each test drives the generated client against the generated router backed by the
//! in-memory service, then checks both what the server extracted (`Received`) and what
//! the client parsed. Raw `reqwest` requests cover only what the client deliberately
//! can't send: explicit `null`s, bodies that break the schema, credential-less calls,
//! and webhook deliveries, which the API sends rather than receives.

mod support;

use chrono::{DateTime, NaiveDate, NaiveTime, TimeDelta, Utc};
use indexmap::{IndexMap, IndexSet};
use petstore_client as client;
use petstore_server as server;
use proptest::prelude::*;
use reqwest::{StatusCode, header::CONTENT_TYPE};
use rstest::rstest;
use serde_json::{Value, json};
use support::{API_KEY, API_VERSION, Petstore, Received, now, petstore, run_case, runtime, start_shared};
use uuid::Uuid;

/// A collar id that matches `^COL-[0-9A-F]{8}$`.
const COLLAR_ID: &str = "COL-0A1B2C3D";

/// An int64 beyond 2^53, which a double can't hold exactly.
const BEYOND_DOUBLE_PRECISION: i64 = 9_007_199_254_740_993;

fn last_received(petstore: &Petstore) -> Received {
  petstore.service.last_received().expect("the server recorded a request")
}

async fn create_pet(petstore: &Petstore, name: &str, species: client::Species) -> Uuid {
  let new_pet = client::NewPet {
    name: name.to_string(),
    species,
    ..client::NewPet::default()
  };
  let response = petstore
    .client
    .create_pet(
      client::CreatePetRequest::builder()
        .body(new_pet)
        .build()
        .expect("the pet is valid"),
    )
    .await
    .expect("the create request completes");
  let client::ApiResponse::Created(client::WithHeaders { body: pet, .. }) = response else {
    panic!("expected 201 for {name}, got {response:?}");
  };
  pet.id
}

async fn pet_status(petstore: &Petstore, pet_id: Uuid) -> Option<client::PetStatus> {
  let request = client::ShowPetByIdRequest::builder()
    .pet_id(pet_id)
    .x_api_version(API_VERSION.to_string())
    .build()
    .expect("the lookup is valid");
  let response = petstore
    .client
    .show_pet_by_id(request)
    .await
    .expect("the lookup completes");
  let client::ApiResponse::Ok(client::WithHeaders { body: pet, .. }) = response else {
    panic!("expected 200 for pet {pet_id}, got {response:?}");
  };
  pet.status
}

fn adopter() -> client::Adopter {
  client::Adopter {
    name: "Avery Quinn".to_string(),
    email: "avery@example.com".to_string(),
    phone: Some("+15035550100".to_string()),
    address: Some(client::Address {
      city: "Portland".to_string(),
      zip: Some("97201".to_string()),
    }),
  }
}

fn application(pet_id: Uuid) -> client::AdoptionRequest {
  client::AdoptionRequest {
    pet_id,
    adopter: adopter(),
    ..client::AdoptionRequest::default()
  }
}

async fn adopt(
  petstore: &Petstore,
  application: client::AdoptionRequest,
) -> client::ApiResponse<client::WithHeaders<client::LocationHeaders, client::Adoption>, client::Error> {
  let request = client::AdoptPetRequest::builder()
    .body(application)
    .build()
    .expect("the application is valid");
  petstore
    .client
    .adopt_pet(request)
    .await
    .expect("the adoption request completes")
}

async fn get_adoption(petstore: &Petstore, adoption_id: i64) -> client::ApiResponse<client::Adoption, client::Problem> {
  let request = client::GetAdoptionRequest::builder()
    .adoption_id(adoption_id)
    .build()
    .expect("the adoption id is valid");
  petstore
    .client
    .get_adoption(request)
    .await
    .expect("the adoption lookup completes")
}

#[rstest]
#[tokio::test]
async fn adopting_an_available_pet_is_accepted_at_its_location(#[future(awt)] petstore: Petstore) {
  let pet_id = create_pet(&petstore, "Biscuit", client::Species::Dog).await;
  let home_visit = now() + TimeDelta::days(3) + TimeDelta::milliseconds(500);
  let callback_url = petstore.url("/callbacks/adoptPet/statusChanged?ref=biscuit");

  let response = adopt(
    &petstore,
    client::AdoptionRequest {
      callback_url: Some(callback_url.clone()),
      home_visit: Some(home_visit),
      ..application(pet_id)
    },
  )
  .await;

  let client::ApiResponse::Accepted(client::WithHeaders {
    headers,
    body: adoption,
  }) = response
  else {
    panic!("expected 202, got {response:?}");
  };
  assert!(
    adoption.id >= 1,
    "the server assigns a positive int64 id, got {}",
    adoption.id
  );
  assert_eq!(headers.location, format!("/adoptions/{}", adoption.id));
  assert_eq!(
    adoption,
    client::Adoption {
      id: adoption.id,
      pet_id,
      adopter: Some(adopter()),
      status: client::AdoptionStatus::Submitted,
      submitted_at: now(),
    }
  );

  let Some(Received::AdoptPet(received)) = petstore
    .service
    .received()
    .into_iter()
    .find(|received| matches!(received, Received::AdoptPet(_)))
  else {
    panic!("the server did not record the adoption");
  };
  assert_eq!(received.body.pet_id, pet_id);
  assert_eq!(received.body.callback_url, Some(callback_url));
  assert!(
    matches!(last_received(&petstore), Received::AdoptPetStatusChanged(_)),
    "the callback URL received the status callback"
  );
  assert_eq!(received.body.home_visit, Some(home_visit));
  let server::Adopter {
    name,
    email,
    phone,
    address,
  } = received.body.adopter;
  assert_eq!(
    (name.as_str(), email.as_str(), phone.as_deref()),
    ("Avery Quinn", "avery@example.com", Some("+15035550100"))
  );
  assert_eq!(
    address.map(|address| (address.city, address.zip)),
    Some(("Portland".to_string(), Some("97201".to_string())))
  );

  assert_eq!(pet_status(&petstore, pet_id).await, Some(client::PetStatus::Pending));
}

#[rstest]
#[tokio::test]
async fn adopting_a_pet_that_is_no_longer_available_conflicts(#[future(awt)] petstore: Petstore) {
  let pet_id = create_pet(&petstore, "Mochi", client::Species::Cat).await;

  let first = adopt(&petstore, application(pet_id)).await;
  assert!(
    matches!(first, client::ApiResponse::Accepted(_)),
    "the first application is accepted, got {first:?}"
  );
  let Received::AdoptPet(received) = last_received(&petstore) else {
    panic!("the server did not record the adoption");
  };
  assert_eq!(
    (received.body.callback_url, received.body.home_visit),
    (None, None),
    "omitted optional fields arrive absent"
  );

  let second = adopt(&petstore, application(pet_id)).await;

  let client::ApiResponse::Conflict(error) = second else {
    panic!("expected 409, got {second:?}");
  };
  assert_eq!(error.code, 409);
  assert!(
    error.message.contains("Mochi"),
    "the conflict explains itself: {}",
    error.message
  );
}

#[rstest]
#[tokio::test]
async fn an_explicit_null_home_visit_is_accepted(#[future(awt)] petstore: Petstore) {
  let pet_id = create_pet(&petstore, "Pepper", client::Species::Bird).await;

  let response = reqwest::Client::new()
    .post(petstore.url("/adoptions"))
    .header(client::X_API_KEY, API_KEY)
    .json(&json!({
      "petId": pet_id,
      "adopter": { "name": "Avery Quinn", "email": "avery@example.com" },
      "homeVisit": null,
    }))
    .send()
    .await
    .expect("the raw request completes");

  assert_eq!(response.status(), StatusCode::ACCEPTED);
  let Received::AdoptPet(received) = last_received(&petstore) else {
    panic!("the server did not record the adoption");
  };
  assert_eq!(received.body.home_visit, None);
}

#[test]
fn the_client_rejects_a_callback_url_that_is_not_a_uri() {
  let application = client::AdoptionRequest {
    callback_url: Some("not a uri".to_string()),
    ..application(Uuid::from_u128(1))
  };

  assert!(client::AdoptPetRequest::builder().body(application).build().is_err());
}

#[rstest]
#[tokio::test]
async fn an_adoption_is_fetched_by_its_id(#[future(awt)] petstore: Petstore) {
  let pet_id = create_pet(&petstore, "Biscuit", client::Species::Dog).await;
  let accepted = adopt(&petstore, application(pet_id)).await;
  let client::ApiResponse::Accepted(client::WithHeaders { body: adoption, .. }) = accepted else {
    panic!("expected 202, got {accepted:?}");
  };

  let response = get_adoption(&petstore, adoption.id).await;

  let client::ApiResponse::Ok(fetched) = response else {
    panic!("expected 200, got {response:?}");
  };
  assert_eq!(fetched, adoption);
  let Received::GetAdoption(received) = last_received(&petstore) else {
    panic!("the server did not record the lookup");
  };
  assert_eq!(received.path.adoption_id, adoption.id);
}

#[rstest]
#[tokio::test]
async fn an_unknown_adoption_is_a_not_found_problem(#[future(awt)] petstore: Petstore) {
  let response = get_adoption(&petstore, BEYOND_DOUBLE_PRECISION).await;

  let client::ApiResponse::NotFound(problem) = response else {
    panic!("expected 404, got {response:?}");
  };
  assert_eq!(problem.status, Some(404));
  assert_eq!(problem.r#type.as_deref(), Some("about:blank"));
  assert_eq!(problem.instance, Some(format!("/adoptions/{BEYOND_DOUBLE_PRECISION}")));
  let Received::GetAdoption(received) = last_received(&petstore) else {
    panic!("the server did not record the lookup");
  };
  assert_eq!(
    received.path.adoption_id, BEYOND_DOUBLE_PRECISION,
    "the int64 path parameter keeps every digit"
  );
}

#[rstest]
#[case::adoption_zero(|| client::GetAdoptionRequest::builder().adoption_id(0).build().is_err())]
#[case::adoption_negative(|| client::GetAdoptionRequest::builder().adoption_id(-5).build().is_err())]
#[case::order_zero(|| client::GetOrderByIdRequest::builder().order_id(0).build().is_err())]
#[case::cancelled_order_zero(|| client::DeleteOrderRequest::builder().order_id(0).build().is_err())]
fn the_client_rejects_path_ids_below_one(#[case] rejected: fn() -> bool) {
  assert!(rejected());
}

/// Telemetry that sets every property `CollarTelemetry` declares, each within its constraints.
#[expect(deprecated)]
fn full_telemetry() -> client::CollarTelemetry {
  client::CollarTelemetry {
    collar_id: COLLAR_ID.to_string(),
    firmware: Some("2.4.1".to_string()),
    pairing_secret: Some("pair-0123456789".to_string()),
    legacy_signal: Some(-42),
    recorded_at: now(),
    recorded_on: NaiveDate::from_ymd_opt(2026, 10, 9),
    local_time: NaiveTime::from_hms_milli_opt(18, 30, 15, 250),
    location: vec![45.52, -122.68],
    samples: vec![72, 75, 80],
    battery_percent: Some(85),
    signal_quality: Some(0.75),
    gateway_ip: Some("192.168.1.20".to_string()),
    gateway_host: Some("gateway.petstore.example".to_string()),
    reading_id: Some(Uuid::from_u128(0x0123_4567_89ab_cdef_0123_4567_89ab_cdef)),
    owner_email: Some("owner@petstore.example".to_string()),
    firmware_image: Some("AAECAw==".to_string()),
    signature: Some(vec![0xde, 0xad, 0xbe, 0xef]),
    sequence: Some(BEYOND_DOUBLE_PRECISION),
    mode: Some(client::CollarTelemetryMode::Tracking),
    zones: Some(IndexSet::from(["kitchen".to_string(), "yard".to_string()])),
    extra: Some(IndexMap::from([
      ("x-channel".to_string(), "beta".to_string()),
      ("room".to_string(), "den".to_string()),
    ])),
    alert: Some("low-battery".to_string()),
    notes: Some("Calm after the walk".to_string()),
    threshold: Some(7),
    any_value: Some(json!({ "nested": [1, "two", null, { "three": 3.5 }] })),
    mixed: Some(json!(42)),
  }
}

/// Telemetry with only the required properties.
fn minimal_telemetry() -> client::CollarTelemetry {
  client::CollarTelemetry {
    collar_id: COLLAR_ID.to_string(),
    recorded_at: now(),
    location: vec![45.52, -122.68],
    samples: vec![72],
    ..client::CollarTelemetry::default()
  }
}

/// The same required properties as [`minimal_telemetry`], as raw JSON.
fn minimal_telemetry_json() -> Value {
  json!({
    "collarId": COLLAR_ID,
    "recordedAt": "2026-10-09T18:00:00Z",
    "location": [45.52, -122.68],
    "samples": [72, 75, 80],
  })
}

#[expect(deprecated)]
fn legacy_signal(telemetry: &server::CollarTelemetry) -> Option<i32> {
  telemetry.legacy_signal
}

async fn submit_telemetry(
  petstore: &Petstore,
  telemetry: client::CollarTelemetry,
) -> client::ApiResponse<client::TelemetryReceipt, client::Error> {
  let request = client::SubmitCollarTelemetryRequest::builder()
    .collar_id(COLLAR_ID.to_string())
    .body(telemetry)
    .build()
    .expect("the telemetry is valid");
  petstore
    .client
    .submit_collar_telemetry(request)
    .await
    .expect("the telemetry request completes")
}

async fn post_raw_telemetry(petstore: &Petstore, body: &Value) -> reqwest::Response {
  reqwest::Client::new()
    .post(petstore.url(&format!("/collars/{COLLAR_ID}/telemetry")))
    .header(client::X_API_KEY, API_KEY)
    .json(body)
    .send()
    .await
    .expect("the raw request completes")
}

fn received_telemetry(petstore: &Petstore) -> server::SubmitCollarTelemetryRequest {
  let Received::SubmitCollarTelemetry(received) = last_received(petstore) else {
    panic!("the server did not record a telemetry submission");
  };
  received
}

#[rstest]
#[tokio::test]
async fn every_telemetry_field_reaches_the_server(#[future(awt)] petstore: Petstore) {
  let sent = full_telemetry();

  let response = submit_telemetry(&petstore, sent.clone()).await;

  let client::ApiResponse::Created(receipt) = response else {
    panic!("expected 201, got {response:?}");
  };
  assert_eq!(
    receipt,
    client::TelemetryReceipt {
      accepted: true,
      reading_count: 3,
      received_at: Some(now()),
    }
  );
  let received = received_telemetry(&petstore);
  assert_eq!(received.path.collar_id, COLLAR_ID);
  assert_eq!(legacy_signal(&received.body), Some(-42), "deprecated legacySignal");
  let body = received.body;
  assert_eq!(body.collar_id, sent.collar_id);
  assert_eq!(body.firmware, sent.firmware, "readOnly firmware");
  assert_eq!(body.pairing_secret, sent.pairing_secret, "writeOnly pairingSecret");
  assert_eq!(
    (body.recorded_at, body.recorded_on, body.local_time),
    (sent.recorded_at, sent.recorded_on, sent.local_time),
    "date-time, date, and time formats"
  );
  assert_eq!(body.location, sent.location, "prefixItems location");
  assert_eq!(body.samples, sent.samples);
  assert_eq!(body.battery_percent, sent.battery_percent);
  assert_eq!(body.signal_quality, sent.signal_quality);
  assert_eq!(
    (body.gateway_ip, body.gateway_host, body.reading_id, body.owner_email),
    (sent.gateway_ip, sent.gateway_host, sent.reading_id, sent.owner_email),
    "ipv4, hostname, uuid, and email strings"
  );
  assert_eq!(
    body.firmware_image, sent.firmware_image,
    "contentEncoding base64 stays text"
  );
  assert_eq!(
    body.signature, sent.signature,
    "format byte decodes back to the same bytes"
  );
  assert_eq!(body.sequence, Some(BEYOND_DOUBLE_PRECISION));
  assert_eq!(body.mode, Some(server::CollarTelemetryMode::Tracking));
  assert_eq!(body.zones, sent.zones);
  assert_eq!(body.extra, sent.extra);
  assert_eq!(
    (body.alert, body.notes, body.threshold),
    (sent.alert, sent.notes, sent.threshold)
  );
  assert_eq!((body.any_value, body.mixed), (sent.any_value, sent.mixed));
}

#[test]
fn telemetry_encodes_bytes_dates_and_times_as_the_schema_says() {
  let wire = serde_json::to_value(full_telemetry()).expect("telemetry serializes");

  assert_eq!(wire["signature"], "3q2+7w==", "format byte is base64 text");
  assert_eq!(
    wire["firmwareImage"], "AAECAw==",
    "contentEncoding base64 is sent as given"
  );
  assert_eq!(wire["recordedAt"], "2026-10-09T18:00:00Z");
  assert_eq!(wire["recordedOn"], "2026-10-09");
  assert_eq!(wire["localTime"], "18:30:15.250");
  assert_eq!(wire["sequence"], json!(BEYOND_DOUBLE_PRECISION));
  assert_eq!(wire["mode"], "tracking");
  assert_eq!(wire["zones"], json!(["kitchen", "yard"]));
}

#[rstest]
#[tokio::test]
async fn an_omitted_mode_defaults_to_standard(#[future(awt)] petstore: Petstore) {
  assert_eq!(
    client::CollarTelemetry::default().mode,
    Some(client::CollarTelemetryMode::Standard)
  );
  let sent = client::CollarTelemetry {
    mode: None,
    ..minimal_telemetry()
  };
  assert_eq!(
    serde_json::to_value(&sent).expect("telemetry serializes").get("mode"),
    None
  );

  let response = submit_telemetry(&petstore, sent).await;

  assert!(
    matches!(response, client::ApiResponse::Created(_)),
    "expected 201, got {response:?}"
  );
  assert_eq!(
    received_telemetry(&petstore).body.mode,
    Some(server::CollarTelemetryMode::Standard)
  );
}

#[rstest]
#[case::object(json!({ "collar": { "leds": [1, 2] } }), json!(42))]
#[case::array(json!([1, "two", null, { "three": 3.5 }]), json!("forty-two"))]
#[case::string(json!("free text"), json!(-7))]
#[case::number(json!(2.5), json!(""))]
#[case::boolean(json!(false), json!(i64::MIN))]
#[tokio::test]
async fn untyped_and_multi_typed_values_arrive_unchanged(
  #[future(awt)] petstore: Petstore,
  #[case] any_value: Value,
  #[case] mixed: Value,
) {
  let sent = client::CollarTelemetry {
    any_value: Some(any_value.clone()),
    mixed: Some(mixed.clone()),
    ..minimal_telemetry()
  };

  let response = submit_telemetry(&petstore, sent).await;

  assert!(
    matches!(response, client::ApiResponse::Created(_)),
    "expected 201, got {response:?}"
  );
  let body = received_telemetry(&petstore).body;
  assert_eq!(body.any_value, Some(any_value), "anyValue");
  assert_eq!(body.mixed, Some(mixed), "mixed");
}

/// Known limitation: an optional untyped field is `Option<serde_json::Value>`, so `null` and absent both arrive as `None`.
#[rstest]
#[tokio::test]
async fn null_and_absent_any_values_both_arrive_as_none(#[future(awt)] petstore: Petstore) {
  let null = client::CollarTelemetry {
    any_value: Some(Value::Null),
    ..minimal_telemetry()
  };
  assert_eq!(
    serde_json::to_value(&null)
      .expect("telemetry serializes")
      .get("anyValue"),
    Some(&Value::Null),
    "the client sends an explicit null"
  );

  let null_response = submit_telemetry(&petstore, null).await;
  let null_received = received_telemetry(&petstore).body.any_value;
  let absent_response = submit_telemetry(&petstore, minimal_telemetry()).await;
  let absent_received = received_telemetry(&petstore).body.any_value;

  assert!(
    matches!(null_response, client::ApiResponse::Created(_)),
    "expected 201, got {null_response:?}"
  );
  assert!(
    matches!(absent_response, client::ApiResponse::Created(_)),
    "expected 201, got {absent_response:?}"
  );
  assert_eq!((null_received, absent_received), (None, None));
}

#[rstest]
#[tokio::test]
async fn explicit_nulls_are_accepted_for_nullable_fields(#[future(awt)] petstore: Petstore) {
  let mut body = minimal_telemetry_json();
  body["notes"] = Value::Null;
  body["threshold"] = Value::Null;

  let response = post_raw_telemetry(&petstore, &body).await;

  assert_eq!(response.status(), StatusCode::CREATED);
  let received = received_telemetry(&petstore).body;
  assert_eq!((received.notes, received.threshold), (None, None));
}

#[rstest]
#[case::path_collar_id_off_pattern("COL-0a1b2c3d", |_: &mut client::CollarTelemetry| {}, "collar_id")]
#[case::body_collar_id_off_pattern(COLLAR_ID, |telemetry: &mut client::CollarTelemetry| telemetry.collar_id = "COL-0A1B2C3".to_string(), "collar_id")]
#[case::no_samples(COLLAR_ID, |telemetry: &mut client::CollarTelemetry| telemetry.samples.clear(), "samples")]
#[case::one_location_item(COLLAR_ID, |telemetry: &mut client::CollarTelemetry| telemetry.location.truncate(1), "location")]
#[case::three_location_items(COLLAR_ID, |telemetry: &mut client::CollarTelemetry| telemetry.location.push(0.0), "location")]
#[case::battery_over_100(COLLAR_ID, |telemetry: &mut client::CollarTelemetry| telemetry.battery_percent = Some(101), "battery_percent")]
#[case::battery_not_a_multiple_of_five(COLLAR_ID, |telemetry: &mut client::CollarTelemetry| telemetry.battery_percent = Some(87), "battery_percent")]
#[case::short_pairing_secret(COLLAR_ID, |telemetry: &mut client::CollarTelemetry| telemetry.pairing_secret = Some("eleven-char".to_string()), "pairing_secret")]
#[case::empty_zones(COLLAR_ID, |telemetry: &mut client::CollarTelemetry| telemetry.zones = Some(IndexSet::new()), "zones")]
#[case::signal_at_exclusive_minimum(COLLAR_ID, |telemetry: &mut client::CollarTelemetry| telemetry.signal_quality = Some(0.0), "signal_quality")]
#[case::signal_at_exclusive_maximum(COLLAR_ID, |telemetry: &mut client::CollarTelemetry| telemetry.signal_quality = Some(1.0), "signal_quality")]
#[case::negative_sequence(COLLAR_ID, |telemetry: &mut client::CollarTelemetry| telemetry.sequence = Some(-1), "sequence")]
#[case::malformed_owner_email(COLLAR_ID, |telemetry: &mut client::CollarTelemetry| telemetry.owner_email = Some("owner-at-petstore".to_string()), "owner_email")]
fn the_client_rejects_telemetry_that_breaks_the_schema(
  #[case] collar_id: &str,
  #[case] tamper: fn(&mut client::CollarTelemetry),
  #[case] field: &str,
) {
  let mut telemetry = full_telemetry();
  tamper(&mut telemetry);

  let error = client::SubmitCollarTelemetryRequest::builder()
    .collar_id(collar_id.to_string())
    .body(telemetry)
    .build()
    .expect_err("the client rejects telemetry that breaks the schema");

  assert!(
    format!("{error:#}").contains(field),
    "the error `{error:#}` names {field}"
  );
}

#[rstest]
#[case::collar_id_off_pattern(|body: &mut Value| body["collarId"] = json!("COL-zz"))]
#[case::three_location_items(|body: &mut Value| body["location"] = json!([45.52, -122.68, 10.0]))]
#[case::no_samples(|body: &mut Value| body["samples"] = json!([]))]
#[case::battery_over_100(|body: &mut Value| body["batteryPercent"] = json!(101))]
#[case::battery_not_a_multiple_of_five(|body: &mut Value| body["batteryPercent"] = json!(87))]
#[tokio::test]
async fn the_service_answers_400_with_an_error_for_telemetry_that_fails_validation(
  #[future(awt)] petstore: Petstore,
  #[case] tamper: fn(&mut Value),
) {
  let mut body = minimal_telemetry_json();
  tamper(&mut body);

  let response = post_raw_telemetry(&petstore, &body).await;

  let status = response.status();
  let text = response.text().await.expect("the response has a body");
  assert_eq!(status, StatusCode::BAD_REQUEST, "{body} was answered with {text:?}");
  let error = serde_json::from_str::<client::Error>(&text)
    .unwrap_or_else(|parse_error| panic!("the 400 body {text:?} is not an Error: {parse_error}"));
  assert_eq!(error.code, 400);
  assert!(
    matches!(
      petstore.service.last_received(),
      Some(Received::SubmitCollarTelemetry(_))
    ),
    "the service saw {body} before rejecting it"
  );
}

/// Known limitation: the generated server can't build the spec's `Error` body, so it rejects undecodable bodies with a text reason.
#[rstest]
#[case::unparseable_recorded_at(|body: &mut Value| body["recordedAt"] = json!("yesterday"), "recordedAt")]
#[case::text_for_samples(|body: &mut Value| body["samples"] = json!("72,75"), "samples")]
#[tokio::test]
async fn the_server_answers_400_with_a_text_reason_for_telemetry_it_cannot_decode(
  #[future(awt)] petstore: Petstore,
  #[case] tamper: fn(&mut Value),
  #[case] field: &str,
) {
  let mut body = minimal_telemetry_json();
  tamper(&mut body);

  let response = post_raw_telemetry(&petstore, &body).await;

  let status = response.status();
  let content_type = response.headers()[CONTENT_TYPE].to_str().map(str::to_string);
  let reason = response.text().await.expect("the response has a body");
  assert_eq!(status, StatusCode::BAD_REQUEST, "{body} was answered with {reason:?}");
  assert!(
    content_type
      .as_deref()
      .is_ok_and(|value| value.starts_with("text/plain")),
    "the reason is plain text, got {content_type:?}"
  );
  assert!(
    reason.starts_with("Bad request: "),
    "the reason {reason:?} is a bad-request message"
  );
  assert!(reason.contains(field), "the reason {reason:?} names {field}");
  assert!(
    petstore.service.received().is_empty(),
    "the service must not see {body}"
  );
}

/// `uniqueItems: true` makes a repeated zone invalid, so the generated server rejects the body
/// before the service runs, naming the duplicate.
#[rstest]
#[tokio::test]
async fn duplicate_zones_are_rejected(#[future(awt)] petstore: Petstore) {
  let mut body = minimal_telemetry_json();
  body["zones"] = json!(["den", "den"]);

  let response = post_raw_telemetry(&petstore, &body).await;

  assert_eq!(response.status(), StatusCode::BAD_REQUEST);
  let reason = response.text().await.expect("a plain-text reason");
  assert!(reason.starts_with("Bad request: "), "{reason}");
  assert!(reason.contains("zones"), "{reason}");
  assert!(!matches!(
    petstore.service.last_received(),
    Some(Received::SubmitCollarTelemetry(_))
  ));
}

/// An adoption that names a `callbackUrl` gets the `statusChanged` callback the document
/// declares. A second store plays the partner, and its generated server receives the
/// callback at `/callbacks/adoptPet/statusChanged`.
#[rstest]
#[tokio::test]
async fn adopting_delivers_the_status_callback_to_the_partner(#[future(awt)] petstore: Petstore) {
  let partner = Petstore::start().await;
  let pet_id = create_pet(&petstore, "Biscuit", client::Species::Dog).await;

  let response = adopt(
    &petstore,
    client::AdoptionRequest {
      callback_url: Some(partner.url("/callbacks/adoptPet/statusChanged")),
      ..application(pet_id)
    },
  )
  .await;

  let client::ApiResponse::Accepted(client::WithHeaders { body: adoption, .. }) = response else {
    panic!("expected 202 Accepted, got {response:?}");
  };
  assert_eq!(
    petstore.service.with_state(|state| state.callback_deliveries.clone()),
    [204],
    "the partner acknowledged the callback"
  );
  let Some(Received::AdoptPetStatusChanged(callback)) = partner.service.last_received() else {
    panic!("the partner did not receive the callback");
  };
  assert_eq!(callback.body.adoption_id, adoption.id);
  assert_eq!(callback.body.pet_id, Some(pet_id));
  assert_eq!(callback.body.status, server::AdoptionStatus::Submitted);
  assert_eq!(callback.body.occurred_at, now());
}

/// A callback is a request the API sends, so its receiver doesn't inherit the API's
/// top-level `security`: it needs no API key. Its body is still decoded before the service.
#[rstest]
#[case::valid(
  json!({ "adoptionId": 7, "status": "approved", "occurredAt": "2026-10-09T18:00:00Z" }),
  StatusCode::NO_CONTENT
)]
#[case::unknown_status(
  json!({ "adoptionId": 7, "status": "lost", "occurredAt": "2026-10-09T18:00:00Z" }),
  StatusCode::BAD_REQUEST
)]
#[case::missing_adoption_id(
  json!({ "status": "approved", "occurredAt": "2026-10-09T18:00:00Z" }),
  StatusCode::BAD_REQUEST
)]
#[tokio::test]
async fn callback_receivers_need_no_credentials(
  #[future(awt)] petstore: Petstore,
  #[case] body: Value,
  #[case] expected: StatusCode,
) {
  let response = reqwest::Client::new()
    .post(petstore.url("/callbacks/adoptPet/statusChanged"))
    .json(&body)
    .send()
    .await
    .expect("the callback request completes");

  assert_eq!(response.status(), expected, "{body}");
  let received = matches!(
    petstore.service.last_received(),
    Some(Received::AdoptPetStatusChanged(_))
  );
  assert_eq!(received, expected.is_success());
}

/// `TRACE` is generated like any other method; the health endpoint echoes the request line
/// without credentials.
#[rstest]
#[tokio::test]
async fn tracing_the_health_endpoint_echoes_the_request_line(#[future(awt)] petstore: Petstore) {
  let response = petstore
    .anonymous()
    .trace_health(client::TraceHealthRequest {})
    .await
    .expect("the trace completes");

  let client::ApiResponse::Ok(echo) = response else {
    panic!("expected 200, got {response:?}");
  };
  assert_eq!(echo, "TRACE /health");
  assert!(matches!(
    petstore.service.last_received(),
    Some(Received::TraceHealth(_))
  ));
}

async fn inventory(petstore: &Petstore) -> IndexMap<String, i32> {
  let response = petstore
    .client
    .get_inventory(client::GetInventoryRequest {})
    .await
    .expect("the inventory request completes");
  let client::ApiResponse::Ok(inventory) = response else {
    panic!("expected 200, got {response:?}");
  };
  inventory.additional_properties
}

#[rstest]
#[tokio::test]
async fn inventory_counts_available_pets_by_species_wire_name(#[future(awt)] petstore: Petstore) {
  create_pet(&petstore, "Biscuit", client::Species::Dog).await;
  let adopted_dog = create_pet(&petstore, "Rex", client::Species::Dog).await;
  create_pet(&petstore, "Mochi", client::Species::Cat).await;
  create_pet(&petstore, "Clover", client::Species::SmallMammal).await;
  create_pet(&petstore, "Juniper", client::Species::SmallMammal).await;
  adopt(&petstore, application(adopted_dog)).await;

  let counts = inventory(&petstore).await;

  assert_eq!(
    counts,
    IndexMap::from([
      ("dog".to_string(), 1),
      ("cat".to_string(), 1),
      ("small_mammal".to_string(), 2),
    ])
  );
  assert!(matches!(last_received(&petstore), Received::GetInventory(_)));
}

async fn place_order(petstore: &Petstore, order: client::Order) -> client::ApiResponse<client::Order, client::Error> {
  let request = client::PlaceOrderRequest::builder()
    .body(order)
    .build()
    .expect("the order is valid");
  petstore
    .client
    .place_order(request)
    .await
    .expect("the order request completes")
}

async fn get_order(petstore: &Petstore, order_id: i64) -> client::ApiResponse<client::Order, client::Problem> {
  let request = client::GetOrderByIdRequest::builder()
    .order_id(order_id)
    .build()
    .expect("the order id is valid");
  petstore
    .client
    .get_order_by_id(request)
    .await
    .expect("the order lookup completes")
}

async fn delete_order(petstore: &Petstore, order_id: i64) -> client::ApiResponse<(), client::Problem> {
  let request = client::DeleteOrderRequest::builder()
    .order_id(order_id)
    .build()
    .expect("the order id is valid");
  petstore
    .client
    .delete_order(request)
    .await
    .expect("the cancellation completes")
}

#[rstest]
#[tokio::test]
async fn an_order_takes_the_spec_defaults_and_a_server_assigned_id(#[future(awt)] petstore: Petstore) {
  let pet_id = create_pet(&petstore, "Biscuit", client::Species::Dog).await;
  let order = client::Order {
    pet_id,
    ..client::Order::default()
  };
  assert_eq!((order.quantity, order.complete), (Some(1), Some(false)));

  let response = place_order(&petstore, order).await;

  let Received::PlaceOrder(received) = last_received(&petstore) else {
    panic!("the server did not record the order");
  };
  assert_eq!(
    received.body,
    server::Order {
      id: None,
      pet_id,
      quantity: Some(1),
      ship_date: None,
      status: None,
      complete: Some(false),
    }
  );
  let client::ApiResponse::Ok(placed) = response else {
    panic!("expected 200, got {response:?}");
  };
  let Some(id) = placed.id else {
    panic!("the server assigns the read-only id, got {placed:?}");
  };
  assert!(id >= 1);
  assert_eq!(
    placed,
    client::Order {
      id: Some(id),
      pet_id,
      quantity: Some(1),
      ship_date: None,
      status: Some(client::OrderStatus::Placed),
      complete: Some(false),
    }
  );
}

#[rstest]
#[tokio::test]
async fn an_order_is_fetched_and_cancelled_through_its_shared_path_parameter(#[future(awt)] petstore: Petstore) {
  let pet_id = create_pet(&petstore, "Biscuit", client::Species::Dog).await;
  let placed_response = place_order(
    &petstore,
    client::Order {
      pet_id,
      quantity: Some(3),
      ship_date: Some(now() + TimeDelta::days(2)),
      status: Some(client::OrderStatus::Approved),
      ..client::Order::default()
    },
  )
  .await;
  let client::ApiResponse::Ok(client::Order { id: Some(id), .. }) = placed_response else {
    panic!("expected 200 with an id, got {placed_response:?}");
  };

  let fetched = get_order(&petstore, id).await;
  let Received::GetOrderById(lookup) = last_received(&petstore) else {
    panic!("the server did not record the lookup");
  };
  let cancelled = delete_order(&petstore, id).await;
  let Received::DeleteOrder(cancellation) = last_received(&petstore) else {
    panic!("the server did not record the cancellation");
  };
  let after_cancelling = get_order(&petstore, id).await;

  let client::ApiResponse::Ok(fetched) = fetched else {
    panic!("expected 200, got {fetched:?}");
  };
  assert_eq!(
    (fetched.id, fetched.quantity, fetched.status),
    (Some(id), Some(3), Some(client::OrderStatus::Approved))
  );
  assert_eq!((lookup.path.order_id, cancellation.path.order_id), (id, id));
  assert!(
    matches!(cancelled, client::ApiResponse::NoContent(())),
    "expected 204, got {cancelled:?}"
  );
  let client::ApiResponse::NotFound(problem) = after_cancelling else {
    panic!("expected 404 after cancelling, got {after_cancelling:?}");
  };
  assert_eq!(problem.status, Some(404));
  assert_eq!(problem.instance, Some(format!("/store/orders/{id}")));
}

#[rstest]
#[tokio::test]
async fn unknown_orders_are_not_found_problems(#[future(awt)] petstore: Petstore) {
  let lookup = get_order(&petstore, BEYOND_DOUBLE_PRECISION).await;
  let cancellation = delete_order(&petstore, BEYOND_DOUBLE_PRECISION).await;

  let client::ApiResponse::NotFound(lookup_problem) = lookup else {
    panic!("expected 404 for the lookup, got {lookup:?}");
  };
  let client::ApiResponse::NotFound(cancellation_problem) = cancellation else {
    panic!("expected 404 for the cancellation, got {cancellation:?}");
  };
  assert_eq!(
    (lookup_problem.status, cancellation_problem.status),
    (Some(404), Some(404))
  );
  assert_eq!(
    lookup_problem.instance,
    Some(format!("/store/orders/{BEYOND_DOUBLE_PRECISION}"))
  );
}

#[rstest]
#[tokio::test]
async fn an_order_for_an_unknown_pet_is_a_bad_request(#[future(awt)] petstore: Petstore) {
  let unknown_pet = Uuid::from_u128(0xdead_beef);

  let response = place_order(
    &petstore,
    client::Order {
      pet_id: unknown_pet,
      ..client::Order::default()
    },
  )
  .await;

  let client::ApiResponse::BadRequest(error) = response else {
    panic!("expected 400, got {response:?}");
  };
  assert_eq!(error.code, 400);
  assert!(error.message.contains(&unknown_pet.to_string()), "{}", error.message);
}

#[rstest]
#[case::zero(0, false)]
#[case::one(1, true)]
#[case::ten(10, true)]
#[case::eleven(11, false)]
#[case::negative(-3, false)]
fn the_client_accepts_order_quantities_from_one_to_ten(#[case] quantity: i32, #[case] accepted: bool) {
  let order = client::Order {
    pet_id: Uuid::from_u128(1),
    quantity: Some(quantity),
    ..client::Order::default()
  };

  assert_eq!(
    client::PlaceOrderRequest::builder().body(order).build().is_ok(),
    accepted,
    "quantity {quantity}"
  );
}

#[rstest]
#[tokio::test]
async fn every_carrier_shape_parses(#[future(awt)] petstore: Petstore) {
  let response = petstore
    .client
    .list_carriers(client::ListCarriersRequest {})
    .await
    .expect("the carrier request completes");

  let client::ApiResponse::Ok(carriers) = response else {
    panic!("expected 200, got {response:?}");
  };
  assert_eq!(
    carriers,
    vec![
      client::PetCarrier {
        id: "rolling-crate".to_string(),
        wheels: Some(4),
        straps: None,
      },
      client::PetCarrier {
        id: "trail-pack".to_string(),
        wheels: None,
        straps: Some(2),
      },
      client::PetCarrier {
        id: "convertible".to_string(),
        wheels: Some(2),
        straps: Some(1),
      },
    ]
  );
}

/// Known limitation: `allOf` plus `anyOf` flatten into optional fields, so "at least one of wheels or straps" isn't enforced.
#[rstest]
#[tokio::test]
async fn a_carrier_with_neither_wheels_nor_straps_still_decodes(#[future(awt)] petstore: Petstore) {
  petstore.service.with_state(|state| {
    state.carriers = vec![server::PetCarrier {
      id: "bare-crate".to_string(),
      wheels: None,
      straps: None,
    }];
  });

  let response = petstore
    .client
    .list_carriers(client::ListCarriersRequest {})
    .await
    .expect("the carrier request completes");

  let client::ApiResponse::Ok(carriers) = response else {
    panic!("expected 200, got {response:?}");
  };
  assert_eq!(
    carriers,
    vec![client::PetCarrier {
      id: "bare-crate".to_string(),
      wheels: None,
      straps: None,
    }]
  );
}

#[rstest]
#[tokio::test]
async fn health_answers_204_without_credentials(#[future(awt)] petstore: Petstore) {
  let response = petstore
    .anonymous()
    .get_health(client::GetHealthRequest {})
    .await
    .expect("the health check completes");
  let raw = reqwest::get(petstore.url("/health"))
    .await
    .expect("the raw health check completes");

  assert!(
    matches!(response, client::ApiResponse::NoContent(())),
    "expected 204, got {response:?}"
  );
  assert!(matches!(last_received(&petstore), Received::GetHealth(_)));
  assert_eq!(raw.status(), StatusCode::NO_CONTENT);
  assert!(raw.bytes().await.expect("the body reads").is_empty());
}

#[rstest]
#[tokio::test]
async fn the_pet_adopted_webhook_records_the_typed_event(#[future(awt)] petstore: Petstore) {
  let event = client::AdoptionEvent {
    adoption_id: BEYOND_DOUBLE_PRECISION,
    pet_id: Some(Uuid::from_u128(5)),
    status: client::AdoptionStatus::UnderReview,
    occurred_at: now() + TimeDelta::milliseconds(125),
  };

  let response = reqwest::Client::new()
    .post(petstore.url("/webhooks/petAdopted"))
    .json(&event)
    .send()
    .await
    .expect("the webhook delivery completes");

  assert_eq!(response.status(), StatusCode::OK);
  let Received::PetAdopted(received) = last_received(&petstore) else {
    panic!("the server did not record the webhook");
  };
  assert_eq!(
    received.body,
    server::AdoptionEvent {
      adoption_id: BEYOND_DOUBLE_PRECISION,
      pet_id: Some(Uuid::from_u128(5)),
      status: server::AdoptionStatus::UnderReview,
      occurred_at: now() + TimeDelta::milliseconds(125),
    }
  );
}

#[rstest]
#[tokio::test]
async fn the_inventory_low_webhook_records_the_typed_alert(#[future(awt)] petstore: Petstore) {
  let alert = client::InventoryAlert {
    species: client::Species::SmallMammal,
    remaining: 2,
  };

  let response = reqwest::Client::new()
    .post(petstore.url("/webhooks/inventoryLow"))
    .json(&alert)
    .send()
    .await
    .expect("the webhook delivery completes");

  assert_eq!(response.status(), StatusCode::NO_CONTENT);
  let Received::InventoryLow(received) = last_received(&petstore) else {
    panic!("the server did not record the webhook");
  };
  assert_eq!(
    received.body,
    server::InventoryAlert {
      species: server::Species::SmallMammal,
      remaining: 2,
    }
  );
}

#[rstest]
#[case::adopted_without_occurred_at("/webhooks/petAdopted", r#"{"adoptionId":7,"status":"approved"}"#)]
#[case::adopted_with_unknown_status(
  "/webhooks/petAdopted",
  r#"{"adoptionId":7,"status":"lost","occurredAt":"2026-10-09T18:00:00Z"}"#
)]
#[case::adopted_with_text_id(
  "/webhooks/petAdopted",
  r#"{"adoptionId":"seven","status":"approved","occurredAt":"2026-10-09T18:00:00Z"}"#
)]
#[case::low_with_unknown_species("/webhooks/inventoryLow", r#"{"species":"dragon","remaining":1}"#)]
#[case::low_without_remaining("/webhooks/inventoryLow", r#"{"species":"cat"}"#)]
#[case::low_not_json("/webhooks/inventoryLow", "species=cat&remaining=1")]
#[tokio::test]
async fn webhooks_reject_bodies_they_cannot_decode_before_the_service_runs(
  #[future(awt)] petstore: Petstore,
  #[case] path: &str,
  #[case] body: &'static str,
) {
  let response = reqwest::Client::new()
    .post(petstore.url(path))
    .header(CONTENT_TYPE, "application/json")
    .body(body)
    .send()
    .await
    .expect("the webhook delivery completes");

  assert!(
    response.status().is_client_error(),
    "{path} answered {} for {body}",
    response.status()
  );
  assert!(
    petstore.service.received().is_empty(),
    "the service must not see {body}"
  );
}

/// The generated server leaves validation to the service, which records the alert and then answers an undeclared 400.
#[rstest]
#[tokio::test]
async fn the_service_answers_400_for_an_inventory_alert_with_negative_remaining(#[future(awt)] petstore: Petstore) {
  let response = reqwest::Client::new()
    .post(petstore.url("/webhooks/inventoryLow"))
    .header(CONTENT_TYPE, "application/json")
    .body(r#"{"species":"cat","remaining":-1}"#)
    .send()
    .await
    .expect("the webhook delivery completes");

  let status = response.status();
  let reason = response.text().await.expect("the response has a body");
  assert_eq!(status, StatusCode::BAD_REQUEST, "answered with {reason:?}");
  assert!(reason.contains("remaining"), "the reason {reason:?} names the field");
  let Received::InventoryLow(received) = last_received(&petstore) else {
    panic!("the service did not record the alert");
  };
  assert_eq!(received.body.remaining, -1);
}

/// Any instant from 1970 through 2099, to the nanosecond.
fn instant() -> impl Strategy<Value = DateTime<Utc>> {
  (0..4_102_444_800_i64, 0..1_000_000_000_u32)
    .prop_map(|(seconds, nanoseconds)| DateTime::from_timestamp(seconds, nanoseconds).expect("an instant in range"))
}

/// A species with the name it has on the wire.
fn species() -> impl Strategy<Value = (client::Species, &'static str)> {
  prop_oneof![
    Just((client::Species::Dog, "dog")),
    Just((client::Species::Cat, "cat")),
    Just((client::Species::Bird, "bird")),
    Just((client::Species::Fish, "fish")),
    Just((client::Species::Reptile, "reptile")),
    Just((client::Species::SmallMammal, "small_mammal")),
  ]
}

fn order_status() -> impl Strategy<Value = client::OrderStatus> {
  prop_oneof![
    Just(client::OrderStatus::Placed),
    Just(client::OrderStatus::Approved),
    Just(client::OrderStatus::Delivered),
  ]
}

prop_compose! {
  /// An order for `pet_id` within the schema: quantity 1..=10 and any ship date, status, and completion.
  fn valid_order(pet_id: Uuid)(
    quantity in 1..=10_i32,
    ship_date in prop::option::of(instant()),
    status in prop::option::of(order_status()),
    complete in prop::option::of(any::<bool>()),
  ) -> client::Order {
    client::Order { id: None, pet_id, quantity: Some(quantity), ship_date, status, complete }
  }
}

#[test]
fn inventory_counts_equal_the_available_pets_created_per_species() {
  let petstore = start_shared();
  proptest!(ProptestConfig::with_cases(32), |(pets in prop::collection::vec((species(), any::<bool>()), 0..12))| {
    run_case(async || {
      petstore.service.with_state(|state| state.pets.clear());
      for ((species, _), adopted) in &pets {
        let pet_id = create_pet(&petstore, "Pip", species.clone()).await;
        if *adopted {
          let response = adopt(&petstore, application(pet_id)).await;
          prop_assert!(matches!(response, client::ApiResponse::Accepted(_)), "adoption failed: {:?}", response);
        }
      }

      let expected = pets
        .iter()
        .filter(|(_, adopted)| !adopted)
        .fold(IndexMap::<String, i32>::new(), |mut counts, ((_, wire_name), _)| {
          *counts.entry((*wire_name).to_string()).or_default() += 1;
          counts
        });
      prop_assert_eq!(inventory(&petstore).await, expected);
      Ok::<_, TestCaseError>(())
    })?;
  });
}

#[test]
fn placed_orders_round_trip_through_get_order_by_id() {
  let petstore = start_shared();
  let pet_id = runtime().block_on(create_pet(&petstore, "Biscuit", client::Species::Dog));
  proptest!(ProptestConfig::with_cases(48), |(order in valid_order(pet_id))| {
    run_case(async || {
      let response = place_order(&petstore, order.clone()).await;
      let client::ApiResponse::Ok(placed) = response else {
        return Err(TestCaseError::fail(format!("expected 200, got {response:?}")));
      };
      let Some(id) = placed.id else {
        return Err(TestCaseError::fail(format!("the server assigned no id to {placed:?}")));
      };
      let expected = client::Order {
        id: Some(id),
        status: Some(order.status.clone().unwrap_or(client::OrderStatus::Placed)),
        complete: Some(order.complete.unwrap_or(false)),
        ..order
      };
      prop_assert_eq!(&placed, &expected);

      let response = get_order(&petstore, id).await;
      let client::ApiResponse::Ok(fetched) = response else {
        return Err(TestCaseError::fail(format!("expected 200 for order {id}, got {response:?}")));
      };
      prop_assert_eq!(fetched, placed);
      Ok::<_, TestCaseError>(())
    })?;
  });
}

#[test]
fn a_cancelled_order_is_no_longer_found() {
  let petstore = start_shared();
  let pet_id = runtime().block_on(create_pet(&petstore, "Biscuit", client::Species::Dog));
  proptest!(ProptestConfig::with_cases(32), |(order in valid_order(pet_id))| {
    run_case(async || {
      let response = place_order(&petstore, order).await;
      let client::ApiResponse::Ok(client::Order { id: Some(id), .. }) = response else {
        return Err(TestCaseError::fail(format!("expected 200 with an id, got {response:?}")));
      };

      let cancelled = delete_order(&petstore, id).await;
      prop_assert!(matches!(cancelled, client::ApiResponse::NoContent(())), "expected 204, got {:?}", cancelled);
      let lookup = get_order(&petstore, id).await;
      prop_assert!(
        matches!(&lookup, client::ApiResponse::NotFound(problem) if problem.status == Some(404)),
        "expected a 404 problem after cancelling, got {:?}",
        lookup
      );
      let cancelled_again = delete_order(&petstore, id).await;
      prop_assert!(
        matches!(cancelled_again, client::ApiResponse::NotFound(_)),
        "expected 404 when cancelling twice, got {:?}",
        cancelled_again
      );
      Ok::<_, TestCaseError>(())
    })?;
  });
}

//! Server-Sent Events, end to end.
//!
//! `streamPetVitals` and `streamAdoptionFeed` answer `text/event-stream`. The generated
//! server sends each item of the `BoxStream` its service returns as one `data:` JSON
//! event through axum's `Sse`, and the generated client reads them back as an
//! `EventStream` of `SseFrame`s holding typed items. These tests prove that items survive
//! the trip intact and in order, that query parameters narrow the stream, that each item
//! is exactly one `data:` event on the wire, and that a malformed event surfaces as an
//! `Err` item naming the JSON path under `data` that failed to decode.
//!
//! `streamAssistantMessage` and `streamAssistantTokens` stream frame types instead:
//! items whose `data`, `event`, `id`, and `retry` are the frame's own fields. These tests
//! prove each frame survives the trip whole, that a union of frame types picks its
//! member by the frame's event name, and that a sentinel ends the stream.

mod support;

use std::{collections::BTreeMap, convert::Infallible, time::Duration};

use axum::response::sse::{Event, Sse};
use chrono::{DateTime, TimeDelta, Utc};
use futures::{StreamExt as _, TryStreamExt as _};
use oas3_gen_support::{DiagnosticsError, SseFrame};
use petstore_client as client;
use petstore_server as server;
use proptest::prelude::*;
use rstest::rstest;
use serde_json::{Value, json};
use support::{API_KEY, Petstore, Received, now, petstore, run_case, start_shared};
use uuid::Uuid;

/// The instant `milliseconds` after the one the service stamps on everything.
fn at(milliseconds: i64) -> DateTime<Utc> {
  now() + TimeDelta::milliseconds(milliseconds)
}

fn decoded_activity(activity: &server::VitalsReadingActivity) -> client::VitalsReadingActivity {
  match activity {
    server::VitalsReadingActivity::Resting => client::VitalsReadingActivity::Resting,
    server::VitalsReadingActivity::Walking => client::VitalsReadingActivity::Walking,
    server::VitalsReadingActivity::Running => client::VitalsReadingActivity::Running,
    server::VitalsReadingActivity::Sleeping => client::VitalsReadingActivity::Sleeping,
  }
}

/// The reading the client must decode from the event the server sends for `reading`.
fn decoded_reading(reading: &server::VitalsReading) -> client::VitalsReading {
  client::VitalsReading {
    id: reading.id.clone(),
    heart_rate_bpm: reading.heart_rate_bpm,
    temperature_c: reading.temperature_c,
    activity: reading.activity.as_ref().map(decoded_activity),
    timestamp: reading.timestamp,
  }
}

fn decoded_event_type(event_type: &server::AdoptionFeedEventType) -> client::AdoptionFeedEventType {
  match event_type {
    server::AdoptionFeedEventType::Created => client::AdoptionFeedEventType::Created,
    server::AdoptionFeedEventType::Updated => client::AdoptionFeedEventType::Updated,
    server::AdoptionFeedEventType::Deleted => client::AdoptionFeedEventType::Deleted,
  }
}

/// The feed event the client must decode from the event the server sends for `event`.
fn decoded_event(event: &server::AdoptionFeedEvent) -> client::AdoptionFeedEvent {
  client::AdoptionFeedEvent {
    r#type: decoded_event_type(&event.r#type),
    payload: client::AdoptionFeedPayload {
      id: event.payload.id.clone(),
      name: event.payload.name.clone(),
      value: event.payload.value,
    },
  }
}

fn reading(id: &str, heart_rate_bpm: i32) -> server::VitalsReading {
  server::VitalsReading {
    id: id.to_string(),
    heart_rate_bpm,
    ..server::VitalsReading::default()
  }
}

fn vitals_request(pet_id: Uuid, since: Option<DateTime<Utc>>) -> client::StreamPetVitalsRequest {
  client::StreamPetVitalsRequest::builder()
    .pet_id(pet_id)
    .maybe_since(since)
    .build()
    .expect("the vitals request is valid")
}

/// Streams the pet's vitals through the generated client, failing on any event it can't decode.
async fn stream_vitals(
  client: &client::PetstoreClient,
  pet_id: Uuid,
  since: Option<DateTime<Utc>>,
) -> Vec<client::VitalsReading> {
  let response = client
    .stream_pet_vitals(vitals_request(pet_id, since))
    .await
    .expect("the vitals request completes");
  let client::ApiResponse::Ok(events) = response else {
    panic!("expected 200 with an event stream, got {response:?}");
  };
  events
    .map_ok(SseFrame::into_data)
    .try_collect::<Vec<_>>()
    .await
    .expect("every vitals event decodes")
}

/// Streams the adoption feed through the generated client, failing on any event it can't decode.
async fn stream_feed(client: &client::PetstoreClient, filter: Option<String>) -> Vec<client::AdoptionFeedEvent> {
  let request = client::StreamAdoptionFeedRequest::builder()
    .maybe_filter(filter)
    .build()
    .expect("the feed request is valid");
  let response = client
    .stream_adoption_feed(request)
    .await
    .expect("the feed request completes");
  let client::ApiResponse::Ok(events) = response else {
    panic!("expected 200 with an event stream, got {response:?}");
  };
  events
    .map_ok(SseFrame::into_data)
    .try_collect::<Vec<_>>()
    .await
    .expect("every feed event decodes")
}

/// Adds a dog named `name` and applies to adopt it, which puts one `created` event on the feed.
async fn adopt_new_pet(petstore: &Petstore, name: &str) -> client::Adoption {
  let new_pet = client::NewPet {
    name: name.to_string(),
    species: client::Species::Dog,
    ..client::NewPet::default()
  };
  let created = petstore
    .client
    .create_pet(
      client::CreatePetRequest::builder()
        .body(new_pet)
        .build()
        .expect("the pet is valid"),
    )
    .await
    .expect("the create request completes");
  let client::ApiResponse::Created(client::WithHeaders { body: pet, .. }) = created else {
    panic!("expected 201 for {name}, got {created:?}");
  };

  let application = client::AdoptionRequest {
    pet_id: pet.id,
    adopter: client::Adopter {
      name: "Avery Quinn".to_string(),
      email: "avery@example.com".to_string(),
      ..client::Adopter::default()
    },
    ..client::AdoptionRequest::default()
  };
  let adopted = petstore
    .client
    .adopt_pet(
      client::AdoptPetRequest::builder()
        .body(application)
        .build()
        .expect("the application is valid"),
    )
    .await
    .expect("the adoption request completes");
  let client::ApiResponse::Accepted(client::WithHeaders { body: adoption, .. }) = adopted else {
    panic!("expected 202 for {name}, got {adopted:?}");
  };
  adoption
}

/// A client for a stand-in server that answers every request with `data` as one SSE event per entry.
async fn client_for_raw_events(data: Vec<String>) -> client::PetstoreClient {
  let router = axum::Router::new().fallback(move || {
    let data = data.clone();
    async move {
      Sse::new(futures::stream::iter(
        data
          .into_iter()
          .map(|line| Ok::<_, Infallible>(Event::default().data(line))),
      ))
    }
  });
  let base_url = support::serve(router).await;
  client::PetstoreClient::with_base_url(&base_url).expect("the base URL parses")
}

#[rstest]
#[tokio::test]
async fn vitals_arrive_in_order_with_every_field_intact(#[future(awt)] petstore: Petstore) {
  let pet_id = Uuid::from_u128(7);
  let readings = vec![
    server::VitalsReading {
      id: "reading-1".to_string(),
      heart_rate_bpm: 72,
      temperature_c: Some(38.5),
      activity: Some(server::VitalsReadingActivity::Resting),
      timestamp: Some(at(0)),
    },
    server::VitalsReading {
      id: "reading-2".to_string(),
      heart_rate_bpm: 400,
      temperature_c: Some(-0.125),
      activity: Some(server::VitalsReadingActivity::Running),
      timestamp: Some(at(1_250)),
    },
    server::VitalsReading {
      id: "reading-3".to_string(),
      heart_rate_bpm: 0,
      temperature_c: Some(39.123_456_789),
      activity: Some(server::VitalsReadingActivity::Walking),
      timestamp: Some(at(-86_400_000)),
    },
    server::VitalsReading {
      activity: Some(server::VitalsReadingActivity::Sleeping),
      ..reading("reading-4", 55)
    },
    reading("reading-5", 61),
  ];
  petstore.service.with_state(|state| state.vitals = readings.clone());

  let decoded = stream_vitals(&petstore.client, pet_id, None).await;

  assert_eq!(decoded, readings.iter().map(decoded_reading).collect::<Vec<_>>());
  let Some(Received::StreamPetVitals(received)) = petstore.service.last_received() else {
    panic!("the server did not record a vitals request");
  };
  assert_eq!(received.path.pet_id, pet_id);
  assert_eq!(received.query.since, None);
}

#[rstest]
#[case::no_bound(None, &["before", "earlier", "latest", "undated"])]
#[case::exactly_the_first_reading(Some(at(-120_000)), &["earlier", "latest"])]
#[case::between_readings(Some(at(-90_000)), &["earlier", "latest"])]
#[case::fractional_seconds(Some(at(-60_500)), &["earlier", "latest"])]
#[case::exactly_the_last_reading(Some(at(0)), &[])]
#[case::after_every_reading(Some(at(3_600_000)), &[])]
#[tokio::test]
async fn since_replays_only_readings_recorded_after_it(
  #[future(awt)] petstore: Petstore,
  #[case] since: Option<DateTime<Utc>>,
  #[case] expected_ids: &[&str],
) {
  let readings = vec![
    server::VitalsReading {
      timestamp: Some(at(-120_000)),
      ..reading("before", 70)
    },
    server::VitalsReading {
      timestamp: Some(at(-60_000)),
      ..reading("earlier", 71)
    },
    server::VitalsReading {
      timestamp: Some(at(0)),
      ..reading("latest", 72)
    },
    reading("undated", 73),
  ];
  petstore.service.with_state(|state| state.vitals = readings);

  let decoded = stream_vitals(&petstore.client, Uuid::from_u128(1), since).await;

  assert_eq!(
    decoded.iter().map(|reading| reading.id.as_str()).collect::<Vec<_>>(),
    expected_ids,
    "readings streamed since {since:?}"
  );
  let Some(Received::StreamPetVitals(received)) = petstore.service.last_received() else {
    panic!("the server did not record a vitals request");
  };
  assert_eq!(
    received.query.since, since,
    "the date-time query parameter crosses the wire exactly"
  );
}

#[rstest]
#[tokio::test]
async fn empty_streams_end_cleanly_with_no_items(#[future(awt)] petstore: Petstore) {
  let vitals = stream_vitals(&petstore.client, Uuid::from_u128(2), None).await;
  let feed = stream_feed(&petstore.client, None).await;

  assert!(vitals.is_empty(), "no readings were recorded, got {vitals:?}");
  assert!(feed.is_empty(), "no adoptions were made, got {feed:?}");
}

#[rstest]
#[tokio::test]
async fn each_reading_is_one_data_event_of_json_on_the_wire(#[future(awt)] petstore: Petstore) {
  let readings = vec![
    server::VitalsReading {
      id: "reading-1".to_string(),
      heart_rate_bpm: 72,
      temperature_c: Some(38.5),
      activity: Some(server::VitalsReadingActivity::Running),
      timestamp: Some(at(250)),
    },
    reading("reading-2", 0),
  ];
  petstore.service.with_state(|state| state.vitals = readings);

  let response = reqwest::Client::new()
    .get(petstore.url(&format!("/pets/{}/vitals", Uuid::from_u128(3))))
    .header(client::X_API_KEY, API_KEY)
    .send()
    .await
    .expect("the raw request completes");

  assert_eq!(response.status(), reqwest::StatusCode::OK);
  assert_eq!(response.headers()[reqwest::header::CONTENT_TYPE], "text/event-stream");
  let body = response.text().await.expect("the event stream is text");
  let events = body.split_terminator("\n\n").collect::<Vec<_>>();
  let data = events
    .iter()
    .map(|event| {
      let json = event
        .strip_prefix("data: ")
        .unwrap_or_else(|| panic!("every event is a data field, got {event:?}"));
      assert!(!json.contains('\n'), "each item is a single data line, got {event:?}");
      serde_json::from_str::<Value>(json).unwrap_or_else(|error| panic!("{json:?} is not JSON: {error}"))
    })
    .collect::<Vec<_>>();
  assert_eq!(
    data,
    vec![
      json!({
        "id": "reading-1",
        "heartRateBpm": 72,
        "temperatureC": 38.5,
        "activity": "running",
        "timestamp": "2026-10-09T18:00:00.250Z",
      }),
      json!({ "id": "reading-2", "heartRateBpm": 0 }),
    ],
    "the raw stream was {body:?}"
  );
}

#[rstest]
#[tokio::test]
async fn adoptions_appear_on_the_feed(#[future(awt)] petstore: Petstore) {
  let biscuit = adopt_new_pet(&petstore, "Biscuit").await;
  let mochi = adopt_new_pet(&petstore, "Mochi").await;

  let feed = stream_feed(&petstore.client, None).await;

  let expected = [(biscuit, "Biscuit"), (mochi, "Mochi")]
    .into_iter()
    .map(|(adoption, name)| client::AdoptionFeedEvent {
      r#type: client::AdoptionFeedEventType::Created,
      payload: client::AdoptionFeedPayload {
        id: adoption.id.to_string(),
        name: Some(name.to_string()),
        value: Some(adoption.id),
      },
    })
    .collect::<Vec<_>>();
  assert_eq!(feed, expected);
}

#[rstest]
#[case::no_filter(None, &["Biscuit", "Mochi", "Bisque"])]
#[case::shared_prefix(Some("Bis"), &["Biscuit", "Bisque"])]
#[case::one_name(Some("Mochi"), &["Mochi"])]
#[case::inner_text(Some("cui"), &["Biscuit"])]
#[case::no_match(Some("Rex"), &[])]
#[tokio::test]
async fn the_feed_filter_narrows_events_by_payload_name(
  #[future(awt)] petstore: Petstore,
  #[case] filter: Option<&str>,
  #[case] expected_names: &[&str],
) {
  for name in ["Biscuit", "Mochi", "Bisque"] {
    adopt_new_pet(&petstore, name).await;
  }

  let feed = stream_feed(&petstore.client, filter.map(str::to_string)).await;

  assert_eq!(
    feed
      .iter()
      .map(|event| event.payload.name.as_deref().unwrap_or_default())
      .collect::<Vec<_>>(),
    expected_names,
    "events streamed for filter {filter:?}"
  );
  let Some(Received::StreamAdoptionFeed(received)) = petstore.service.last_received() else {
    panic!("the server did not record a feed request");
  };
  assert_eq!(received.query.filter.as_deref(), filter);
}

#[rstest]
#[case::text_for_an_integer(r#"{"id":"bad","heartRateBpm":"fast"}"#, "data.heartRateBpm")]
#[case::text_for_a_double(r#"{"id":"bad","heartRateBpm":80,"temperatureC":"warm"}"#, "data.temperatureC")]
#[case::unknown_enum_value(r#"{"id":"bad","heartRateBpm":80,"activity":"flying"}"#, "data.activity")]
#[case::unparseable_date_time(r#"{"id":"bad","heartRateBpm":80,"timestamp":"yesterday"}"#, "data.timestamp")]
#[tokio::test]
async fn a_malformed_vitals_event_is_an_error_item_naming_its_path(#[case] malformed: &str, #[case] path: &str) {
  let client = client_for_raw_events(vec![
    r#"{"id":"before","heartRateBpm":60}"#.to_string(),
    malformed.to_string(),
    r#"{"id":"after","heartRateBpm":61}"#.to_string(),
  ])
  .await;

  let response = client
    .stream_pet_vitals(vitals_request(Uuid::nil(), None))
    .await
    .expect("the vitals request completes");
  let client::ApiResponse::Ok(events) = response else {
    panic!("expected 200 with an event stream, got {response:?}");
  };
  let items = events.collect::<Vec<_>>().await;

  let [Ok(before), Err(error), Ok(after)] = items.as_slice() else {
    panic!("expected a reading, an error, then a reading; got {items:?}");
  };
  let DiagnosticsError::DeserializationError { path: error_path, .. } = error else {
    panic!("expected a JSON error for {malformed}, got {error:?}");
  };
  assert_eq!(error_path, path, "the error for {malformed} names the failing field");
  assert!(error.to_string().contains(path), "the message {error} names {path}");
  assert_eq!(
    (before.data().id.as_str(), after.data().id.as_str()),
    ("before", "after")
  );
}

#[rstest]
#[case::text_for_a_nested_integer(r#"{"type":"created","payload":{"id":"1","value":"seven"}}"#, "data.payload.value")]
#[case::missing_nested_id(r#"{"type":"created","payload":{"name":"Rex"}}"#, "data.payload")]
#[case::unknown_event_type(r#"{"type":"archived","payload":{"id":"1"}}"#, "data.type")]
#[tokio::test]
async fn a_malformed_feed_event_is_an_error_item_naming_its_path(#[case] malformed: &str, #[case] path: &str) {
  let client = client_for_raw_events(vec![
    r#"{"type":"created","payload":{"id":"before"}}"#.to_string(),
    malformed.to_string(),
    r#"{"type":"deleted","payload":{"id":"after"}}"#.to_string(),
  ])
  .await;

  let response = client
    .stream_adoption_feed(
      client::StreamAdoptionFeedRequest::builder()
        .build()
        .expect("the feed request is valid"),
    )
    .await
    .expect("the feed request completes");
  let client::ApiResponse::Ok(events) = response else {
    panic!("expected 200 with an event stream, got {response:?}");
  };
  let items = events.collect::<Vec<_>>().await;

  let [Ok(before), Err(error), Ok(after)] = items.as_slice() else {
    panic!("expected an event, an error, then an event; got {items:?}");
  };
  let DiagnosticsError::DeserializationError { path: error_path, .. } = error else {
    panic!("expected a JSON error for {malformed}, got {error:?}");
  };
  assert_eq!(error_path, path, "the error for {malformed} names the failing field");
  assert_eq!(
    (before.data().payload.id.as_str(), after.data().payload.id.as_str()),
    ("before", "after")
  );
}

/// Any instant from 1970 through 2099, to the nanosecond.
fn instant() -> impl Strategy<Value = DateTime<Utc>> {
  (0..4_102_444_800_i64, 0..1_000_000_000_u32)
    .prop_map(|(seconds, nanoseconds)| DateTime::from_timestamp(seconds, nanoseconds).expect("an instant in range"))
}

fn activity() -> impl Strategy<Value = server::VitalsReadingActivity> {
  prop_oneof![
    Just(server::VitalsReadingActivity::Resting),
    Just(server::VitalsReadingActivity::Walking),
    Just(server::VitalsReadingActivity::Running),
    Just(server::VitalsReadingActivity::Sleeping),
  ]
}

prop_compose! {
  /// A reading the schema allows: any text id, a heart rate in 0..=400, and any finite temperature.
  fn vitals_reading()(
    id in "(?s).{0,24}",
    heart_rate_bpm in 0..=400_i32,
    temperature_c in prop::option::of(any::<f64>().prop_filter("JSON numbers are finite", |value| value.is_finite())),
    activity in prop::option::of(activity()),
    timestamp in prop::option::of(instant()),
  ) -> server::VitalsReading {
    server::VitalsReading { id, heart_rate_bpm, temperature_c, activity, timestamp }
  }
}

fn feed_event_type() -> impl Strategy<Value = server::AdoptionFeedEventType> {
  prop_oneof![
    Just(server::AdoptionFeedEventType::Created),
    Just(server::AdoptionFeedEventType::Updated),
    Just(server::AdoptionFeedEventType::Deleted),
  ]
}

prop_compose! {
  /// A feed event whose payload name, when present, is drawn from a tiny alphabet so filters often match.
  fn feed_event()(
    event_type in feed_event_type(),
    id in "[0-9]{1,6}",
    name in prop::option::of("[abc]{0,4}"),
    value in prop::option::of(any::<i64>()),
  ) -> server::AdoptionFeedEvent {
    server::AdoptionFeedEvent {
      r#type: event_type,
      payload: server::AdoptionFeedPayload { id, name, value },
    }
  }
}

#[test]
fn any_vitals_sequence_streams_unchanged_and_in_order() {
  let petstore = start_shared();
  proptest!(ProptestConfig::with_cases(48), |(readings in prop::collection::vec(vitals_reading(), 0..50))| {
    run_case(async || {
      petstore.service.with_state(|state| state.vitals = readings.clone());

      let decoded = stream_vitals(&petstore.client, Uuid::nil(), None).await;

      prop_assert_eq!(decoded, readings.iter().map(decoded_reading).collect::<Vec<_>>());
      Ok::<_, TestCaseError>(())
    })?;
  });
}

#[test]
fn the_feed_streams_exactly_the_events_matching_any_filter() {
  let petstore = start_shared();
  proptest!(
    ProptestConfig::with_cases(48),
    |(events in prop::collection::vec(feed_event(), 0..30), filter in prop::option::of("[abc]{1,2}"))| {
      run_case(async || {
        petstore.service.with_state(|state| state.feed = events.clone());

        let streamed = stream_feed(&petstore.client, filter.clone()).await;

        let matching = events
          .iter()
          .filter(|event| {
            filter
              .as_deref()
              .is_none_or(|filter| event.payload.name.as_deref().is_some_and(|name| name.contains(filter)))
          })
          .map(decoded_event)
          .collect::<Vec<_>>();
        prop_assert_eq!(streamed, matching);
        let Some(Received::StreamAdoptionFeed(received)) = petstore.service.last_received() else {
          return Err(TestCaseError::fail("the server did not record a feed request"));
        };
        prop_assert_eq!(received.query.filter, filter);
        Ok::<_, TestCaseError>(())
      })?;
    }
  );
}

/// The frames `streamAssistantMessage` sends when asked to stream: one with every field
/// set, then one with only its data and event name.
fn assistant_frames() -> Vec<server::AssistantEventFrame> {
  vec![
    server::AssistantEventFrame {
      data: Box::new(server::AssistantEvent::MessageStop(server::MessageStopEvent::default())),
      event: server::AssistantEventFrameEvent::MessageStop,
      id: Some("evt-1".to_string()),
      retry: Some(1500),
    },
    server::AssistantEventFrame {
      data: Box::new(server::AssistantEvent::Ping(server::PingEvent::default())),
      event: server::AssistantEventFrameEvent::Ping,
      id: None,
      retry: None,
    },
  ]
}

fn summary_request() -> client::StreamAssistantMessageRequest {
  client::StreamAssistantMessageRequest::builder()
    .stream(true)
    .body(client::ContentRequest {
      blocks: vec![client::ContentBlock::text("Summarize Biscuit's visit".to_string())],
      ..Default::default()
    })
    .build()
    .expect("the message request is valid")
}

/// Streams the assistant's frames through the generated client.
async fn stream_frames(client: &client::PetstoreClient) -> Vec<Result<client::AssistantEventFrame, DiagnosticsError>> {
  let response = client
    .stream_assistant_message(summary_request())
    .await
    .expect("the message request completes");
  let client::ApiResponse::Ok(client::ContentResponseOrAssistantEventFrameStream::AssistantEventFrameStream(frames)) =
    response
  else {
    panic!("expected 200 with an event stream, got {response:?}");
  };
  frames.collect().await
}

/// Streams the assistant's token frames through the generated client.
async fn stream_tokens(client: &client::PetstoreClient) -> Vec<Result<client::AssistantTokenFrame, DiagnosticsError>> {
  let response = client
    .stream_assistant_tokens(client::StreamAssistantTokensRequest {})
    .await
    .expect("the token request completes");
  let client::ApiResponse::Ok(frames) = response else {
    panic!("expected 200 with an event stream, got {response:?}");
  };
  frames.collect::<Vec<_>>().await
}

/// What a test compares of a token frame: its event name, its data as JSON, its id, and
/// its retry time.
fn token_frame(frame: &client::AssistantTokenFrame) -> (&str, Value, Option<&str>, Option<Duration>) {
  match frame {
    client::AssistantTokenFrame::Token(token) => {
      (token.event().as_str(), json!(token.data()), token.id(), token.retry())
    }
    client::AssistantTokenFrame::Usage(usage) => {
      (usage.event().as_str(), json!(usage.data()), usage.id(), usage.retry())
    }
  }
}

/// The fields of each event in a raw `text/event-stream` body, with data lines joined.
fn wire_frames(body: &str) -> Vec<BTreeMap<String, String>> {
  body
    .split_terminator("\n\n")
    .map(|event| {
      let mut fields = BTreeMap::<String, String>::new();
      for line in event.lines() {
        let (field, value) = line.split_once(": ").unwrap_or((line, ""));
        fields
          .entry(field.to_string())
          .and_modify(|joined| {
            joined.push('\n');
            joined.push_str(value);
          })
          .or_insert_with(|| value.to_string());
      }
      fields
    })
    .collect()
}

fn frame(fields: &[(&str, &str)]) -> BTreeMap<String, String> {
  fields
    .iter()
    .map(|(field, value)| ((*field).to_string(), (*value).to_string()))
    .collect()
}

/// A client for a stand-in server that answers every request with `events` as its stream.
async fn client_for_raw_frames(events: Vec<Event>) -> client::PetstoreClient {
  let router = axum::Router::new().fallback(move || {
    let events = events.clone();
    async move { Sse::new(futures::stream::iter(events.into_iter().map(Ok::<_, Infallible>))) }
  });
  let base_url = support::serve(router).await;
  client::PetstoreClient::with_base_url(&base_url).expect("the base URL parses")
}

#[rstest]
#[tokio::test]
async fn assistant_frames_arrive_whole_and_end_at_the_sentinel(#[future(awt)] petstore: Petstore) {
  petstore
    .service
    .with_state(|state| state.assistant_frames = assistant_frames());

  let frames = stream_frames(&petstore.client)
    .await
    .into_iter()
    .collect::<Result<Vec<_>, _>>()
    .expect("every frame decodes");

  let read = frames
    .iter()
    .map(|frame| (frame.data().as_ref(), frame.event(), frame.id(), frame.retry()))
    .collect::<Vec<_>>();
  let retry = Some(Duration::from_millis(1500));
  assert_eq!(
    read,
    vec![
      (
        &client::AssistantEvent::MessageStop(client::MessageStopEvent::default()),
        &client::AssistantEventFrameEvent::MessageStop,
        Some("evt-1"),
        retry,
      ),
      (
        &client::AssistantEvent::Ping(client::PingEvent::default()),
        &client::AssistantEventFrameEvent::Ping,
        Some("evt-1"),
        retry,
      ),
    ],
    "a frame without an id or retry keeps the last ones the stream set"
  );
  let Some(Received::StreamAssistantMessage(received)) = petstore.service.last_received() else {
    panic!("the server did not record a message request");
  };
  assert_eq!(received.query.stream, Some(true));
}

#[rstest]
#[tokio::test]
async fn assistant_frames_are_whole_frames_on_the_wire_ending_with_done(#[future(awt)] petstore: Petstore) {
  petstore
    .service
    .with_state(|state| state.assistant_frames = assistant_frames());

  let response = reqwest::Client::new()
    .post(petstore.url("/assistant/messages/stream?stream=true"))
    .header(client::X_API_KEY, API_KEY)
    .json(&json!({ "blocks": [{ "type": "text", "text": "Summarize Biscuit's visit" }] }))
    .send()
    .await
    .expect("the raw request completes");

  assert_eq!(response.status(), reqwest::StatusCode::OK);
  assert_eq!(response.headers()[reqwest::header::CONTENT_TYPE], "text/event-stream");
  let body = response.text().await.expect("the event stream is text");
  assert_eq!(
    wire_frames(&body),
    vec![
      frame(&[
        ("data", r#"{"type":"message_stop"}"#),
        ("event", "message_stop"),
        ("id", "evt-1"),
        ("retry", "1500"),
      ]),
      frame(&[("data", r#"{"type":"ping"}"#), ("event", "ping")]),
      frame(&[("data", "[DONE]")]),
    ],
    "the raw stream was {body:?}"
  );
}

#[rstest]
#[tokio::test]
async fn token_frames_pick_their_member_by_event_name(#[future(awt)] petstore: Petstore) {
  petstore.service.with_state(|state| {
    state.token_frames = vec![
      server::AssistantTokenFrame::Token(server::TokenFrame {
        data: "Biscuit is".to_string(),
        ..Default::default()
      }),
      server::AssistantTokenFrame::Token(server::TokenFrame {
        data: "\"healthy\"\nand happy".to_string(),
        ..Default::default()
      }),
      server::AssistantTokenFrame::Usage(server::UsageFrame {
        data: server::Usage {
          input_tokens: 3,
          output_tokens: 2,
        },
        id: Some("usage-1".to_string()),
        ..Default::default()
      }),
    ];
  });

  let frames = stream_tokens(&petstore.client)
    .await
    .into_iter()
    .collect::<Result<Vec<_>, _>>()
    .expect("every frame decodes");

  assert_eq!(
    frames.iter().map(token_frame).collect::<Vec<_>>(),
    vec![
      ("token", json!("Biscuit is"), None, None),
      ("token", json!("\"healthy\"\nand happy"), None, None),
      (
        "usage",
        json!({ "input_tokens": 3, "output_tokens": 2 }),
        Some("usage-1"),
        None
      ),
    ],
    "token text arrives as written, and the usage frame decodes its JSON"
  );

  let body = reqwest::Client::new()
    .get(petstore.url("/assistant/tokens"))
    .header(client::X_API_KEY, API_KEY)
    .send()
    .await
    .expect("the raw request completes")
    .text()
    .await
    .expect("the event stream is text");
  assert_eq!(
    wire_frames(&body),
    vec![
      frame(&[("data", "Biscuit is"), ("event", "token")]),
      frame(&[("data", "\"healthy\"\nand happy"), ("event", "token")]),
      frame(&[
        ("data", r#"{"input_tokens":3,"output_tokens":2}"#),
        ("event", "usage"),
        ("id", "usage-1"),
      ]),
      frame(&[("data", "[END]")]),
    ],
    "token data is raw text, and the x-sse-sentinel ends the stream; the raw stream was {body:?}"
  );
}

#[rstest]
#[tokio::test]
async fn an_unknown_event_or_malformed_data_is_an_error_item() {
  let client = client_for_raw_frames(vec![
    Event::default().event("token").data("before"),
    Event::default().event("mystery").data("?"),
    Event::default()
      .event("usage")
      .data(r#"{"input_tokens":"many","output_tokens":1}"#),
    Event::default().event("token").data("after"),
    Event::default().data("[END]"),
    Event::default().event("token").data("ignored"),
  ])
  .await;

  let items = stream_tokens(&client).await;

  let [Ok(before), Err(unknown), Err(malformed), Ok(after)] = items.as_slice() else {
    panic!("expected a token, two errors, then a token, ending at the [END] sentinel; got {items:?}");
  };
  let DiagnosticsError::DeserializationError { path, inner } = unknown else {
    panic!("expected an unknown event error, got {unknown:?}");
  };
  assert_eq!(path, "event", "an unlisted event name is an error at the event");
  assert!(
    inner.to_string().contains("`mystery`"),
    "the error names the event: {inner}"
  );
  let DiagnosticsError::DeserializationError { path, .. } = malformed else {
    panic!("expected a JSON error, got {malformed:?}");
  };
  assert_eq!(path, "data.input_tokens", "the error names the field under data");
  for (item, text) in [(before, "before"), (after, "after")] {
    let client::AssistantTokenFrame::Token(token) = item else {
      panic!("expected a token frame, got {item:?}");
    };
    assert_eq!(token.data(), text);
  }
}

#[rstest]
#[case::event_with_a_line_break(Event::try_from(server::TokenFrame {
  event: "to\nken".to_string(),
  data: "text".to_string(),
}))]
#[case::id_with_a_carriage_return(Event::try_from(server::UsageFrame {
  id: Some("usage\r1".to_string()),
  ..Default::default()
}))]
#[case::id_with_a_nul(Event::try_from(server::UsageFrame {
  id: Some("usage\x001".to_string()),
  ..Default::default()
}))]
#[case::negative_retry(Event::try_from(server::AssistantEventFrame {
  data: Box::new(server::AssistantEvent::Ping(server::PingEvent::default())),
  event: server::AssistantEventFrameEvent::Ping,
  id: None,
  retry: Some(-1),
}))]
fn a_frame_axum_cannot_send_is_an_error_not_a_panic(#[case] event: Result<Event, axum::Error>) {
  assert!(event.is_err(), "the conversion refuses the frame");
}

#[test]
fn a_union_member_converts_to_one_axum_event() {
  let member = server::AssistantTokenFrame::Usage(server::UsageFrame {
    id: Some("usage-1".to_string()),
    ..Default::default()
  });
  assert!(Event::try_from(member).is_ok(), "a sendable frame converts");
}

use axum::extract::{FromRequest as _, Multipart};
use chrono::{NaiveDate, NaiveTime};
use indexmap::{IndexMap, IndexSet};
use petstore_client::{self as client, ApiResponse};
use petstore_server as server;
use proptest::{collection, option, prelude::*, sample::select};
use rstest::rstest;
use serde_json::{Value, json};
use uuid::Uuid;

use crate::{
  WireRequest, Wiretap, checked, property_config,
  support::{Petstore, now, run_case, start_shared},
};

const PNG: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];

fn biscuit() -> server::Pet {
  server::Pet {
    id: Uuid::from_u128(0xB15C),
    name: "Biscuit".to_string(),
    species: server::Species::Dog,
    status: Some(server::PetStatus::Available),
    ..Default::default()
  }
}

#[derive(Debug, Clone, PartialEq)]
struct Part {
  name: String,
  file_name: Option<String>,
  content_type: Option<String>,
  bytes: Vec<u8>,
}

impl Part {
  fn text(name: &str, value: &str) -> Self {
    Self {
      name: name.to_string(),
      file_name: None,
      content_type: None,
      bytes: value.as_bytes().to_vec(),
    }
  }

  fn typed(name: &str, content_type: &str, value: &str) -> Self {
    Self {
      content_type: Some(content_type.to_string()),
      ..Self::text(name, value)
    }
  }

  fn file(name: &str, content_type: &str, bytes: &[u8]) -> Self {
    Self {
      name: name.to_string(),
      file_name: Some(name.to_string()),
      content_type: Some(content_type.to_string()),
      bytes: bytes.to_vec(),
    }
  }
}

async fn parts(request: &WireRequest) -> anyhow::Result<Vec<Part>> {
  let content_type = request.content_type().unwrap_or_default();
  anyhow::ensure!(
    content_type.starts_with("multipart/form-data; boundary="),
    "sent as `{content_type}`"
  );
  let rebuilt = http::Request::builder()
    .header(http::header::CONTENT_TYPE, content_type)
    .body(axum::body::Body::from(request.body.clone()))?;
  let mut multipart = Multipart::from_request(rebuilt, &())
    .await
    .map_err(|rejection| anyhow::anyhow!("{rejection}"))?;
  let mut parts = vec![];
  while let Some(field) = multipart.next_field().await? {
    parts.push(Part {
      name: field.name().unwrap_or_default().to_string(),
      file_name: field.file_name().map(str::to_string),
      content_type: field.content_type().map(str::to_string),
      bytes: field.bytes().await?.to_vec(),
    });
  }
  Ok(parts)
}

fn full_intake_form() -> client::IntakeForm {
  client::IntakeForm {
    name: "Biscuit".to_string(),
    file_name: Some("records.pdf".to_string()),
    r#type: Some("stray".to_string()),
    resume: Some("Found near the caf\u{e9}".to_string()),
    file: Some(b"%PDF-1.7 records".to_vec()),
    attachments: vec![b"vet letter".to_vec(), vec![0x00, 0x9F, 0x92, 0x96, 0xFF]],
    thumbnail: PNG.to_vec(),
    encoded: b"hello intake".to_vec(),
    count: 42,
    ratio: Some(0.75),
    enabled: Some(true),
    quality: Some(client::CoatQuality::High),
    level: Some(client::EnergyLevel::Value2),
    tags: Some(vec!["friendly".to_string(), "house trained".to_string()]),
    created_at: Some(now()),
    address: Some(client::Address {
      city: "Portland".to_string(),
      zip: Some("97201".to_string()),
    }),
    addresses: Some(vec![
      client::Address {
        city: "Salem".to_string(),
        zip: None,
      },
      client::Address {
        city: "Eugene".to_string(),
        zip: Some("97401".to_string()),
      },
    ]),
    note: Some("# Intake\n\n*Shy* at first".to_string()),
    labels: Some(client::IntakeFormLabels::Object(IndexMap::from([
      ("coat".to_string(), "brindle".to_string()),
      ("eyes".to_string(), "amber".to_string()),
    ]))),
    ids: Some(vec![1, 2, 3]),
    colors: Some(vec!["brown".to_string(), "white".to_string()]),
    filter: Some(client::IntakeFilter {
      size: Some(3),
      color: Some("brown".to_string()),
    }),
    range: Some(client::AgeRange {
      min: Some(1),
      max: Some(5),
    }),
    extras: Some(client::IntakeExtras {
      mood: Some("calm".to_string()),
      score: Some(7),
    }),
    options: Some(IndexMap::from([("a".to_string(), 1), ("b".to_string(), 2)])),
  }
}

fn full_intake_form_parts() -> Vec<Part> {
  vec![
    Part::text("name", "Biscuit"),
    Part::text("fileName", "records.pdf"),
    Part::text("type", "stray"),
    Part::text("r\u{e9}sum\u{e9}", "Found near the caf\u{e9}"),
    Part::file("file", "application/octet-stream", b"%PDF-1.7 records"),
    Part::file("attachments", "application/octet-stream", b"vet letter"),
    Part::file(
      "attachments",
      "application/octet-stream",
      &[0x00, 0x9F, 0x92, 0x96, 0xFF],
    ),
    Part::file("thumbnail", "application/octet-stream", &PNG),
    Part::text("encoded", "aGVsbG8gaW50YWtl"),
    Part::text("count", "42"),
    Part::text("ratio", "0.75"),
    Part::text("enabled", "true"),
    Part::text("quality", "high"),
    Part::text("level", "2"),
    Part::text("tags", "friendly"),
    Part::text("tags", "house trained"),
    Part::text("createdAt", "2026-10-09T18:00:00Z"),
    Part::typed("address", "application/json", r#"{"city":"Portland","zip":"97201"}"#),
    Part::typed(
      "addresses",
      "application/vnd.petstore.address+json",
      r#"{"city":"Salem"}"#,
    ),
    Part::typed(
      "addresses",
      "application/vnd.petstore.address+json",
      r#"{"city":"Eugene","zip":"97401"}"#,
    ),
    Part::typed("note", "text/markdown", "# Intake\n\n*Shy* at first"),
    Part::typed("labels", "application/json", r#"{"coat":"brindle","eyes":"amber"}"#),
    Part::text("ids", "1|2|3"),
    Part::text("colors", "brown"),
    Part::text("colors", "white"),
    Part::text("filter[size]", "3"),
    Part::text("filter[color]", "brown"),
    Part::text("range", "min,1,max,5"),
    Part::text("mood", "calm"),
    Part::text("score", "7"),
    Part::text("options[a]", "1"),
    Part::text("options[b]", "2"),
  ]
}

fn json_lookalike_intake_form() -> client::IntakeForm {
  let null = || Some("null".to_string());
  client::IntakeForm {
    name: "null".to_string(),
    file_name: null(),
    note: null(),
    labels: Some(client::IntakeFormLabels::String("null".to_string())),
    tags: Some(vec!["null".to_string()]),
    filter: Some(client::IntakeFilter {
      size: None,
      color: null(),
    }),
    extras: Some(client::IntakeExtras {
      mood: null(),
      score: None,
    }),
    ..Default::default()
  }
}

fn json_lookalike_intake_form_parts() -> Vec<Part> {
  vec![
    Part::text("name", "null"),
    Part::text("fileName", "null"),
    Part::file("thumbnail", "application/octet-stream", &[]),
    Part::text("encoded", ""),
    Part::text("count", "0"),
    Part::text("tags", "null"),
    Part::typed("note", "text/markdown", "null"),
    Part::text("labels", "null"),
    Part::text("filter[color]", "null"),
    Part::text("mood", "null"),
  ]
}

enum Sent {
  Intake(Box<client::IntakeForm>),
  Media(client::MediumRequestBody),
  Tallies(IndexMap<String, Vec<i64>>),
  Photo(Vec<u8>),
  Notes(String),
  Patch(client::PetPatch),
  Signup(client::NewsletterSignup),
}

enum Framed {
  Multipart(Vec<Part>),
  Raw(&'static str, &'static [u8]),
}

async fn send(client: &client::PetstoreClient, pet_id: Uuid, sent: Sent) -> anyhow::Result<()> {
  match sent {
    Sent::Intake(form) => {
      let form = *form;
      let request = client::SubmitIntakeFormRequest::builder()
        .form_id("form-7".to_string())
        .body(form.clone())
        .build()?;
      let echo = answered!(client.submit_intake_form(request).await?, ApiResponse::Ok(echo) => echo);
      anyhow::ensure!(echo == form, "the server read {echo:?}");
    }
    Sent::Media(media) => {
      let sizes = media
        .file
        .iter()
        .flatten()
        .map(|file| i64::try_from(file.len()))
        .collect::<Result<Vec<_>, _>>()?;
      let request = client::UploadMediaRequest::builder().body(media).build()?;
      let receipt = answered!(client.upload_media(request).await?, ApiResponse::Ok(receipt) => receipt);
      anyhow::ensure!(receipt.sizes == sizes, "the server read {receipt:?}");
    }
    Sent::Tallies(tallies) => {
      let body = client::Tallies {
        additional_properties: tallies.clone(),
      };
      let request = client::SubmitTalliesRequest::builder().body(body).build()?;
      let echo = answered!(client.submit_tallies(request).await?, ApiResponse::Ok(echo) => echo);
      anyhow::ensure!(echo.additional_properties == tallies, "the server read {echo:?}");
    }
    Sent::Photo(photo) => {
      let request = client::ReplacePetPhotoRequest::builder()
        .pet_id(pet_id)
        .body(photo)
        .build()?;
      answered!(client.replace_pet_photo(request).await?, ApiResponse::NoContent(()) => ());
    }
    Sent::Notes(notes) => {
      let request = client::SetPetNotesRequest::builder()
        .pet_id(pet_id)
        .body(notes)
        .build()?;
      answered!(client.set_pet_notes(request).await?, ApiResponse::NoContent(()) => ());
    }
    Sent::Patch(patch) => {
      let request = client::UpdatePetRequest::builder().pet_id(pet_id).body(patch).build()?;
      answered!(client.update_pet(request).await?, ApiResponse::Ok(_) => ());
    }
    Sent::Signup(signup) => {
      let request = client::SubscribeNewsletterRequest::builder().body(signup).build()?;
      answered!(client.subscribe_newsletter(request).await?, ApiResponse::Created(_) => ());
    }
  }
  Ok(())
}

#[rstest]
#[case::multipart_follows_every_encoding_object(
  Sent::Intake(Box::new(full_intake_form())),
  Framed::Multipart(full_intake_form_parts())
)]
#[case::multipart_text_that_looks_like_json_stays_text(
  Sent::Intake(Box::new(json_lookalike_intake_form())),
  Framed::Multipart(json_lookalike_intake_form_parts())
)]
#[case::multipart_files_take_their_property_name_and_encoding(
  Sent::Media(client::MediumRequestBody {
    file: Some(vec![b"abc".to_vec(), vec![0xFF]]),
    cover: Some(PNG.to_vec()),
    note: Some("vacation".to_string()),
  }),
  Framed::Multipart(vec![
    Part::file("file", "application/octet-stream", b"abc"),
    Part::file("file", "application/octet-stream", &[0xFF]),
    Part::file("cover", "image/png", &PNG),
    Part::text("note", "vacation"),
  ])
)]
#[case::multipart_map_sends_one_part_per_item(
  Sent::Tallies(IndexMap::from([("daily walks".to_string(), vec![1, 2, 3]), ("naps".to_string(), vec![-4])])),
  Framed::Multipart(vec![
    Part::text("daily walks", "1"),
    Part::text("daily walks", "2"),
    Part::text("daily walks", "3"),
    Part::text("naps", "-4"),
  ])
)]
#[case::binary(Sent::Photo(PNG.to_vec()), Framed::Raw("image/png", &PNG))]
#[case::merge_patch(
  Sent::Patch(client::PetPatch { nickname: Some("Bis".to_string()), ..Default::default() }),
  Framed::Raw("application/merge-patch+json", br#"{"nickname":"Bis"}"#)
)]
#[case::url_encoded_form(
  Sent::Signup(client::NewsletterSignup {
    email: "ada+pets@example.com".to_string(),
    frequency: client::Frequency::Monthly,
    consent: Some(true),
    favorite_species: Some(client::Species::SmallMammal),
  }),
  Framed::Raw(
    "application/x-www-form-urlencoded",
    b"email=ada%2Bpets%40example.com&frequency=monthly&consent=true&favoriteSpecies=small_mammal"
  )
)]
#[tokio::test]
async fn bodies_are_framed_as_their_media_type_declares(
  #[case] sent: Sent,
  #[case] framed: Framed,
) -> anyhow::Result<()> {
  let wiretap = Wiretap::petstore().await?;
  let pet_id = wiretap.petstore.service.insert_pet(biscuit());

  send(wiretap.client(), pet_id, sent).await?;

  let request = wiretap.last()?;
  match framed {
    Framed::Multipart(expected) => assert_eq!(parts(&request).await?, expected),
    Framed::Raw(content_type, body) => {
      assert_eq!(request.content_type(), Some(content_type));
      assert_eq!(request.body, body);
    }
  }
  Ok(())
}

#[derive(Debug)]
struct NonJsonBodies {
  photo: Vec<u8>,
  notes: String,
  labels: IndexMap<String, String>,
  tallies: IndexMap<String, Vec<i64>>,
  intake: client::IntakeForm,
}

async fn non_json_bodies_round_trip(petstore: &Petstore, pet_id: Uuid, bodies: NonJsonBodies) -> anyhow::Result<()> {
  let client = &petstore.client;
  send(client, pet_id, Sent::Photo(bodies.photo.clone())).await?;
  let photo = received!(petstore.service, ReplacePetPhoto).body;
  anyhow::ensure!(photo == bodies.photo, "the photo arrived as {photo:?}");
  send(client, pet_id, Sent::Notes(bodies.notes.clone())).await?;
  let notes = received!(petstore.service, SetPetNotes).body;
  anyhow::ensure!(notes == bodies.notes, "the notes arrived as {notes:?}");
  let labels = client::MediaLabels {
    additional_properties: bodies.labels.clone(),
  };
  let request = client::SubmitMediaLabelsRequest::builder().body(labels).build()?;
  let echo = answered!(client.submit_media_labels(request).await?, ApiResponse::Ok(echo) => echo);
  anyhow::ensure!(
    echo.additional_properties == bodies.labels,
    "the labels arrived as {echo:?}"
  );
  send(client, pet_id, Sent::Tallies(bodies.tallies)).await?;
  send(client, pet_id, Sent::Intake(Box::new(bodies.intake))).await
}

fn unicode_text(length: std::ops::Range<usize>) -> impl Strategy<Value = String> {
  collection::vec(any::<char>(), length).prop_map(String::from_iter)
}

fn part_name() -> impl Strategy<Value = String> {
  let character = any::<char>().prop_filter("not expressible in a part name", |character| {
    !matches!(character, '"' | '\\') && (*character == '\t' || !character.is_control())
  });
  collection::vec(character, 1..12).prop_map(String::from_iter)
}

prop_compose! {
  fn non_json_bodies()(
    photo in collection::vec(any::<u8>(), 0..2048),
    notes in unicode_text(0..2000),
    labels in collection::vec((part_name(), unicode_text(1..24)), 0..6),
    tallies in collection::vec((part_name(), collection::vec(any::<i64>(), 1..5)), 0..6),
    name in unicode_text(1..24),
    note in option::of(unicode_text(0..48)),
    count in any::<i64>(),
    enabled in option::of(any::<bool>()),
    tags in option::of(collection::vec(unicode_text(0..12), 1..4)),
    ids in option::of(collection::vec(any::<i64>(), 1..6)),
  ) -> NonJsonBodies {
    NonJsonBodies {
      photo,
      notes,
      labels: labels.into_iter().collect(),
      tallies: tallies.into_iter().collect(),
      intake: client::IntakeForm { name, note, count, enabled, tags, ids, ..Default::default() },
    }
  }
}

#[rstest]
fn any_non_json_body_round_trips_unchanged() {
  let petstore = start_shared();
  let pet_id = petstore.service.insert_pet(biscuit());
  proptest!(property_config(32), |(bodies in non_json_bodies())| {
    checked(run_case(async || non_json_bodies_round_trip(&petstore, pet_id, bodies).await))?;
  });
}

fn with_spec_defaults(settings: client::AssistantSettings) -> client::AssistantSettings {
  client::AssistantSettings {
    model_selection: settings.model_selection.map(|selection| client::ModelSelection {
      model: selection.model.or(Some(client::ModelFilterModel::Enum(
        client::ModelFilterModelEnum::Model10,
      ))),
    }),
    response_kind: settings.response_kind.or(Some(client::ResponseKind::Json)),
    usage: settings.usage.or(Some(client::UsageChoice::Auto)),
    sample_rate: settings.sample_rate.or(Some(client::SampleRate::Value24000)),
    playback_rate: settings.playback_rate.or(Some(client::PlaybackRate::Value1)),
    ..settings
  }
}

enum JsonBody {
  Settings(Box<client::AssistantSettings>),
  Telemetry(Box<client::CollarTelemetry>),
}

fn every_format_telemetry() -> client::CollarTelemetry {
  client::CollarTelemetry {
    collar_id: "COL-0A1B2C3D".to_string(),
    recorded_at: now() + chrono::TimeDelta::milliseconds(250),
    recorded_on: NaiveDate::from_ymd_opt(2026, 10, 9),
    local_time: NaiveTime::from_hms_milli_opt(18, 30, 15, 250),
    location: vec![45.52, -122.68],
    samples: vec![72, 75],
    reading_id: Some(Uuid::from_u128(0x0123_4567_89ab_cdef_0123_4567_89ab_cdef)),
    firmware_image: Some("AAECAw==".to_string()),
    signature: Some(vec![0xde, 0xad, 0xbe, 0xef]),
    sequence: Some(9_007_199_254_740_993),
    zones: Some(IndexSet::from(["kitchen".to_string(), "yard".to_string()])),
    ..Default::default()
  }
}

async fn send_json(client: &client::PetstoreClient, body: JsonBody) -> anyhow::Result<()> {
  match body {
    JsonBody::Settings(settings) => {
      let settings = *settings;
      let request = client::UpdateAssistantSettingsRequest::builder()
        .body(settings.clone())
        .build()?;
      let echoed = answered!(client.update_assistant_settings(request).await?, ApiResponse::Ok(echoed) => echoed);
      anyhow::ensure!(echoed == with_spec_defaults(settings), "the server read {echoed:?}");
    }
    JsonBody::Telemetry(telemetry) => {
      let samples = i32::try_from(telemetry.samples.len())?;
      let request = client::SubmitCollarTelemetryRequest::builder()
        .collar_id(telemetry.collar_id.clone())
        .body(*telemetry)
        .build()?;
      let receipt = answered!(client.submit_collar_telemetry(request).await?, ApiResponse::Created(receipt) => receipt);
      anyhow::ensure!(receipt.reading_count == samples, "the server read {receipt:?}");
    }
  }
  Ok(())
}

#[rstest]
#[case::enums_of_every_json_type(
  JsonBody::Settings(Box::new(client::AssistantSettings {
    streaming: Some(client::Toggle::True),
    quality: Some(client::ResponseQuality::Value0_5),
    priority: Some(client::Priority::Value3),
    sample_rate: Some(client::SampleRate::Value44100),
    playback_rate: Some(client::PlaybackRate::Value2),
    stop_reason: Some(client::StopReason::StopSequence),
    ..Default::default()
  })),
  json!({
    "responseKind": "json",
    "usage": "auto",
    "streaming": true,
    "quality": 0.5,
    "priority": 3,
    "sampleRate": 44100,
    "playbackRate": 2.0,
    "stopReason": "stop_sequence",
  })
)]
#[case::string_formats(
  JsonBody::Telemetry(Box::new(every_format_telemetry())),
  json!({
    "collarId": "COL-0A1B2C3D",
    "recordedAt": "2026-10-09T18:00:00.250Z",
    "recordedOn": "2026-10-09",
    "localTime": "18:30:15.250",
    "location": [45.52, -122.68],
    "samples": [72, 75],
    "readingId": "01234567-89ab-cdef-0123-456789abcdef",
    "firmwareImage": "AAECAw==",
    "signature": "3q2+7w==",
    "sequence": 9_007_199_254_740_993_i64,
    "mode": "standard",
    "zones": ["kitchen", "yard"],
  })
)]
#[case::absent_members_are_omitted(
  JsonBody::Settings(Box::new(client::AssistantSettings {
    response_kind: None,
    usage: None,
    sample_rate: None,
    playback_rate: None,
    model_selection: Some(client::ModelSelection { model: None }),
    ..Default::default()
  })),
  json!({ "modelSelection": {} })
)]
#[tokio::test]
async fn json_bodies_carry_each_member_in_its_schema_form(
  #[case] body: JsonBody,
  #[case] wire: Value,
) -> anyhow::Result<()> {
  let wiretap = Wiretap::petstore().await?;

  send_json(wiretap.client(), body).await?;

  assert_eq!(serde_json::from_slice::<Value>(&wiretap.last()?.body)?, wire);
  Ok(())
}

fn web_url() -> impl Strategy<Value = String> {
  "https://[a-z]{1,12}\\.example/[a-z0-9]{0,12}"
}

fn json_value() -> impl Strategy<Value = Value> {
  let leaf = prop_oneof![
    Just(Value::Null),
    any::<bool>().prop_map(Value::Bool),
    any::<i64>().prop_map(Value::from),
    (-1.0e6..1.0e6).prop_map(Value::from),
    "\\PC{0,12}".prop_map(Value::String),
  ];
  leaf.prop_recursive(3, 24, 4, |inner| {
    prop_oneof![
      collection::vec(inner.clone(), 0..4).prop_map(Value::Array),
      collection::vec(("[a-zA-Z_]{1,8}", inner), 0..4).prop_map(|members| Value::Object(members.into_iter().collect())),
    ]
  })
}

fn free_form_object() -> impl Strategy<Value = IndexMap<String, Value>> {
  collection::vec(("x_[a-z]{1,6}", json_value()), 0..4).prop_map(|members| members.into_iter().collect())
}

prop_compose! {
  fn citation()(start in 0i64..500, length in 0i64..500, build in "\\PC{1,16}") -> client::CitationAnnotation {
    client::CitationAnnotation { start, end: start + length, build, ..Default::default() }
  }
}

prop_compose! {
  fn link()(start in 0i64..500, length in 0i64..500, url in web_url()) -> client::LinkAnnotation {
    client::LinkAnnotation { start, end: start + length, url, ..Default::default() }
  }
}

prop_compose! {
  fn recipe()(
    ingredients in option::of(free_form_object()),
    required in option::of(collection::vec("[a-z]{1,8}", 0..4)),
    additional_properties in free_form_object(),
  ) -> client::RecipeList {
    client::RecipeList { ingredients, required, additional_properties, ..Default::default() }
  }
}

prop_compose! {
  fn generated_text_block()(
    text in "\\PC{1,16}",
    annotations in option::of(collection::vec(prop_oneof![
      citation().prop_map(client::Annotation::Citation),
      link().prop_map(client::Annotation::Link),
    ], 0..3)),
    recipes in option::of(collection::vec(recipe(), 0..2)),
  ) -> client::TextBlock {
    client::TextBlock { text, annotations, recipes, ..Default::default() }
  }
}

fn image_block() -> impl Strategy<Value = client::ImageBlock> {
  let media_type = select(vec![
    client::MediaType::ImageJpeg,
    client::MediaType::ImagePng,
    client::MediaType::ImageGif,
    client::MediaType::ImageWebp,
  ]);
  let source = prop_oneof![
    (media_type, collection::vec(any::<u8>(), 0..48)).prop_map(|(media_type, data)| {
      client::ImageSource::Base64(client::Base64ImageSource {
        media_type,
        data,
        ..Default::default()
      })
    }),
    web_url().prop_map(client::ImageSource::url),
  ];
  (source, option::of("\\PC{0,16}")).prop_map(|(source, alt_text)| client::ImageBlock {
    source: Box::new(source),
    alt_text,
    ..Default::default()
  })
}

fn tool_result_block() -> impl Strategy<Value = client::ToolResultBlock> {
  let nested = prop_oneof![
    generated_text_block().prop_map(client::ToolResultContentBlock::Text),
    image_block().prop_map(client::ToolResultContentBlock::Image),
  ];
  let content = prop_oneof![
    "\\PC{0,24}".prop_map(client::ToolResultContent::String),
    collection::vec(nested, 0..3).prop_map(client::ToolResultContent::Array),
  ];
  let counter = prop_oneof![
    (0i64..10_000).prop_map(client::UsageCounterUsageCounterKind::a),
    (0i64..10_000).prop_map(client::UsageCounterUsageCounterKind::b),
  ];
  (
    "toolu_[a-z0-9]{1,12}",
    content,
    option::of(collection::vec(counter, 0..3)),
    option::of(any::<bool>()),
  )
    .prop_map(|(tool_use_id, content, iterations, is_error)| client::ToolResultBlock {
      tool_use_id,
      content,
      iterations,
      is_error,
      ..Default::default()
    })
}

fn content_block() -> impl Strategy<Value = client::ContentBlock> {
  prop_oneof![
    generated_text_block().prop_map(client::ContentBlock::Text),
    image_block().prop_map(client::ContentBlock::Image),
    ("\\PC{1,16}", option::of("[a-z]{1,10}")).prop_map(|(code, language)| client::ContentBlock::Code(
      client::CodeBlock {
        code,
        language,
        ..Default::default()
      }
    )),
    ("toolu_[a-z0-9]{1,12}", "[a-z_]{1,16}", free_form_object()).prop_map(|(id, name, input)| {
      client::ContentBlock::ToolUse(client::ToolUseBlock {
        id,
        name,
        input,
        ..Default::default()
      })
    }),
    tool_result_block().prop_map(client::ContentBlock::ToolResult),
  ]
}

fn model_union() -> impl Strategy<Value = client::ModelFilterModel> {
  select(vec![
    client::ModelFilterModel::Enum(client::ModelFilterModelEnum::Model10Draft),
    client::ModelFilterModel::Enum(client::ModelFilterModelEnum::Model10),
    client::ModelFilterModel::Enum(client::ModelFilterModelEnum::Model11),
    client::ModelFilterModel::Enum2(client::ModelFilterModelEnum2::Legacy01Mini),
    client::ModelFilterModel::Enum2(client::ModelFilterModelEnum2::Legacy01Plus),
  ])
}

prop_compose! {
  fn assistant_settings()(
    (model_filter, model_selection, model_requirement, model_choice) in (
      option::of(option::of(model_union())),
      option::of(option::of(model_union())),
      option::of(model_union()),
      option::of(prop_oneof![
        Just(client::ChatModelChoice::chat10()),
        Just(client::ChatModelChoice::chat11()),
        "[a-z][a-z0-9]{0,11}".prop_map(client::ChatModelChoice::String),
      ]),
    ),
    (response_kind, usage, streaming, quality, priority, temperature) in (
      option::of(select(vec![client::ResponseKind::Text, client::ResponseKind::Json])),
      option::of(prop_oneof![
        Just(client::UsageChoice::Auto),
        (0i64..1_000_000, 0i64..1_000_000).prop_map(|(input_tokens, output_tokens)| {
          client::UsageChoice::Usage(client::Usage { input_tokens, output_tokens })
        }),
        "[a-z]{1,10}".prop_filter("the constant reads as Auto", |choice| choice != "auto").prop_map(client::UsageChoice::String),
      ]),
      option::of(select(vec![client::Toggle::True, client::Toggle::False])),
      option::of(select(vec![
        client::ResponseQuality::Auto,
        client::ResponseQuality::Value1,
        client::ResponseQuality::Value0_5,
        client::ResponseQuality::True,
      ])),
      option::of(select(vec![client::Priority::Value1, client::Priority::Value2, client::Priority::Value3])),
      option::of(prop_oneof![
        (-100.0..100.0).prop_map(client::NullableStringOrNumber::Number),
        "\\PC{0,8}".prop_map(client::NullableStringOrNumber::String),
      ]),
    ),
    (highlights, citation, sample_rate, playback_rate, stop_reason, role) in (
      option::of(prop_oneof![
        generated_text_block().prop_map(client::ArrayOrSingle::TextBlock),
        collection::vec(generated_text_block(), 0..3).prop_map(client::ArrayOrSingle::Array),
      ]),
      option::of(prop_oneof![
        citation().prop_map(client::HypotheticalVersion::Citation),
        link().prop_map(client::HypotheticalVersion::Link),
      ]),
      option::of(select(vec![
        client::SampleRate::Value8000,
        client::SampleRate::Value16000,
        client::SampleRate::Value24000,
        client::SampleRate::Value44100,
        client::SampleRate::Value48000,
      ])),
      option::of(select(vec![
        client::PlaybackRate::Value0_5,
        client::PlaybackRate::Value1,
        client::PlaybackRate::Value1_5,
        client::PlaybackRate::Value2,
      ])),
      option::of(select(vec![
        client::StopReason::EndTurn,
        client::StopReason::MaxTokens,
        client::StopReason::StopSequence,
        client::StopReason::ToolUse,
      ])),
      option::of(select(vec![client::Role::User, client::Role::Assistant])),
    ),
  ) -> client::AssistantSettings {
    client::AssistantSettings {
      model_filter: model_filter.map(|model| client::ModelFilter { model }),
      model_selection: model_selection.map(|model| client::ModelSelection { model }),
      model_requirement: model_requirement.map(|model| client::ModelRequirement { model }),
      model_choice,
      response_kind,
      usage,
      streaming,
      quality,
      priority,
      temperature,
      highlights,
      citation,
      sample_rate,
      playback_rate,
      stop_reason,
      role,
    }
  }
}

async fn assistant_bodies_round_trip(
  petstore: &Petstore,
  blocks: Vec<client::ContentBlock>,
  settings: client::AssistantSettings,
) -> anyhow::Result<()> {
  let message = client::ContentRequest {
    blocks: blocks.clone(),
    ..Default::default()
  };
  let request = client::SendAssistantMessageRequest::builder().body(message).build()?;
  let reply = answered!(petstore.client.send_assistant_message(request).await?, ApiResponse::Ok(reply) => reply);
  let received = received!(petstore.service, SendAssistantMessage).body.blocks;
  anyhow::ensure!(
    serde_json::to_value(&received)? == serde_json::to_value(&blocks)?,
    "the server read {received:?}"
  );
  anyhow::ensure!(
    Some(reply.content.as_ref()) == blocks.last(),
    "the client read {reply:?}"
  );

  let expected = with_spec_defaults(settings.clone());
  let request = client::UpdateAssistantSettingsRequest::builder()
    .body(settings)
    .build()?;
  let echoed = answered!(petstore.client.update_assistant_settings(request).await?, ApiResponse::Ok(echoed) => echoed);
  let received = received!(petstore.service, UpdateAssistantSettings).body;
  anyhow::ensure!(
    serde_json::to_value(&received)? == serde_json::to_value(&expected)?,
    "the server read {received:?}"
  );
  anyhow::ensure!(echoed == expected, "the client read {echoed:?}");
  Ok(())
}

#[rstest]
fn any_assistant_body_round_trips_unchanged() {
  let petstore = start_shared();
  proptest!(property_config(48), |(blocks in collection::vec(content_block(), 1..4), settings in assistant_settings())| {
    checked(run_case(async || assistant_bodies_round_trip(&petstore, blocks, settings).await))?;
  });
}

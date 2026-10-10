//! Request and response bodies other than plain JSON, end to end.
//!
//! Covers `multipart/form-data` (including every Encoding Object on `submitIntakeForm`),
//! optional and map-shaped multipart bodies, `image/png` and `text/plain` bodies in both
//! directions, and the `application/x-www-form-urlencoded` newsletter signup. Round trips
//! run the generated client against the generated router and [`support::PetstoreService`];
//! wire checks swap the server for a capture router that records each request verbatim, so
//! the exact parts and media types the client emits can be compared with what the document
//! requires.

mod support;

use std::sync::{Arc, Mutex};

use axum::{
  body::Body,
  extract::{FromRequest as _, Multipart, Request, State},
  http::{HeaderMap, Method, StatusCode, header},
};
use indexmap::IndexMap;
use petstore_client::{self as client, ApiResponse, PetstoreClient, X_API_KEY};
use petstore_server as server;
use proptest::{collection, option, prelude::*, sample::SizeRange};
use reqwest::multipart::{Form, Part};
use rstest::rstest;
use serde::{Serialize, de::DeserializeOwned};
use support::{API_KEY, Petstore, Received, now, petstore, run_case, start_shared};
use uuid::Uuid;

/// The eight-byte signature every PNG file starts with.
const PNG_SIGNATURE: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];

/// The pet every media test attaches photos, notes, and images to.
const PET_ID: Uuid = Uuid::from_u128(0xB15C_0175);

/// Stores the dog Biscuit and returns its id.
fn seed_pet(petstore: &Petstore) -> Uuid {
  petstore.service.insert_pet(server::Pet {
    id: PET_ID,
    name: "Biscuit".to_string(),
    species: server::Species::Dog,
    status: Some(server::PetStatus::Available),
    ..server::Pet::default()
  })
}

/// The server-side twin of a client value, mapped through the JSON shape both share.
fn mirrored<Server: DeserializeOwned>(client_value: &impl Serialize) -> Server {
  let json = serde_json::to_value(client_value).expect("client values serialize");
  serde_json::from_value(json).expect("client and server types share one JSON shape")
}

/// `response` for a failure message, with an undeclared status's raw body shown as text.
fn outcome<Value: std::fmt::Debug, Failure: std::fmt::Debug>(response: &ApiResponse<Value, Failure>) -> String {
  match response {
    ApiResponse::Other(status, body) => format!("{status} {:?}", String::from_utf8_lossy(body)),
    declared => format!("{declared:?}"),
  }
}

/// The essence (`type/subtype`) of a `Content-Type` value, without parameters.
fn essence(content_type: &str) -> String {
  mediatype::MediaType::parse(content_type).map_or_else(
    |_| content_type.to_string(),
    |media_type| media_type.essence().to_string(),
  )
}

/// The `404` problem `response` carries, or a panic naming what came back instead.
fn not_found<Value: std::fmt::Debug>(response: ApiResponse<Value, client::Problem>) -> client::Problem {
  let ApiResponse::NotFound(problem) = response else {
    panic!("expected 404 Not Found, got {}", outcome(&response));
  };
  problem
}

/// One request exactly as it reached the capture router.
#[derive(Debug, Clone)]
struct CapturedRequest {
  method: Method,
  path: String,
  headers: HeaderMap,
  body: Vec<u8>,
}

impl CapturedRequest {
  /// The request's `Content-Type`, if it sent one.
  fn content_type(&self) -> Option<&str> {
    self
      .headers
      .get(header::CONTENT_TYPE)
      .and_then(|value| value.to_str().ok())
  }

  /// Every `multipart/form-data` part in the body, in wire order.
  async fn parts(&self) -> Vec<CapturedPart> {
    let content_type = self.content_type().expect("a multipart request names its boundary");
    let request = axum::http::Request::builder()
      .header(header::CONTENT_TYPE, content_type)
      .body(Body::from(self.body.clone()))
      .expect("a rebuilt request");
    let mut multipart = Multipart::from_request(request, &())
      .await
      .expect("a multipart/form-data body");
    let mut parts = vec![];
    while let Some(field) = multipart.next_field().await.expect("a well-formed part") {
      parts.push(CapturedPart {
        name: field.name().unwrap_or_default().to_string(),
        file_name: field.file_name().map(str::to_string),
        content_type: field.content_type().map(str::to_string),
        bytes: field.bytes().await.expect("the part's bytes").to_vec(),
      });
    }
    parts
  }
}

/// One `multipart/form-data` part as the client framed it.
#[derive(Debug, Clone, PartialEq)]
struct CapturedPart {
  name: String,
  file_name: Option<String>,
  content_type: Option<String>,
  bytes: Vec<u8>,
}

impl CapturedPart {
  /// A plain text part, which RFC 7578 reads as `text/plain` because it names no type.
  fn text(name: &str, value: &str) -> Self {
    Self {
      name: name.to_string(),
      file_name: None,
      content_type: None,
      bytes: value.as_bytes().to_vec(),
    }
  }

  /// A text part with an explicit media type.
  fn typed(name: &str, content_type: &str, value: &str) -> Self {
    Self {
      content_type: Some(content_type.to_string()),
      ..Self::text(name, value)
    }
  }

  /// A file part, which the generated client names after its property.
  fn file(name: &str, content_type: &str, bytes: &[u8]) -> Self {
    Self {
      name: name.to_string(),
      file_name: Some(name.to_string()),
      content_type: Some(content_type.to_string()),
      bytes: bytes.to_vec(),
    }
  }
}

/// Every request the capture router has seen, oldest first.
type CaptureLog = Arc<Mutex<Vec<CapturedRequest>>>;

/// A stand-in server that answers `202 Accepted` to everything and records each request.
struct CaptureServer {
  client: PetstoreClient,
  requests: CaptureLog,
}

impl CaptureServer {
  /// The single request the client sent.
  fn only_request(&self) -> CapturedRequest {
    let requests = self.requests.lock().expect("the capture log is never poisoned").clone();
    let [request] = requests.as_slice() else {
      panic!("expected exactly one request, got {requests:#?}");
    };
    request.clone()
  }
}

/// Records `request` verbatim and accepts it.
async fn record(State(requests): State<CaptureLog>, request: Request) -> StatusCode {
  let (parts, body) = request.into_parts();
  let body = axum::body::to_bytes(body, usize::MAX)
    .await
    .expect("the request body arrives");
  requests
    .lock()
    .expect("the capture log is never poisoned")
    .push(CapturedRequest {
      method: parts.method,
      path: parts.uri.path().to_string(),
      headers: parts.headers,
      body: body.to_vec(),
    });
  StatusCode::ACCEPTED
}

/// A capture server with an authenticated client pointed at it.
#[rstest::fixture]
async fn capture() -> CaptureServer {
  let requests = CaptureLog::default();
  let router = axum::Router::new().fallback(record).with_state(Arc::clone(&requests));
  let base_url = support::serve(router).await;
  let client = PetstoreClient::with_base_url(&base_url)
    .expect("the base URL parses")
    .with_api_key_auth(API_KEY);
  CaptureServer { client, requests }
}

/// A label map built from string pairs.
fn string_map<const N: usize>(entries: [(&str, &str); N]) -> IndexMap<String, String> {
  entries
    .into_iter()
    .map(|(key, value)| (key.to_string(), value.to_string()))
    .collect()
}

/// An intake form with every property the schema declares.
fn full_intake_form() -> client::IntakeForm {
  client::IntakeForm {
    name: "Biscuit".to_string(),
    file_name: Some("records.pdf".to_string()),
    r#type: Some("stray".to_string()),
    resume: Some("Found near the café".to_string()),
    file: Some(b"%PDF-1.7 records".to_vec()),
    attachments: vec![b"vet letter".to_vec(), vec![0x00, 0x9F, 0x92, 0x96, 0xFF]],
    thumbnail: PNG_SIGNATURE.to_vec(),
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
    labels: Some(client::IntakeFormLabels::Object(string_map([
      ("coat", "brindle"),
      ("eyes", "amber"),
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

/// The parts [`full_intake_form`] must produce, in schema order, as its Encoding Objects require.
fn full_intake_form_parts() -> Vec<CapturedPart> {
  vec![
    CapturedPart::text("name", "Biscuit"),
    CapturedPart::text("fileName", "records.pdf"),
    CapturedPart::text("type", "stray"),
    CapturedPart::text("résumé", "Found near the café"),
    CapturedPart::file("file", "application/octet-stream", b"%PDF-1.7 records"),
    CapturedPart::file("attachments", "application/octet-stream", b"vet letter"),
    CapturedPart::file(
      "attachments",
      "application/octet-stream",
      &[0x00, 0x9F, 0x92, 0x96, 0xFF],
    ),
    CapturedPart::file("thumbnail", "application/octet-stream", &PNG_SIGNATURE),
    CapturedPart::text("encoded", "aGVsbG8gaW50YWtl"),
    CapturedPart::text("count", "42"),
    CapturedPart::text("ratio", "0.75"),
    CapturedPart::text("enabled", "true"),
    CapturedPart::text("quality", "high"),
    CapturedPart::text("level", "2"),
    CapturedPart::text("tags", "friendly"),
    CapturedPart::text("tags", "house trained"),
    CapturedPart::text("createdAt", "2026-10-09T18:00:00Z"),
    CapturedPart::typed("address", "application/json", r#"{"city":"Portland","zip":"97201"}"#),
    CapturedPart::typed(
      "addresses",
      "application/vnd.petstore.address+json",
      r#"{"city":"Salem"}"#,
    ),
    CapturedPart::typed(
      "addresses",
      "application/vnd.petstore.address+json",
      r#"{"city":"Eugene","zip":"97401"}"#,
    ),
    CapturedPart::typed("note", "text/markdown", "# Intake\n\n*Shy* at first"),
    CapturedPart::typed("labels", "application/json", r#"{"coat":"brindle","eyes":"amber"}"#),
    CapturedPart::text("ids", "1|2|3"),
    CapturedPart::text("colors", "brown"),
    CapturedPart::text("colors", "white"),
    CapturedPart::text("filter[size]", "3"),
    CapturedPart::text("filter[color]", "brown"),
    CapturedPart::text("range", "min,1,max,5"),
    CapturedPart::text("mood", "calm"),
    CapturedPart::text("score", "7"),
    CapturedPart::text("options[a]", "1"),
    CapturedPart::text("options[b]", "2"),
  ]
}

/// A form with no files, numbers, or structured values, where every free-text value is `lookalike`.
fn sparse_intake_form(lookalike: &str) -> client::IntakeForm {
  client::IntakeForm {
    name: lookalike.to_string(),
    file_name: Some(lookalike.to_string()),
    r#type: Some(lookalike.to_string()),
    resume: Some(lookalike.to_string()),
    note: Some(lookalike.to_string()),
    labels: Some(client::IntakeFormLabels::String(lookalike.to_string())),
    tags: Some(vec![lookalike.to_string()]),
    colors: Some(vec![lookalike.to_string()]),
    filter: Some(client::IntakeFilter {
      size: None,
      color: Some(lookalike.to_string()),
    }),
    extras: Some(client::IntakeExtras {
      mood: Some(lookalike.to_string()),
      score: None,
    }),
    ..client::IntakeForm::default()
  }
}

/// Submits `form` as intake form `form-7`.
async fn submit_intake_form(client: &PetstoreClient, form: client::IntakeForm) -> ApiResponse<client::IntakeForm, ()> {
  let request = client::SubmitIntakeFormRequest::builder()
    .form_id("form-7".to_string())
    .body(form)
    .build()
    .expect("a valid intake form");
  client.submit_intake_form(request).await.expect("the form submits")
}

#[rstest]
#[case::every_property(full_intake_form())]
#[case::sparse_null_text(sparse_intake_form("null"))]
#[case::sparse_object_text(sparse_intake_form(r#"{"a":1}"#))]
#[case::sparse_boolean_text(sparse_intake_form("true"))]
#[case::sparse_number_text(sparse_intake_form("42"))]
#[case::sparse_array_text(sparse_intake_form("[1,2]"))]
#[case::no_previous_records(client::IntakeForm { file: None, ..full_intake_form() })]
#[case::empty_previous_records(client::IntakeForm { file: Some(vec![]), ..full_intake_form() })]
#[case::no_attachments(client::IntakeForm { attachments: vec![], ..full_intake_form() })]
#[case::every_byte_encoded(client::IntakeForm { encoded: (0..=u8::MAX).collect(), ..full_intake_form() })]
#[case::empty_encoded(client::IntakeForm { encoded: vec![], ..full_intake_form() })]
#[case::null_ratio(client::IntakeForm { ratio: None, ..full_intake_form() })]
#[case::inexact_ratio(client::IntakeForm { ratio: Some(0.1 + 0.2), ..full_intake_form() })]
#[case::large_negative_ratio(client::IntakeForm { ratio: Some(-1.5e300), ..full_intake_form() })]
#[case::string_labels(client::IntakeForm {
  labels: Some(client::IntakeFormLabels::String("brindle".to_string())),
  ..full_intake_form()
})]
#[case::null_labels(client::IntakeForm { labels: None, ..full_intake_form() })]
#[case::many_options(client::IntakeForm {
  options: Some(IndexMap::from([("zeta".to_string(), -1), ("alpha".to_string(), i64::MAX), ("mid".to_string(), 0)])),
  ..full_intake_form()
})]
#[tokio::test]
async fn intake_form_round_trips_unchanged(#[future(awt)] petstore: Petstore, #[case] form: client::IntakeForm) {
  let expected_on_server = mirrored::<server::IntakeForm>(&form);
  let expected_echo = form.clone();

  let response = submit_intake_form(&petstore.client, form).await;

  let ApiResponse::Ok(echo) = response else {
    panic!("expected 200 OK, got {}", outcome(&response));
  };
  assert_eq!(echo, expected_echo);
  let Some(Received::SubmitIntakeForm(received)) = petstore.service.last_received() else {
    panic!("the service did not receive the form");
  };
  assert_eq!(received.path.form_id, "form-7");
  assert_eq!(received.body, expected_on_server);
}

#[rstest]
#[tokio::test]
async fn intake_form_parts_follow_the_encoding_objects(#[future(awt)] capture: CaptureServer) {
  let response = submit_intake_form(&capture.client, full_intake_form()).await;
  assert!(
    matches!(response, ApiResponse::Other(StatusCode::ACCEPTED, _)),
    "{}",
    outcome(&response)
  );

  let request = capture.only_request();
  assert_eq!(request.method, Method::POST);
  assert_eq!(request.path, "/intake/form-7");
  let content_type = request.content_type().expect("a multipart content type");
  assert!(
    content_type.starts_with("multipart/form-data; boundary="),
    "unexpected request content type {content_type}"
  );
  let parts = request.parts().await;
  assert_eq!(parts, full_intake_form_parts());

  let thumbnail = parts
    .iter()
    .find(|part| part.name == "thumbnail")
    .expect("a thumbnail part");
  let thumbnail_type = thumbnail.content_type.as_deref().expect("a thumbnail media type");
  let media_type = mediatype::MediaType::parse(thumbnail_type).expect("a valid media type");
  assert!(
    media_type.ty != "*" && media_type.subty != "*",
    "the `image/*` encoding must resolve to a concrete media type, got {thumbnail_type}"
  );
}

#[rstest]
#[case::object(
  Some(client::IntakeFormLabels::Object(string_map([("coat", "brindle")]))),
  vec![CapturedPart::typed("labels", "application/json", r#"{"coat":"brindle"}"#)],
)]
#[case::plain_string(
  Some(client::IntakeFormLabels::String("brindle".to_string())),
  vec![CapturedPart::text("labels", "brindle")],
)]
#[case::object_lookalike_string(
  Some(client::IntakeFormLabels::String(r#"{"a":1}"#.to_string())),
  vec![CapturedPart::text("labels", r#"{"a":1}"#)],
)]
#[case::null_lookalike_string(
  Some(client::IntakeFormLabels::String("null".to_string())),
  vec![CapturedPart::text("labels", "null")],
)]
#[case::null(None, vec![])]
#[tokio::test]
async fn intake_labels_part_follows_the_value_shape(
  #[future(awt)] capture: CaptureServer,
  #[case] labels: Option<client::IntakeFormLabels>,
  #[case] expected: Vec<CapturedPart>,
) {
  submit_intake_form(
    &capture.client,
    client::IntakeForm {
      labels,
      ..full_intake_form()
    },
  )
  .await;

  let labels_parts = capture
    .only_request()
    .parts()
    .await
    .into_iter()
    .filter(|part| part.name == "labels")
    .collect::<Vec<_>>();
  assert_eq!(labels_parts, expected);
}

#[rstest]
#[case::complete_form(None, "7", StatusCode::OK, "")]
#[case::missing_file_part(
  Some("thumbnail"),
  "7",
  StatusCode::BAD_REQUEST,
  "missing multipart part `thumbnail`"
)]
#[case::missing_text_part(Some("name"), "7", StatusCode::BAD_REQUEST, "missing field `name`")]
#[case::missing_number_part(Some("count"), "7", StatusCode::BAD_REQUEST, "missing field `count`")]
#[case::non_numeric_count(None, "seven", StatusCode::BAD_REQUEST, "invalid multipart part `count`")]
#[tokio::test]
async fn intake_server_rejects_malformed_multipart(
  #[future(awt)] petstore: Petstore,
  #[case] omitted_part: Option<&str>,
  #[case] count: &str,
  #[case] expected_status: StatusCode,
  #[case] expected_reason: &str,
) {
  let parts = [
    ("name", Part::text("Biscuit")),
    (
      "thumbnail",
      Part::bytes(PNG_SIGNATURE.to_vec()).file_name("thumbnail.png"),
    ),
    ("encoded", Part::text("aGk=")),
    ("count", Part::text(count.to_string())),
  ];
  let form = parts
    .into_iter()
    .filter(|(name, _)| Some(*name) != omitted_part)
    .fold(Form::new(), |form, (name, part)| form.part(name, part));

  let response = reqwest::Client::new()
    .post(petstore.url("/intake/form-7"))
    .header(X_API_KEY, API_KEY)
    .multipart(form)
    .send()
    .await
    .expect("the server answers");

  assert_eq!(response.status(), expected_status);
  let body = response.text().await.expect("a text body");
  assert!(
    body.contains(expected_reason),
    "{body:?} does not mention {expected_reason:?}"
  );
  assert_eq!(
    petstore.service.received().len(),
    usize::from(expected_status.is_success())
  );
}

#[rstest]
#[case::intake_form("/intake/form-7")]
#[case::pet_image("/pets/00000000-0000-0000-0000-0000b15c0175/images")]
#[case::optional_media("/media")]
#[case::media_labels("/media/labels")]
#[case::tallies("/media/tallies")]
#[tokio::test]
async fn multipart_endpoints_reject_a_json_body(#[future(awt)] petstore: Petstore, #[case] path: &str) {
  seed_pet(&petstore);
  let response = reqwest::Client::new()
    .post(petstore.url(path))
    .header(X_API_KEY, API_KEY)
    .json(&serde_json::json!({ "name": "Biscuit", "count": 7, "note": "sent as JSON" }))
    .send()
    .await
    .expect("the server answers");

  assert_eq!(response.status(), StatusCode::BAD_REQUEST);
  assert_eq!(
    petstore.service.received().len(),
    0,
    "{:?}",
    petstore.service.received()
  );
}

#[rstest]
#[tokio::test]
async fn uploading_an_image_renames_the_pet_and_stores_the_photo(#[future(awt)] petstore: Petstore) {
  let pet_id = seed_pet(&petstore);
  let image = [PNG_SIGNATURE.as_slice(), &[0x00, 0xFF, 0x0D, 0x0A]].concat();
  let request = client::UploadPetImageRequest::builder()
    .pet_id(pet_id)
    .body(client::ImageRequestBody {
      name: "Pixel".to_string(),
      image: image.clone(),
    })
    .build()
    .expect("a valid upload");

  let response = petstore
    .client
    .upload_pet_image(request)
    .await
    .expect("the upload completes");

  let ApiResponse::Ok(pet) = response else {
    panic!("expected 200 OK, got {}", outcome(&response));
  };
  assert_eq!(pet.id, pet_id);
  assert_eq!(pet.name, "Pixel");
  assert_eq!(pet.photo_urls, Some(vec![format!("/pets/{pet_id}/photo")]));
  let Some(Received::UploadPetImage(received)) = petstore.service.last_received() else {
    panic!("the service did not receive the upload");
  };
  assert_eq!(received.path.pet_id, pet_id);
  assert_eq!(
    received.body,
    server::ImageRequestBody {
      name: "Pixel".to_string(),
      image: image.clone(),
    }
  );

  let photo = petstore
    .client
    .get_pet_photo(
      client::GetPetPhotoRequest::builder()
        .pet_id(pet_id)
        .build()
        .expect("a valid id"),
    )
    .await
    .expect("the download completes");
  let ApiResponse::Ok(bytes) = photo else {
    panic!("expected 200 OK, got {}", outcome(&photo));
  };
  assert_eq!(bytes, image);
}

#[rstest]
#[case::no_body(None, None, client::MediaReceipt::default())]
#[case::empty_file_list(
  Some(client::MediumRequestBody { file: Some(vec![]), cover: None, note: None }),
  Some(server::MediumRequestBody::default()),
  client::MediaReceipt::default(),
)]
#[case::files_cover_and_note(
  Some(client::MediumRequestBody {
    file: Some(vec![b"abc".to_vec(), vec![], vec![7; 5]]),
    cover: Some(PNG_SIGNATURE.to_vec()),
    note: Some("vacation".to_string()),
  }),
  Some(server::MediumRequestBody {
    file: Some(vec![b"abc".to_vec(), vec![], vec![7; 5]]),
    cover: Some(PNG_SIGNATURE.to_vec()),
    note: Some("vacation".to_string()),
  }),
  client::MediaReceipt { sizes: vec![3, 0, 5], note: Some("vacation".to_string()), cover_size: Some(8) },
)]
#[tokio::test]
async fn optional_media_upload_reports_what_arrived(
  #[future(awt)] petstore: Petstore,
  #[case] body: Option<client::MediumRequestBody>,
  #[case] expected_on_server: Option<server::MediumRequestBody>,
  #[case] expected_receipt: client::MediaReceipt,
) {
  let request = client::UploadMediaRequest::builder()
    .maybe_body(body)
    .build()
    .expect("a valid upload");

  let response = petstore
    .client
    .upload_media(request)
    .await
    .expect("the upload completes");

  let ApiResponse::Ok(receipt) = response else {
    panic!("expected 200 OK, got {}", outcome(&response));
  };
  assert_eq!(receipt, expected_receipt);
  let Some(Received::UploadMedia(received)) = petstore.service.last_received() else {
    panic!("the service did not receive the upload");
  };
  assert_eq!(received.body, expected_on_server);
}

#[rstest]
#[tokio::test]
async fn optional_media_parts_carry_filenames_and_the_cover_encoding(#[future(awt)] capture: CaptureServer) {
  let request = client::UploadMediaRequest::builder()
    .body(client::MediumRequestBody {
      file: Some(vec![b"abc".to_vec(), vec![0xFF]]),
      cover: Some(PNG_SIGNATURE.to_vec()),
      note: Some("vacation".to_string()),
    })
    .build()
    .expect("a valid upload");

  capture
    .client
    .upload_media(request)
    .await
    .expect("the upload completes");

  assert_eq!(
    capture.only_request().parts().await,
    vec![
      CapturedPart::file("file", "application/octet-stream", b"abc"),
      CapturedPart::file("file", "application/octet-stream", &[0xFF]),
      CapturedPart::file("cover", "image/png", &PNG_SIGNATURE),
      CapturedPart::text("note", "vacation"),
    ]
  );
}

#[rstest]
#[case::spaces_and_non_ascii_keys(string_map([("front door", "blue"), ("café", "crème brûlée"), ("名前", "ビスケット")]))]
#[case::json_lookalike_values(string_map([("count", "42"), ("flag", "true"), ("nothing", "null"), ("object", r#"{"a":1}"#)]))]
#[case::empty(IndexMap::new())]
#[tokio::test]
async fn media_labels_round_trip_unchanged(
  #[future(awt)] petstore: Petstore,
  #[case] labels: IndexMap<String, String>,
) {
  let request = client::SubmitMediaLabelsRequest::builder()
    .body(client::MediaLabels {
      additional_properties: labels.clone(),
    })
    .build()
    .expect("valid labels");

  let response = petstore
    .client
    .submit_media_labels(request)
    .await
    .expect("the labels submit");

  let ApiResponse::Ok(echo) = response else {
    panic!("expected 200 OK, got {}", outcome(&response));
  };
  assert_eq!(echo.additional_properties, labels);
  let Some(Received::SubmitMediaLabels(received)) = petstore.service.last_received() else {
    panic!("the service did not receive the labels");
  };
  assert_eq!(received.body.additional_properties, labels);
}

#[rstest]
#[case::several_lists(IndexMap::from([
  ("daily walks".to_string(), vec![1, 2, 3]),
  ("naps".to_string(), vec![i64::MIN, 0, i64::MAX]),
]))]
#[case::non_ascii_key(IndexMap::from([("名前".to_string(), vec![7])]))]
#[case::empty(IndexMap::new())]
#[tokio::test]
async fn tallies_round_trip_unchanged(#[future(awt)] petstore: Petstore, #[case] tallies: IndexMap<String, Vec<i64>>) {
  let request = client::SubmitTalliesRequest::builder()
    .body(client::Tallies {
      additional_properties: tallies.clone(),
    })
    .build()
    .expect("valid tallies");

  let response = petstore
    .client
    .submit_tallies(request)
    .await
    .expect("the tallies submit");

  let ApiResponse::Ok(echo) = response else {
    panic!("expected 200 OK, got {}", outcome(&response));
  };
  assert_eq!(echo.additional_properties, tallies);
  let Some(Received::SubmitTallies(received)) = petstore.service.last_received() else {
    panic!("the service did not receive the tallies");
  };
  assert_eq!(received.body.additional_properties, tallies);
}

#[rstest]
#[case::repeated_part_names(
  IndexMap::from([("daily walks".to_string(), vec![1, 2, 3]), ("naps".to_string(), vec![-4])]),
  vec![
    CapturedPart::text("daily walks", "1"),
    CapturedPart::text("daily walks", "2"),
    CapturedPart::text("daily walks", "3"),
    CapturedPart::text("naps", "-4"),
  ],
)]
#[case::empty(IndexMap::new(), vec![])]
#[tokio::test]
async fn tallies_send_one_part_per_count(
  #[future(awt)] capture: CaptureServer,
  #[case] tallies: IndexMap<String, Vec<i64>>,
  #[case] expected: Vec<CapturedPart>,
) {
  let request = client::SubmitTalliesRequest::builder()
    .body(client::Tallies {
      additional_properties: tallies,
    })
    .build()
    .expect("valid tallies");

  capture
    .client
    .submit_tallies(request)
    .await
    .expect("the tallies submit");

  let request = capture.only_request();
  let content_type = request.content_type().expect("a multipart content type");
  assert!(
    content_type.starts_with("multipart/form-data; boundary="),
    "{content_type}"
  );
  assert_eq!(request.parts().await, expected);
}

#[rstest]
#[case::empty(vec![])]
#[case::png_signature(PNG_SIGNATURE.to_vec())]
#[case::every_byte_value((0..=u8::MAX).collect())]
#[tokio::test]
async fn replaced_photo_downloads_byte_for_byte(#[future(awt)] petstore: Petstore, #[case] photo: Vec<u8>) {
  let pet_id = seed_pet(&petstore);
  let request = client::ReplacePetPhotoRequest::builder()
    .pet_id(pet_id)
    .body(photo.clone())
    .build()
    .expect("a valid photo");

  let replaced = petstore
    .client
    .replace_pet_photo(request)
    .await
    .expect("the upload completes");

  assert!(matches!(replaced, ApiResponse::NoContent(())), "{}", outcome(&replaced));
  let Some(Received::ReplacePetPhoto(received)) = petstore.service.last_received() else {
    panic!("the service did not receive the photo");
  };
  assert_eq!(received.body, photo);
  let downloaded = petstore
    .client
    .get_pet_photo(
      client::GetPetPhotoRequest::builder()
        .pet_id(pet_id)
        .build()
        .expect("a valid id"),
    )
    .await
    .expect("the download completes");
  let ApiResponse::Ok(bytes) = downloaded else {
    panic!("expected 200 OK, got {}", outcome(&downloaded));
  };
  assert_eq!(bytes, photo);
}

#[rstest]
#[case::photo("photo", "image/png", &PNG_SIGNATURE)]
#[case::kennel_card("card", "text/plain", b"Biscuit (dog)\nStatus: available\nNotes: Loves naps")]
#[tokio::test]
async fn downloads_declare_their_media_type(
  #[future(awt)] petstore: Petstore,
  #[case] resource: &str,
  #[case] media_type: &str,
  #[case] expected_body: &[u8],
) {
  let pet_id = seed_pet(&petstore);
  petstore.service.with_state(|state| {
    state.photos.insert(pet_id, PNG_SIGNATURE.to_vec());
    state.notes.insert(pet_id, "Loves naps".to_string());
  });

  let response = reqwest::Client::new()
    .get(petstore.url(&format!("/pets/{pet_id}/{resource}")))
    .header(X_API_KEY, API_KEY)
    .send()
    .await
    .expect("the server answers");

  assert_eq!(response.status(), StatusCode::OK);
  let content_type = response
    .headers()
    .get(header::CONTENT_TYPE)
    .and_then(|value| value.to_str().ok());
  assert_eq!(content_type.map(essence).as_deref(), Some(media_type));
  assert_eq!(response.bytes().await.expect("a body").as_ref(), expected_body);
}

#[rstest]
#[tokio::test]
async fn missing_photo_answers_with_a_problem_document(#[future(awt)] petstore: Petstore) {
  let response = reqwest::Client::new()
    .get(petstore.url(&format!("/pets/{PET_ID}/photo")))
    .header(X_API_KEY, API_KEY)
    .send()
    .await
    .expect("the server answers");

  assert_eq!(response.status(), StatusCode::NOT_FOUND);
  let content_type = response
    .headers()
    .get(header::CONTENT_TYPE)
    .and_then(|value| value.to_str().ok());
  assert_eq!(content_type.map(essence).as_deref(), Some("application/problem+json"));
}

#[rstest]
#[tokio::test]
async fn photo_upload_is_sent_as_image_png(#[future(awt)] capture: CaptureServer) {
  let request = client::ReplacePetPhotoRequest::builder()
    .pet_id(PET_ID)
    .body(PNG_SIGNATURE.to_vec())
    .build()
    .expect("a valid photo");

  capture
    .client
    .replace_pet_photo(request)
    .await
    .expect("the upload completes");

  let request = capture.only_request();
  assert_eq!(
    (request.method.clone(), request.path.as_str()),
    (Method::PUT, format!("/pets/{PET_ID}/photo").as_str())
  );
  assert_eq!(request.body, PNG_SIGNATURE);
  assert_eq!(request.content_type().map(essence).as_deref(), Some("image/png"));
}

#[rstest]
#[tokio::test]
async fn notes_are_sent_as_text_plain(#[future(awt)] capture: CaptureServer) {
  let request = client::SetPetNotesRequest::builder()
    .pet_id(PET_ID)
    .body("Loves naps".to_string())
    .build()
    .expect("valid notes");

  capture.client.set_pet_notes(request).await.expect("the notes send");

  let request = capture.only_request();
  assert_eq!(
    (request.method.clone(), request.path.as_str()),
    (Method::PUT, format!("/pets/{PET_ID}/notes").as_str())
  );
  assert_eq!(request.body, b"Loves naps");
  assert_eq!(request.content_type().map(essence).as_deref(), Some("text/plain"));
}

#[rstest]
#[case::plain("Loves naps")]
#[case::multiline_non_ascii("Eats twice a day.\r\nAllergic to crème fraîche.\n名前は Biscuit")]
#[case::empty("")]
#[tokio::test]
async fn notes_show_up_on_the_kennel_card(#[future(awt)] petstore: Petstore, #[case] notes: &str) {
  let pet_id = seed_pet(&petstore);
  let request = client::SetPetNotesRequest::builder()
    .pet_id(pet_id)
    .body(notes.to_string())
    .build()
    .expect("valid notes");

  let saved = petstore.client.set_pet_notes(request).await.expect("the notes save");

  assert!(matches!(saved, ApiResponse::NoContent(())), "{}", outcome(&saved));
  let Some(Received::SetPetNotes(received)) = petstore.service.last_received() else {
    panic!("the service did not receive the notes");
  };
  assert_eq!(received.body, notes);
  let card = petstore
    .client
    .get_pet_card(
      client::GetPetCardRequest::builder()
        .pet_id(pet_id)
        .build()
        .expect("a valid id"),
    )
    .await
    .expect("the card renders");
  let ApiResponse::Ok(card) = card else {
    panic!("expected 200 OK, got {}", outcome(&card));
  };
  assert_eq!(card, format!("Biscuit (dog)\nStatus: available\nNotes: {notes}"));
}

#[rstest]
#[case::at_the_limit("a".repeat(2000), true)]
#[case::past_the_limit("a".repeat(2001), false)]
#[case::multibyte_at_the_limit("é".repeat(2000), true)]
#[case::multibyte_past_the_limit("é".repeat(2001), false)]
fn notes_over_2000_characters_are_rejected_before_sending(#[case] notes: String, #[case] accepted: bool) {
  let length = notes.chars().count();
  let built = client::SetPetNotesRequest::builder().pet_id(PET_ID).body(notes).build();
  assert_eq!(
    built.is_ok(),
    accepted,
    "{length} characters built {:?}",
    built.map(|_| ())
  );
}

/// A media operation scoped to one pet.
#[derive(Debug, Clone, Copy)]
enum PetMediaOperation {
  UploadImage,
  ReplacePhoto,
  GetPhoto,
  SetNotes,
  GetCard,
}

impl PetMediaOperation {
  /// Runs the operation against `pet_id` and returns the `404` problem it must answer with.
  async fn problem_for(self, client: &PetstoreClient, pet_id: Uuid) -> client::Problem {
    match self {
      Self::UploadImage => {
        let body = client::ImageRequestBody {
          name: "Ghost".to_string(),
          image: PNG_SIGNATURE.to_vec(),
        };
        let request = client::UploadPetImageRequest::builder()
          .pet_id(pet_id)
          .body(body)
          .build();
        not_found(
          client
            .upload_pet_image(request.expect("a valid upload"))
            .await
            .expect("a response"),
        )
      }
      Self::ReplacePhoto => {
        let request = client::ReplacePetPhotoRequest::builder()
          .pet_id(pet_id)
          .body(PNG_SIGNATURE.to_vec())
          .build();
        not_found(
          client
            .replace_pet_photo(request.expect("a valid photo"))
            .await
            .expect("a response"),
        )
      }
      Self::GetPhoto => {
        let request = client::GetPetPhotoRequest::builder().pet_id(pet_id).build();
        not_found(
          client
            .get_pet_photo(request.expect("a valid id"))
            .await
            .expect("a response"),
        )
      }
      Self::SetNotes => {
        let request = client::SetPetNotesRequest::builder()
          .pet_id(pet_id)
          .body("Boo".to_string())
          .build();
        not_found(
          client
            .set_pet_notes(request.expect("valid notes"))
            .await
            .expect("a response"),
        )
      }
      Self::GetCard => {
        let request = client::GetPetCardRequest::builder().pet_id(pet_id).build();
        not_found(
          client
            .get_pet_card(request.expect("a valid id"))
            .await
            .expect("a response"),
        )
      }
    }
  }
}

#[rstest]
#[case::upload_image(PetMediaOperation::UploadImage)]
#[case::replace_photo(PetMediaOperation::ReplacePhoto)]
#[case::get_photo(PetMediaOperation::GetPhoto)]
#[case::set_notes(PetMediaOperation::SetNotes)]
#[case::get_card(PetMediaOperation::GetCard)]
#[tokio::test]
async fn unknown_pet_answers_with_a_problem(#[future(awt)] petstore: Petstore, #[case] operation: PetMediaOperation) {
  let unknown = Uuid::from_u128(0xDEAD);

  let problem = operation.problem_for(&petstore.client, unknown).await;

  assert_eq!(problem.status, Some(404));
  assert_eq!(problem.detail, Some(format!("no pet has id {unknown}")));
  assert_eq!(problem.instance, Some(format!("/pets/{unknown}")));
}

#[rstest]
#[case::consent_omitted(client::Frequency::Weekly, None, None)]
#[case::consent_given(client::Frequency::Monthly, Some(true), Some(client::Species::Cat))]
#[case::consent_declined(client::Frequency::Weekly, Some(false), Some(client::Species::SmallMammal))]
#[tokio::test]
async fn newsletter_signup_creates_a_subscription(
  #[future(awt)] petstore: Petstore,
  #[case] frequency: client::Frequency,
  #[case] consent: Option<bool>,
  #[case] favorite_species: Option<client::Species>,
) {
  let signup = client::NewsletterSignup {
    email: "ada@example.com".to_string(),
    frequency,
    consent,
    favorite_species,
  };
  let expected_on_server = mirrored::<server::NewsletterSignup>(&client::NewsletterSignup {
    consent: Some(consent.unwrap_or(false)),
    ..signup.clone()
  });
  let request = client::SubscribeNewsletterRequest::builder()
    .body(signup.clone())
    .build()
    .expect("a valid signup");

  let response = petstore
    .anonymous()
    .subscribe_newsletter(request)
    .await
    .expect("the signup completes");

  let ApiResponse::Created(subscription) = response else {
    panic!("expected 201 Created, got {}", outcome(&response));
  };
  let requested_frequency: &client::Frequency = &signup.frequency;
  let subscribed_frequency: &client::Frequency = &subscription.frequency;
  assert_eq!(subscribed_frequency, requested_frequency);
  assert_eq!(subscription.email, signup.email);
  assert_eq!(subscription.consent, Some(consent.unwrap_or(false)));
  assert_eq!(subscription.subscribed_at, Some(now()));
  let Some(Received::SubscribeNewsletter(received)) = petstore.service.last_received() else {
    panic!("the service did not receive the signup");
  };
  assert_eq!(received.body, expected_on_server);
}

#[rstest]
#[case::every_field(
  client::NewsletterSignup {
    email: "ada@example.com".to_string(),
    frequency: client::Frequency::Monthly,
    consent: Some(true),
    favorite_species: Some(client::Species::SmallMammal),
  },
  "email=ada%40example.com&frequency=monthly&consent=true&favoriteSpecies=small_mammal",
)]
#[case::required_only(
  client::NewsletterSignup {
    email: "grace+pets@example.com".to_string(),
    frequency: client::Frequency::Weekly,
    consent: None,
    favorite_species: None,
  },
  "email=grace%2Bpets%40example.com&frequency=weekly",
)]
#[tokio::test]
async fn newsletter_signup_is_sent_url_encoded_without_credentials(
  #[future(awt)] capture: CaptureServer,
  #[case] signup: client::NewsletterSignup,
  #[case] expected_body: &str,
) {
  let request = client::SubscribeNewsletterRequest::builder()
    .body(signup)
    .build()
    .expect("a valid signup");

  capture
    .client
    .subscribe_newsletter(request)
    .await
    .expect("the signup completes");

  let request = capture.only_request();
  assert_eq!(request.path, "/store/newsletter");
  assert_eq!(request.content_type(), Some("application/x-www-form-urlencoded"));
  assert_eq!(String::from_utf8_lossy(&request.body), expected_body);
  assert!(
    !request.headers.contains_key(X_API_KEY),
    "`security: []` sends no API key"
  );
}

/// An optional `application/x-www-form-urlencoded` body is either absent, for a one-click
/// unsubscribe link, or the form the client built.
#[rstest]
#[case::one_click(None)]
#[case::address_only(Some(client::NewsletterOptOut {
  email: "grace@example.com".to_string(),
  reason: None,
}))]
#[case::with_a_reason(Some(client::NewsletterOptOut {
  email: "grace+pets@example.com".to_string(),
  reason: Some("Too many cat pictures & not enough dogs".to_string()),
}))]
#[tokio::test]
async fn unsubscribing_sends_an_optional_form(
  #[future(awt)] petstore: Petstore,
  #[case] opt_out: Option<client::NewsletterOptOut>,
) {
  let request = client::UnsubscribeNewsletterRequest::builder()
    .maybe_body(opt_out.clone())
    .build()
    .expect("a valid unsubscribe request");

  let response = petstore
    .anonymous()
    .unsubscribe_newsletter(request)
    .await
    .expect("unsubscribeNewsletter round trip");

  assert!(matches!(response, ApiResponse::NoContent(())), "{response:?}");
  let Some(Received::UnsubscribeNewsletter(received)) = petstore.service.last_received() else {
    panic!("the server did not record unsubscribeNewsletter");
  };
  let expected = opt_out.as_ref().map(mirrored::<server::NewsletterOptOut>);
  assert_eq!(received.body, expected);
}

/// A body the optional form can't decode is rejected before the service runs, while a request
/// with no body at all is the one-click case.
#[rstest]
#[case::json(
  Some("application/json"),
  r#"{"email":"grace@example.com"}"#,
  StatusCode::BAD_REQUEST
)]
#[case::missing_required_email(Some("application/x-www-form-urlencoded"), "reason=moving", StatusCode::BAD_REQUEST)]
#[case::no_body(None, "", StatusCode::NO_CONTENT)]
#[tokio::test]
async fn unsubscribing_rejects_bodies_that_are_not_the_form(
  #[future(awt)] petstore: Petstore,
  #[case] content_type: Option<&str>,
  #[case] body: &'static str,
  #[case] expected: StatusCode,
) {
  let mut request = reqwest::Client::new()
    .post(petstore.url("/store/newsletter/unsubscribe"))
    .body(body);
  if let Some(content_type) = content_type {
    request = request.header(header::CONTENT_TYPE, content_type);
  }

  let response = request.send().await.expect("the request completes");

  assert_eq!(response.status(), expected, "{content_type:?} {body:?}");
  let reached_service = matches!(
    petstore.service.last_received(),
    Some(Received::UnsubscribeNewsletter(_))
  );
  assert_eq!(
    reached_service,
    expected.is_success(),
    "only accepted requests reach the service"
  );
}

/// The case count every property runs, with failure persistence off so no regression file lands beside the tests.
fn property_config() -> ProptestConfig {
  ProptestConfig {
    failure_persistence: None,
    ..ProptestConfig::with_cases(48)
  }
}

/// Text of `length` characters drawn from every Unicode scalar value.
fn unicode_text(length: impl Into<SizeRange>) -> impl Strategy<Value = String> {
  collection::vec(any::<char>(), length).prop_map(String::from_iter)
}

/// A non-empty `multipart/form-data` part name drawn from every Unicode scalar value except `"`, `\`,
/// and control characters other than tab.
///
/// RFC 7578 quotes the name in `Content-Disposition` and defines no escape the generated server
/// reverses (it only unescapes `\"`), so those characters cannot cross the wire in a part name. This
/// is a documented limitation of map-shaped multipart bodies; spaces, `;`, `=`, tabs, and non-ASCII
/// text all stay in.
fn part_name() -> impl Strategy<Value = String> {
  let name_character = any::<char>().prop_filter("not expressible in a part name", |character| {
    !matches!(character, '"' | '\\') && (*character == '\t' || !character.is_control())
  });
  collection::vec(name_character, 1..12).prop_map(String::from_iter)
}

prop_compose! {
  /// A label map whose keys are any [`part_name`] and whose values are arbitrary non-empty Unicode text.
  fn media_labels()(entries in collection::vec((part_name(), unicode_text(1..24)), 0..6))
    -> IndexMap<String, String> {
    entries.into_iter().collect()
  }
}

prop_compose! {
  /// A tally map whose keys are any [`part_name`].
  ///
  /// Every list holds at least one count: `multipart/form-data` repeats a part per item, so an
  /// empty list has no parts and cannot be told apart from an absent key.
  fn tallies()(entries in collection::vec((part_name(), collection::vec(any::<i64>(), 1..5)), 0..6))
    -> IndexMap<String, Vec<i64>> {
    entries.into_iter().collect()
  }
}

prop_compose! {
  /// An intake form with arbitrary text and scalar fields and empty file parts.
  ///
  /// Present arrays are never empty, for the same reason as [`tallies`].
  fn intake_scalars()(
    name in unicode_text(1..24),
    note in option::of(unicode_text(0..48)),
    count in any::<i64>(),
    enabled in option::of(any::<bool>()),
    tags in option::of(collection::vec(unicode_text(0..12), 1..4)),
    ids in option::of(collection::vec(any::<i64>(), 1..6)),
  ) -> client::IntakeForm {
    client::IntakeForm { name, note, count, enabled, tags, ids, ..client::IntakeForm::default() }
  }
}

#[test]
fn any_photo_round_trips_byte_for_byte() {
  let petstore = start_shared();
  let pet_id = seed_pet(&petstore);
  proptest!(property_config(), |(photo in collection::vec(any::<u8>(), 0..4096))| {
    run_case(async || {
      let replace = client::ReplacePetPhotoRequest::builder().pet_id(pet_id).body(photo.clone()).build();
      let replaced = petstore.client.replace_pet_photo(replace.expect("a valid photo")).await.expect("a response");
      prop_assert!(matches!(replaced, ApiResponse::NoContent(())), "{}", outcome(&replaced));
      let download = client::GetPetPhotoRequest::builder().pet_id(pet_id).build();
      let downloaded = petstore.client.get_pet_photo(download.expect("a valid id")).await.expect("a response");
      let ApiResponse::Ok(bytes) = downloaded else {
        return Err(TestCaseError::fail(format!("expected 200 OK, got {}", outcome(&downloaded))));
      };
      prop_assert_eq!(bytes, photo);
      Ok::<_, TestCaseError>(())
    })?;
  });
}

#[test]
fn any_notes_appear_on_the_kennel_card_exactly() {
  let petstore = start_shared();
  let pet_id = seed_pet(&petstore);
  proptest!(property_config(), |(notes in unicode_text(0..=2000))| {
    run_case(async || {
      let save = client::SetPetNotesRequest::builder().pet_id(pet_id).body(notes.clone()).build();
      let saved = petstore.client.set_pet_notes(save.expect("valid notes")).await.expect("a response");
      prop_assert!(matches!(saved, ApiResponse::NoContent(())), "{}", outcome(&saved));
      let render = client::GetPetCardRequest::builder().pet_id(pet_id).build();
      let card = petstore.client.get_pet_card(render.expect("a valid id")).await.expect("a response");
      let ApiResponse::Ok(card) = card else {
        return Err(TestCaseError::fail(format!("expected 200 OK, got {}", outcome(&card))));
      };
      prop_assert_eq!(card, format!("Biscuit (dog)\nStatus: available\nNotes: {notes}"));
      Ok::<_, TestCaseError>(())
    })?;
  });
}

#[test]
fn any_media_labels_round_trip_unchanged() {
  let petstore = start_shared();
  proptest!(property_config(), |(labels in media_labels())| {
    run_case(async || {
      let body = client::MediaLabels { additional_properties: labels.clone() };
      let request = client::SubmitMediaLabelsRequest::builder().body(body).build().expect("valid labels");
      let response = petstore.client.submit_media_labels(request).await.expect("a response");
      let ApiResponse::Ok(echo) = response else {
        return Err(TestCaseError::fail(format!("expected 200 OK, got {}", outcome(&response))));
      };
      prop_assert_eq!(echo.additional_properties, labels);
      Ok::<_, TestCaseError>(())
    })?;
  });
}

#[test]
fn any_tallies_round_trip_unchanged() {
  let petstore = start_shared();
  proptest!(property_config(), |(tallies in tallies())| {
    run_case(async || {
      let body = client::Tallies { additional_properties: tallies.clone() };
      let request = client::SubmitTalliesRequest::builder().body(body).build().expect("valid tallies");
      let response = petstore.client.submit_tallies(request).await.expect("a response");
      let ApiResponse::Ok(echo) = response else {
        return Err(TestCaseError::fail(format!("expected 200 OK, got {}", outcome(&response))));
      };
      prop_assert_eq!(echo.additional_properties, tallies);
      Ok::<_, TestCaseError>(())
    })?;
  });
}

#[test]
fn any_intake_text_and_scalars_round_trip_unchanged() {
  let petstore = start_shared();
  proptest!(property_config(), |(form in intake_scalars())| {
    run_case(async || {
      let response = submit_intake_form(&petstore.client, form.clone()).await;
      let ApiResponse::Ok(echo) = response else {
        return Err(TestCaseError::fail(format!("expected 200 OK, got {}", outcome(&response))));
      };
      prop_assert_eq!(echo, form);
      Ok::<_, TestCaseError>(())
    })?;
  });
}

use std::{
  future::{Future, ready},
  sync::{Arc, Mutex},
};

use axum::{extract::Multipart, routing::post};
use chrono::TimeZone as _;

use crate::{
  fixtures::{multipart as client, multipart_server as server},
  tests::common::serve,
};

const LEXICON: &[u8] = b"<?xml version=\"1.0\"?><lexicon/>";
const PNG: &[u8] = &[137, 80, 78, 71];

#[derive(Clone)]
struct Echo;

impl server::ApiServer for Echo {
  fn submit_form(
    &self,
    request: server::SubmitFormRequest,
  ) -> impl Future<Output = anyhow::Result<server::ApiResponse<server::UploadForm>>> + Send {
    ready(Ok(server::ApiResponse::Ok(request.body)))
  }

  fn upload_files(
    &self,
    request: server::UploadFilesRequest,
  ) -> impl Future<Output = anyhow::Result<server::ApiResponse<server::FilesReceipt>>> + Send {
    let length = |bytes: &Vec<u8>| i64::try_from(bytes.len()).unwrap();
    let receipt = request.body.map(|body| server::FilesReceipt {
      sizes: body.file.iter().flatten().map(length).collect(),
      cover_size: body.cover.as_ref().map(length),
      note: body.note,
    });
    ready(Ok(server::ApiResponse::Ok(receipt.unwrap_or_default())))
  }

  fn submit_metadata(
    &self,
    request: server::SubmitMetadataRequest,
  ) -> impl Future<Output = anyhow::Result<server::ApiResponse<server::Metadata>>> + Send {
    ready(Ok(server::ApiResponse::Ok(request.body)))
  }

  fn submit_counters(
    &self,
    request: server::SubmitCountersRequest,
  ) -> impl Future<Output = anyhow::Result<server::ApiResponse<server::Counters>>> + Send {
    ready(Ok(server::ApiResponse::Ok(request.body)))
  }
}

fn full_form() -> client::UploadForm {
  client::UploadForm {
    name: "Lexicon".to_string(),
    file_name: Some("lexicon.pls".to_string()),
    r#type: Some("pls".to_string()),
    resume: Some("naïve".to_string()),
    file: Some(LEXICON.to_vec()),
    attachments: vec![b"first".to_vec(), b"second".to_vec()],
    thumbnail: PNG.to_vec(),
    encoded: b"hello".to_vec(),
    count: 3,
    ratio: Some(0.5),
    enabled: Some(true),
    quality: Some(client::Quality::High),
    level: Some(client::Level::Value2),
    tags: Some(vec!["a".to_string(), "42".to_string()]),
    created_at: Some(chrono::Utc.with_ymd_and_hms(2026, 10, 7, 16, 42, 52).unwrap()),
    address: Some(client::Address {
      city: "Portland".to_string(),
      zip: None,
    }),
    addresses: Some(vec![
      client::Address {
        city: "Salem".to_string(),
        zip: Some("97301".to_string()),
      },
      client::Address {
        city: "Bend".to_string(),
        zip: None,
      },
    ]),
    note: Some("# heading".to_string()),
    labels: Some(client::UploadFormLabels::Object(indexmap::IndexMap::from([(
      "accent".to_string(),
      "british".to_string(),
    )]))),
    ids: Some(vec![1, 2, 3]),
    colors: Some(vec!["red".to_string(), "blue".to_string()]),
    filter: Some(client::Filter {
      size: Some(10),
      color: Some("42".to_string()),
    }),
    range: Some(client::Range {
      min: Some(1),
      max: Some(5),
    }),
    extras: Some(client::Extras {
      mood: Some("12".to_string()),
      score: Some(7),
    }),
    options: Some(indexmap::IndexMap::from([("a".to_string(), 1), ("b".to_string(), 2)])),
  }
}

fn submit_form_request(body: client::UploadForm) -> client::SubmitFormRequest {
  client::SubmitFormRequest::builder()
    .form_id("f1".to_string())
    .body(body)
    .build()
    .unwrap()
}

#[tokio::test]
async fn generated_client_and_server_round_trip_every_part_kind() {
  let base_url = serve(server::router(Echo)).await;
  let api = client::MultipartFormsClient::with_base_url(&base_url).unwrap();

  let cases = [
    full_form(),
    client::UploadForm {
      name: "null".to_string(),
      resume: Some("{\"a\":1}".to_string()),
      tags: Some(vec!["true".to_string()]),
      labels: Some(client::UploadFormLabels::String("1".to_string())),
      ..Default::default()
    },
  ];
  for sent in cases {
    match api.submit_form(submit_form_request(sent.clone())).await.unwrap() {
      client::ApiResponse::Ok(received) => assert_eq!(received, sent, "server should receive the form as sent"),
      client::ApiResponse::Other(status, body) => {
        panic!("expected 200, got {status}: {}", String::from_utf8_lossy(&body))
      }
    }
  }
}

#[tokio::test]
async fn generated_client_and_server_round_trip_files_and_maps() {
  let base_url = serve(server::router(Echo)).await;
  let api = client::MultipartFormsClient::with_base_url(&base_url).unwrap();

  let cases = [
    (
      Some(client::FileRequestBody {
        file: Some(vec![b"one".to_vec(), b"three".to_vec()]),
        cover: Some(PNG.to_vec()),
        note: Some("two files".to_string()),
      }),
      client::FilesReceipt {
        sizes: vec![3, 5],
        cover_size: Some(4),
        note: Some("two files".to_string()),
      },
    ),
    (
      Some(client::FileRequestBody {
        file: Some(vec![]),
        ..Default::default()
      }),
      client::FilesReceipt::default(),
    ),
    (None, client::FilesReceipt::default()),
  ];
  for (body, expected) in cases {
    let label = format!("{body:?}");
    let request = client::UploadFilesRequest::builder().maybe_body(body).build().unwrap();
    match api.upload_files(request).await.unwrap() {
      client::ApiResponse::Ok(receipt) => assert_eq!(receipt, expected, "files for {label}"),
      client::ApiResponse::Other(status, body) => {
        panic!(
          "{label}: expected 200, got {status}: {}",
          String::from_utf8_lossy(&body)
        )
      }
    }
  }

  let metadata = client::Metadata {
    additional_properties: indexmap::IndexMap::from([
      ("voice".to_string(), "Rachel".to_string()),
      ("speed".to_string(), "1.5".to_string()),
      ("first name".to_string(), "Ada".to_string()),
      ("café".to_string(), "true".to_string()),
    ]),
  };
  let request = client::SubmitMetadataRequest::builder()
    .body(metadata.clone())
    .build()
    .unwrap();
  match api.submit_metadata(request).await.unwrap() {
    client::ApiResponse::Ok(received) => assert_eq!(received, metadata, "flattened entries should round trip"),
    client::ApiResponse::Other(status, _) => panic!("expected 200, got {status}"),
  }

  let counter_cases = [
    indexmap::IndexMap::from([("a".to_string(), vec![1, 2]), ("b".to_string(), vec![3])]),
    indexmap::IndexMap::new(),
  ];
  for counts in counter_cases {
    let sent = client::Counters {
      additional_properties: counts,
    };
    let request = client::SubmitCountersRequest::builder()
      .body(sent.clone())
      .build()
      .unwrap();
    match api.submit_counters(request).await.unwrap() {
      client::ApiResponse::Ok(received) => assert_eq!(received, sent, "repeated entries should collect into arrays"),
      client::ApiResponse::Other(status, body) => {
        panic!("expected 200, got {status}: {}", String::from_utf8_lossy(&body))
      }
    }
  }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CapturedPart {
  name: Option<String>,
  file_name: Option<String>,
  content_type: Option<String>,
  content: Vec<u8>,
}

impl CapturedPart {
  fn new(name: &str, file_name: Option<&str>, content_type: Option<&str>, content: &[u8]) -> Self {
    Self {
      name: Some(name.to_string()),
      file_name: file_name.map(str::to_string),
      content_type: content_type.map(str::to_string),
      content: content.to_vec(),
    }
  }
}

#[tokio::test]
async fn generated_client_sends_spec_conformant_parts() {
  let captured = Arc::new(Mutex::new(vec![]));
  let sink = captured.clone();
  let router = axum::Router::new().route(
    "/forms/{form_id}",
    post(move |mut multipart: Multipart| async move {
      while let Some(part) = multipart.next_field().await.unwrap() {
        let name = part.name().map(str::to_string);
        let file_name = part.file_name().map(str::to_string);
        let content_type = part.content_type().map(str::to_string);
        let content = part.bytes().await.unwrap().to_vec();
        sink.lock().unwrap().push(CapturedPart {
          name,
          file_name,
          content_type,
          content,
        });
      }
      axum::http::StatusCode::NO_CONTENT
    }),
  );
  let base_url = serve(router).await;
  let api = client::MultipartFormsClient::with_base_url(&base_url).unwrap();
  let _ = api.submit_form(submit_form_request(full_form())).await.unwrap();

  let parts = captured.lock().unwrap().clone();
  let json = Some("application/json");
  let octet_stream = Some("application/octet-stream");
  let expected = [
    CapturedPart::new("name", None, None, b"Lexicon"),
    CapturedPart::new("fileName", None, None, b"lexicon.pls"),
    CapturedPart::new("type", None, None, b"pls"),
    CapturedPart::new("résumé", None, None, "naïve".as_bytes()),
    CapturedPart::new("file", Some("file"), octet_stream, LEXICON),
    CapturedPart::new("attachments", Some("attachments"), octet_stream, b"first"),
    CapturedPart::new("attachments", Some("attachments"), octet_stream, b"second"),
    CapturedPart::new("thumbnail", Some("thumbnail"), octet_stream, PNG),
    CapturedPart::new("encoded", None, None, b"aGVsbG8="),
    CapturedPart::new("count", None, None, b"3"),
    CapturedPart::new("ratio", None, None, b"0.5"),
    CapturedPart::new("enabled", None, None, b"true"),
    CapturedPart::new("quality", None, None, b"high"),
    CapturedPart::new("level", None, None, b"2"),
    CapturedPart::new("tags", None, None, b"a"),
    CapturedPart::new("tags", None, None, b"42"),
    CapturedPart::new("createdAt", None, None, b"2026-10-07T16:42:52Z"),
    CapturedPart::new("address", None, json, br#"{"city":"Portland"}"#),
    CapturedPart::new(
      "addresses",
      None,
      Some("application/vnd.example+json"),
      br#"{"city":"Salem","zip":"97301"}"#,
    ),
    CapturedPart::new(
      "addresses",
      None,
      Some("application/vnd.example+json"),
      br#"{"city":"Bend"}"#,
    ),
    CapturedPart::new("note", None, Some("text/markdown"), b"# heading"),
    CapturedPart::new("labels", None, json, br#"{"accent":"british"}"#),
    CapturedPart::new("ids", None, None, b"1|2|3"),
    CapturedPart::new("colors", None, None, b"red"),
    CapturedPart::new("colors", None, None, b"blue"),
    CapturedPart::new("filter[size]", None, None, b"10"),
    CapturedPart::new("filter[color]", None, None, b"42"),
    CapturedPart::new("range", None, None, b"min,1,max,5"),
    CapturedPart::new("mood", None, None, b"12"),
    CapturedPart::new("score", None, None, b"7"),
    CapturedPart::new("options[a]", None, None, b"1"),
    CapturedPart::new("options[b]", None, None, b"2"),
  ];
  assert_eq!(parts, expected, "parts on the wire");
}

#[tokio::test]
async fn generated_server_rejects_malformed_forms() {
  let base_url = serve(server::router(Echo)).await;
  let url = format!("{base_url}/forms/f1");
  let thumbnail = || reqwest::multipart::Part::bytes(PNG.to_vec()).file_name("thumbnail");
  let cases = [
    (
      "missing required file part",
      reqwest::multipart::Form::new()
        .text("name", "n")
        .text("count", "1")
        .text("encoded", "aGk="),
      "missing multipart part `thumbnail`",
    ),
    (
      "scalar that is not a number",
      reqwest::multipart::Form::new()
        .text("name", "n")
        .text("count", "three")
        .text("encoded", "aGk=")
        .part("thumbnail", thumbnail()),
      "invalid multipart part `count`",
    ),
    (
      "missing required text part",
      reqwest::multipart::Form::new()
        .text("count", "1")
        .text("encoded", "aGk=")
        .part("thumbnail", thumbnail()),
      "missing field `name`",
    ),
  ];
  for (case, form, message) in cases {
    let response = reqwest::Client::new().post(&url).multipart(form).send().await.unwrap();
    let status = response.status();
    let text = response.text().await.unwrap();
    assert_eq!(status, reqwest::StatusCode::BAD_REQUEST, "{case}: {text}");
    assert!(text.contains(message), "{case}: unexpected message {text}");
  }

  let json = reqwest::Client::new()
    .post(&url)
    .json(&serde_json::json!({"name": "n"}))
    .send()
    .await
    .unwrap();
  assert_eq!(
    json.status(),
    reqwest::StatusCode::BAD_REQUEST,
    "a JSON body is not a multipart form"
  );
}

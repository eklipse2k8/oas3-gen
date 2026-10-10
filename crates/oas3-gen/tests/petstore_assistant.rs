//! The `assistant` tag, end to end over HTTP.
//!
//! Requests are built with the generated client, extracted by the generated router,
//! answered by [`support::PetstoreService`], and parsed by the generated client. Each
//! test asserts both directions: what the server extracted from the wire (`Received`)
//! and what the client got back. Together they prove that discriminated unions,
//! untagged unions, non-exhaustive enums, constants, spec defaults, and enums whose
//! values are numbers or booleans survive a real round trip.

mod support;

use indexmap::IndexMap;
use petstore_client as client;
use petstore_server as server;
use proptest::{collection, option, prelude::*, sample::select};
use reqwest::{Method, StatusCode};
use rstest::rstest;
use serde::Serialize;
use serde_json::{Value, json};
use support::{API_KEY, Petstore, Received, petstore, run_case, start_shared};

fn to_json<T: Serialize>(value: &T) -> Value {
  serde_json::to_value(value).expect("the value serializes to JSON")
}

/// The members of a JSON object literal, in order.
fn object(value: Value) -> IndexMap<String, Value> {
  let Value::Object(members) = value else {
    panic!("{value} is not a JSON object");
  };
  members.into_iter().collect()
}

fn text(text: &str) -> client::TextBlock {
  client::TextBlock {
    text: text.to_string(),
    ..Default::default()
  }
}

/// Sends `body` to `path` with the test API key, bypassing the generated client.
///
/// A reply that is not JSON comes back as a JSON string holding its text.
/// Sends a request the generated client can't express, with the API key and the
/// `conversation` cookie `listAssistantEvents` requires.
async fn send_raw(petstore: &Petstore, method: Method, path: &str, body: Option<&Value>) -> (StatusCode, Value) {
  let mut request = reqwest::Client::new()
    .request(method, petstore.url(path))
    .header(client::X_API_KEY, API_KEY)
    .header(reqwest::header::COOKIE, "conversation=conversation-1");
  if let Some(body) = body {
    request = request.json(body);
  }
  let response = request.send().await.expect("the raw request completes");
  let status = response.status();
  let text = response.text().await.expect("the reply has a body");
  let reply = serde_json::from_str(&text).unwrap_or(Value::String(text));
  (status, reply)
}

async fn send_message(
  petstore: &Petstore,
  body: client::ContentRequest,
) -> client::ApiResponse<client::ContentResponse, client::ErrorResponse> {
  let request = client::SendAssistantMessageRequest::builder()
    .body(body)
    .build()
    .expect("the request is valid");
  petstore
    .client
    .send_assistant_message(request)
    .await
    .expect("the request completes")
}

async fn send_blocks(petstore: &Petstore, blocks: Vec<client::ContentBlock>) -> client::ContentResponse {
  let response = send_message(
    petstore,
    client::ContentRequest {
      blocks,
      ..Default::default()
    },
  )
  .await;
  let client::ApiResponse::Ok(reply) = response else {
    panic!("expected 200 OK, got {response:?}");
  };
  reply
}

/// The body of the most recent `sendAssistantMessage` the server extracted.
fn received_message(petstore: &Petstore) -> server::ContentRequest {
  let Some(Received::SendAssistantMessage(request)) = petstore.service.last_received() else {
    panic!("the server did not record a sendAssistantMessage request");
  };
  request.body
}

async fn put_settings(
  client: &client::PetstoreClient,
  settings: client::AssistantSettings,
) -> client::AssistantSettings {
  let request = client::UpdateAssistantSettingsRequest::builder()
    .body(settings)
    .build()
    .expect("the request is valid");
  let response = client
    .update_assistant_settings(request)
    .await
    .expect("the request completes");
  let client::ApiResponse::Ok(echoed) = response else {
    panic!("expected 200 OK, got {response:?}");
  };
  echoed
}

/// The settings document after the spec defaults fill every absent member that has one.
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

fn received_settings(petstore: &Petstore) -> server::AssistantSettings {
  let Some(Received::UpdateAssistantSettings(request)) = petstore.service.last_received() else {
    panic!("the server did not record an updateAssistantSettings request");
  };
  request.body
}

async fn put_preference(petstore: &Petstore, preference: client::ChatPreference) -> client::ChatPreference {
  let request = client::UpdateChatPreferenceRequest::builder()
    .body(preference)
    .build()
    .expect("the request is valid");
  let response = petstore
    .client
    .update_chat_preference(request)
    .await
    .expect("the request completes");
  let client::ApiResponse::Ok(stored) = response else {
    panic!("expected 200 OK, got {response:?}");
  };
  stored
}

async fn get_preference(petstore: &Petstore) -> client::ChatPreference {
  let response = petstore
    .client
    .get_chat_preference(client::GetChatPreferenceRequest {})
    .await
    .expect("the request completes");
  let client::ApiResponse::Ok(preference) = response else {
    panic!("expected 200 OK, got {response:?}");
  };
  preference
}

fn received_preference(petstore: &Petstore) -> server::ChatPreference {
  let Some(Received::UpdateChatPreference(request)) = petstore.service.last_received() else {
    panic!("the server did not record an updateChatPreference request");
  };
  request.body
}

/// A text block carrying both annotation kinds and recipes with members the schema does not name.
fn annotated_text() -> client::ContentBlock {
  client::ContentBlock::Text(client::TextBlock {
    text: "Biscuit needs a rabies booster.".to_string(),
    annotations: Some(vec![
      client::Annotation::Citation(client::CitationAnnotation {
        start: 0,
        end: 7,
        build: "vet-guide-2026".to_string(),
        ..Default::default()
      }),
      client::Annotation::Link(client::LinkAnnotation {
        start: 16,
        end: 30,
        url: "https://vet.example/rabies".to_string(),
        ..Default::default()
      }),
    ]),
    recipes: Some(vec![
      client::RecipeList {
        ingredients: Some(object(json!({ "chicken": "200g", "rice": { "cups": 1 } }))),
        required: Some(vec!["chicken".to_string()]),
        additional_properties: object(json!({ "servings": 2, "vetApproved": true })),
        ..Default::default()
      },
      client::RecipeList {
        ingredients: None,
        required: None,
        ..Default::default()
      },
    ]),
    ..Default::default()
  })
}

fn base64_image(media_type: client::MediaType, data: &[u8]) -> client::ContentBlock {
  client::ContentBlock::Image(client::ImageBlock {
    source: Box::new(client::ImageSource::Base64(client::Base64ImageSource {
      media_type,
      data: data.to_vec(),
      ..Default::default()
    })),
    alt_text: Some("A sleepy beagle".to_string()),
    ..Default::default()
  })
}

fn tool_result(content: client::ToolResultContent, is_error: Option<bool>) -> client::ContentBlock {
  client::ContentBlock::ToolResult(client::ToolResultBlock {
    tool_use_id: "toolu_01".to_string(),
    content,
    is_error,
    ..Default::default()
  })
}

#[rstest]
#[case::plain_text(client::ContentBlock::text("Biscuit is due for a booster.".to_string()))]
#[case::text_with_annotations_and_recipes(annotated_text())]
#[case::base64_image(base64_image(client::MediaType::ImagePng, &[0x89, b'P', b'N', b'G', 0, 255]))]
#[case::url_image(client::ContentBlock::image(Box::new(client::ImageSource::url(
  "https://cdn.petstore.example/biscuit.webp".to_string()
))))]
#[case::code_with_language(client::ContentBlock::Code(client::CodeBlock {
  code: "fn bark() {}".to_string(),
  language: Some("rust".to_string()),
  ..Default::default()
}))]
#[case::code_without_language(client::ContentBlock::code("ls -la".to_string()))]
#[case::tool_use(client::ContentBlock::ToolUse(client::ToolUseBlock {
  id: "toolu_01".to_string(),
  name: "find_vet".to_string(),
  input: object(json!({ "city": "Portland", "radiusKm": 12.5, "open": true, "species": ["dog"], "filters": { "rating": 4 } })),
  ..Default::default()
}))]
#[case::tool_use_without_input(client::ContentBlock::ToolUse(client::ToolUseBlock {
  id: "toolu_02".to_string(),
  name: "ping".to_string(),
  input: IndexMap::new(),
  ..Default::default()
}))]
#[case::tool_result_text(tool_result(client::ToolResultContent::String("No vets open".to_string()), Some(true)))]
#[case::tool_result_blocks(tool_result(
  client::ToolResultContent::Array(vec![
    client::ToolResultContentBlock::Text(text("Two vets nearby")),
    client::ToolResultContentBlock::image(Box::new(client::ImageSource::url("https://maps.example/vets.png".to_string()))),
  ]),
  Some(false),
))]
#[case::tool_result_without_blocks(tool_result(client::ToolResultContent::Array(vec![]), None))]
#[tokio::test]
async fn every_content_block_kind_round_trips(#[future(awt)] petstore: Petstore, #[case] block: client::ContentBlock) {
  let reply = send_blocks(&petstore, vec![block.clone()]).await;

  assert_eq!(
    to_json(&received_message(&petstore).blocks),
    json!([to_json(&block)]),
    "the server extracted the block as sent"
  );
  assert_eq!(*reply.content, block, "the service echoes the block unchanged");
}

#[rstest]
#[case::jpeg(client::MediaType::ImageJpeg, "image/jpeg")]
#[case::png(client::MediaType::ImagePng, "image/png")]
#[case::gif(client::MediaType::ImageGif, "image/gif")]
#[case::webp(client::MediaType::ImageWebp, "image/webp")]
#[tokio::test]
async fn base64_image_sources_carry_base64_data_and_slash_media_types(
  #[future(awt)] petstore: Petstore,
  #[case] media_type: client::MediaType,
  #[case] wire_name: &str,
) {
  let block = base64_image(media_type, b"Biscuit");
  let body = to_json(&client::ContentRequest {
    blocks: vec![block.clone()],
    ..Default::default()
  });
  let expected_source = json!({ "type": "base64", "media_type": wire_name, "data": "QmlzY3VpdA==" });
  assert_eq!(body["blocks"][0]["source"], expected_source, "the client writes base64");

  let (status, reply) = send_raw(&petstore, Method::POST, "/assistant/messages", Some(&body)).await;
  assert_eq!(status, StatusCode::OK);
  assert_eq!(reply["content"]["source"], expected_source, "the server writes base64");

  let Some(server::ContentBlock::Image(image)) = received_message(&petstore).blocks.pop() else {
    panic!("the server did not extract an image block");
  };
  let server::ImageSource::Base64(source) = *image.source else {
    panic!("the server did not extract a base64 source");
  };
  assert_eq!(source.data, b"Biscuit", "the server decoded the base64 data");
  assert_eq!(source.media_type.to_string(), wire_name);

  let reply = send_blocks(&petstore, vec![block.clone()]).await;
  assert_eq!(*reply.content, block, "the client decoded the base64 data");
}

#[rstest]
#[case::counter_a(vec![client::UsageCounterUsageCounterKind::a(5)], json!([{ "output_tokens": 5, "type": "counter_a" }]))]
#[case::counter_b(vec![client::UsageCounterUsageCounterKind::b(7)], json!([{ "output_tokens": 7, "type": "counter_b" }]))]
#[case::both_kinds(
  vec![client::UsageCounterUsageCounterKind::a(1), client::UsageCounterUsageCounterKind::b(2)],
  json!([{ "output_tokens": 1, "type": "counter_a" }, { "output_tokens": 2, "type": "counter_b" }])
)]
#[case::no_counters(vec![], json!([]))]
#[tokio::test]
async fn tool_result_iterations_cross_the_wire_as_a_json_array(
  #[future(awt)] petstore: Petstore,
  #[case] iterations: client::UsageCounters,
  #[case] wire: Value,
) {
  let block = client::ContentBlock::ToolResult(client::ToolResultBlock {
    tool_use_id: "toolu_01".to_string(),
    content: client::ToolResultContent::String("done".to_string()),
    iterations: Some(iterations),
    is_error: Some(false),
    ..Default::default()
  });
  let body = to_json(&client::ContentRequest {
    blocks: vec![block.clone()],
    ..Default::default()
  });
  assert_eq!(body["blocks"][0]["iterations"], wire, "the client writes an array");

  let (status, reply) = send_raw(&petstore, Method::POST, "/assistant/messages", Some(&body)).await;
  assert_eq!(status, StatusCode::OK);
  assert_eq!(reply["content"]["iterations"], wire, "the server writes an array");

  let Some(server::ContentBlock::ToolResult(received)) = received_message(&petstore).blocks.pop() else {
    panic!("the server did not extract a tool result block");
  };
  let received_kinds = received
    .iterations
    .unwrap_or_default()
    .iter()
    .map(|counter| match counter {
      server::UsageCounterUsageCounterKind::A(_) => "counter_a",
      server::UsageCounterUsageCounterKind::B(_) => "counter_b",
    })
    .collect::<Vec<_>>();
  let sent_kinds = wire
    .as_array()
    .into_iter()
    .flatten()
    .map(|counter| counter["type"].as_str().unwrap_or_default())
    .collect::<Vec<_>>();
  assert_eq!(
    received_kinds, sent_kinds,
    "the server extracted each counter as its kind"
  );

  let reply = send_blocks(&petstore, vec![block.clone()]).await;
  assert_eq!(*reply.content, block, "the client parsed each counter as its kind");
}

#[rstest]
#[tokio::test]
async fn recipe_lists_accept_null_members_and_keep_unnamed_ones(#[future(awt)] petstore: Petstore) {
  let body = json!({
    "blocks": [{
      "type": "text",
      "text": "Dinner ideas",
      "recipes": [{ "type": "object", "ingredients": null, "required": null, "servings": 2, "tags": ["quick"] }],
    }],
  });

  let (status, reply) = send_raw(&petstore, Method::POST, "/assistant/messages", Some(&body)).await;

  assert_eq!(status, StatusCode::OK);
  let Some(server::ContentBlock::Text(received)) = received_message(&petstore).blocks.pop() else {
    panic!("the server did not extract a text block");
  };
  assert_eq!(
    received.recipes,
    Some(vec![server::RecipeList {
      ingredients: None,
      required: None,
      r#type: "object".to_string(),
      additional_properties: object(json!({ "servings": 2, "tags": ["quick"] })),
    }])
  );
  assert_eq!(
    reply["content"]["recipes"],
    json!([{ "type": "object", "servings": 2, "tags": ["quick"] }])
  );
}

#[rstest]
#[tokio::test]
async fn the_reply_echoes_the_last_block_and_counts_every_block(
  #[future(awt)] petstore: Petstore,
  #[values(1, 2, 5)] count: usize,
) {
  let blocks = (0..count)
    .map(|index| client::ContentBlock::text(format!("block {index}")))
    .collect::<Vec<_>>();

  let reply = send_blocks(&petstore, blocks.clone()).await;

  assert_eq!(received_message(&petstore).blocks.len(), count);
  assert_eq!(Some(reply.content.as_ref()), blocks.last());
  assert_eq!(
    reply.usage,
    Some(client::Usage {
      input_tokens: i64::try_from(count).expect("a small count"),
      output_tokens: 1,
    })
  );
  assert!(reply.id.starts_with("msg_"), "unexpected message id {}", reply.id);
}

#[rstest]
#[tokio::test]
async fn empty_blocks_are_a_bad_request_with_an_error_envelope(#[future(awt)] petstore: Petstore) {
  let response = send_message(&petstore, client::ContentRequest::default()).await;

  let client::ApiResponse::BadRequest(failure) = response else {
    panic!("expected 400 Bad Request, got {response:?}");
  };
  assert_eq!(
    failure,
    client::ErrorResponse {
      r#type: "error".to_string(),
      error: client::ErrorDetails {
        r#type: client::ErrorType::InvalidRequestError,
        message: "blocks must not be empty".to_string(),
      },
    }
  );

  let (status, reply) = send_raw(
    &petstore,
    Method::POST,
    "/assistant/messages",
    Some(&json!({ "blocks": [] })),
  )
  .await;
  assert_eq!(status, StatusCode::BAD_REQUEST);
  assert_eq!(
    reply,
    json!({ "type": "error", "error": { "type": "invalid_request_error", "message": "blocks must not be empty" } })
  );
}

#[rstest]
#[case::absent(None, None)]
#[case::string(
  Some(client::MessageMetadata::String("triage".to_string())),
  Some(server::MessageMetadata::String("triage".to_string()))
)]
#[case::object(
  Some(client::MessageMetadata::Object(object(json!({ "pet": "Biscuit", "priority": 2, "tags": ["urgent"], "owner": { "id": 7 } })))),
  Some(server::MessageMetadata::Object(object(json!({ "pet": "Biscuit", "priority": 2, "tags": ["urgent"], "owner": { "id": 7 } }))))
)]
#[case::empty_object(
  Some(client::MessageMetadata::Object(IndexMap::new())),
  Some(server::MessageMetadata::Object(IndexMap::new()))
)]
#[tokio::test]
async fn metadata_reaches_the_server_as_a_string_or_an_object(
  #[future(awt)] petstore: Petstore,
  #[case] sent: Option<client::MessageMetadata>,
  #[case] expected: Option<server::MessageMetadata>,
) {
  let response = send_message(
    &petstore,
    client::ContentRequest {
      blocks: vec![client::ContentBlock::text("hello".to_string())],
      metadata: sent,
      ..Default::default()
    },
  )
  .await;

  assert!(matches!(response, client::ApiResponse::Ok(_)), "{response:?}");
  assert_eq!(received_message(&petstore).metadata, expected);
}

#[rstest]
#[case::absent_takes_the_spec_default(
  None,
  None,
  server::ContentRequestImageSize::Preset(server::ImageSizePreset::Auto2k)
)]
#[case::client_default(
  Some(client::ContentRequestImageSize::default()),
  Some(json!("auto_2K")),
  server::ContentRequestImageSize::Preset(server::ImageSizePreset::Auto2k)
)]
#[case::square_hd(
  Some(client::ContentRequestImageSize::square_hd()),
  Some(json!("square_hd")),
  server::ContentRequestImageSize::Preset(server::ImageSizePreset::SquareHd)
)]
#[case::auto_1k(
  Some(client::ContentRequestImageSize::auto1k()),
  Some(json!("auto_1K")),
  server::ContentRequestImageSize::Preset(server::ImageSizePreset::Auto1k)
)]
#[case::landscape(
  Some(client::ContentRequestImageSize::landscape169()),
  Some(json!("landscape_16_9")),
  server::ContentRequestImageSize::Preset(server::ImageSizePreset::Landscape169)
)]
#[case::explicit_size(
  Some(client::ContentRequestImageSize::ImageSize(client::ImageSize { width: Some(1024), height: Some(768) })),
  Some(json!({ "width": 1024, "height": 768 })),
  server::ContentRequestImageSize::ImageSize(server::ImageSize { width: Some(1024), height: Some(768) })
)]
#[case::size_without_height(
  Some(client::ContentRequestImageSize::ImageSize(client::ImageSize { width: Some(2048), height: None })),
  Some(json!({ "width": 2048 })),
  server::ContentRequestImageSize::ImageSize(server::ImageSize { width: Some(2048), height: Some(512) })
)]
#[tokio::test]
async fn image_size_reaches_the_server_as_a_size_or_a_preset(
  #[future(awt)] petstore: Petstore,
  #[case] sent: Option<client::ContentRequestImageSize>,
  #[case] wire: Option<Value>,
  #[case] expected: server::ContentRequestImageSize,
) {
  let body = client::ContentRequest {
    blocks: vec![client::ContentBlock::text("draw Biscuit".to_string())],
    metadata: None,
    image_size: sent,
  };
  assert_eq!(
    to_json(&body).get("image_size"),
    wire.as_ref(),
    "the client's wire form"
  );

  let response = send_message(&petstore, body).await;

  assert!(matches!(response, client::ApiResponse::Ok(_)), "{response:?}");
  assert_eq!(received_message(&petstore).image_size, Some(expected));
}

/// The required `conversation` cookie parameter reaches the server percent-decoded, and a
/// request without it, or with a value that breaks its constraints, never reaches the service.
#[rstest]
#[case::plain(Some("conversation=chat-42"), StatusCode::OK, Some("chat-42"))]
#[case::encoded(Some("conversation=caf%C3%A9%3B%20chat"), StatusCode::OK, Some("café; chat"))]
#[case::among_other_cookies(Some("theme=dark; conversation=chat-7; session=abc"), StatusCode::OK, Some("chat-7"))]
#[case::missing(None, StatusCode::BAD_REQUEST, None)]
#[case::empty(Some("conversation="), StatusCode::OK, Some(""))]
#[tokio::test]
async fn the_conversation_cookie_selects_the_replayed_events(
  #[future(awt)] petstore: Petstore,
  #[case] cookie: Option<&str>,
  #[case] expected: StatusCode,
  #[case] received: Option<&str>,
) {
  let mut request = reqwest::Client::new()
    .get(petstore.url("/assistant/events"))
    .header(client::X_API_KEY, API_KEY);
  if let Some(cookie) = cookie {
    request = request.header(reqwest::header::COOKIE, cookie);
  }

  let response = request.send().await.expect("the raw request completes");

  assert_eq!(response.status(), expected, "{cookie:?}");
  let reason = response.text().await.expect("the reply has a body");
  if expected == StatusCode::BAD_REQUEST {
    assert!(reason.contains("missing required cookie `conversation`"), "{reason}");
  }
  let recorded = match petstore.service.last_received() {
    Some(Received::ListAssistantEvents(request)) => Some(request.cookie.conversation),
    _ => None,
  };
  assert_eq!(recorded.as_deref(), received);
}

#[rstest]
#[case::empty("")]
#[case::one_character("x")]
fn the_client_validates_the_conversation_cookie(#[case] conversation: &str) {
  let built = client::ListAssistantEventsRequest::builder()
    .conversation(conversation.to_string())
    .build();

  if conversation.is_empty() {
    let error = built
      .expect_err("an empty conversation breaks `minLength: 1`")
      .to_string();
    assert!(error.contains("conversation"), "{error}");
  } else {
    built.expect("a one-character conversation meets `minLength: 1`");
  }
}

#[rstest]
#[tokio::test]
async fn assistant_events_parse_into_their_variants_in_order(#[future(awt)] petstore: Petstore) {
  let response = petstore
    .client
    .list_assistant_events(
      client::ListAssistantEventsRequest::builder()
        .conversation("conversation-1".to_string())
        .build()
        .expect("a valid listAssistantEvents request"),
    )
    .await
    .expect("the request completes");

  let client::ApiResponse::Ok(list) = response else {
    panic!("expected 200 OK, got {response:?}");
  };
  assert!(matches!(
    petstore.service.last_received(),
    Some(Received::ListAssistantEvents(_))
  ));
  assert_eq!(
    list.events,
    vec![
      client::AssistantEvent::message_start(client::Message {
        id: "msg_1".to_string(),
        role: client::Role::Assistant,
        content: vec![],
        model: "chat-1.1".to_string(),
        stop_reason: None,
        usage: Some(client::Usage {
          input_tokens: 12,
          output_tokens: 0,
        }),
      }),
      client::AssistantEvent::ContentBlockStart(client::ContentBlockStartEvent {
        index: 0,
        content_block: Box::new(client::ContentBlock::text("Biscuit is due for a booster.".to_string())),
        ..Default::default()
      }),
      client::AssistantEvent::ContentBlockDelta(client::ContentBlockDeltaEvent {
        index: 0,
        delta: Box::new(client::Delta::text(" Book it this week.".to_string())),
        ..Default::default()
      }),
      client::AssistantEvent::ContentBlockDelta(client::ContentBlockDeltaEvent {
        index: 0,
        delta: Box::new(client::Delta::input_json("{\"vaccine\":".to_string())),
        ..Default::default()
      }),
      client::AssistantEvent::content_block_stop(0),
      client::AssistantEvent::ping(),
      client::AssistantEvent::message_stop(),
    ]
  );

  let (status, raw) = send_raw(&petstore, Method::GET, "/assistant/events", None).await;
  assert_eq!(status, StatusCode::OK);
  let events = raw["events"].as_array().cloned().unwrap_or_default();
  let event_tags = events.iter().map(|event| event["type"].clone()).collect::<Vec<_>>();
  let delta_tags = events
    .iter()
    .filter_map(|event| event.get("delta"))
    .map(|delta| delta["type"].clone())
    .collect::<Vec<_>>();
  assert_eq!(
    event_tags,
    [
      "message_start",
      "content_block_start",
      "content_block_delta",
      "content_block_delta",
      "content_block_stop",
      "ping",
      "message_stop",
    ]
  );
  assert_eq!(delta_tags, ["text_delta", "input_json_delta"]);
}

#[rstest]
#[tokio::test]
async fn a_fresh_store_answers_with_the_spec_default_preference(#[future(awt)] petstore: Petstore) {
  let preference = get_preference(&petstore).await;

  assert_eq!(
    preference,
    client::ChatPreference {
      model: client::ChatModelId::Chat11,
      object: Some("chat.preference".to_string()),
      fallback: Some(client::ChatModel::Chat11),
      tier: Some(client::ChatPreferenceTier::Other2("flex".to_string())),
    }
  );
  assert!(matches!(
    petstore.service.last_received(),
    Some(Received::GetChatPreference(_))
  ));

  let (status, raw) = send_raw(&petstore, Method::GET, "/assistant/preferences", None).await;
  assert_eq!(status, StatusCode::OK);
  assert_eq!(
    raw,
    json!({ "model": "chat-1.1", "object": "chat.preference", "fallback": "chat-1.1", "tier": "flex" })
  );
}

#[rstest]
#[tokio::test]
async fn chat_preferences_round_trip_listed_and_unlisted_values(
  #[future(awt)] petstore: Petstore,
  #[values(
    client::ChatModelId::Chat10,
    client::ChatModelId::Chat11,
    client::ChatModelId::Other("chat-2.0-preview".to_string()),
    client::ChatModelId::Other("Pet Vet 7B".to_string())
  )]
  model: client::ChatModelId,
  #[values(
    client::ChatPreferenceTier::Standard,
    client::ChatPreferenceTier::Priority,
    client::ChatPreferenceTier::Other,
    client::ChatPreferenceTier::Other2("flex".to_string()),
    client::ChatPreferenceTier::Other2("enterprise".to_string())
  )]
  tier: client::ChatPreferenceTier,
) {
  let preference = client::ChatPreference {
    model,
    object: Some("chat.preference".to_string()),
    fallback: Some(client::ChatModel::Chat10),
    tier: Some(tier),
  };

  let stored = put_preference(&petstore, preference.clone()).await;

  assert_eq!(
    to_json(&received_preference(&petstore)),
    to_json(&preference),
    "the server extracted the preference as sent"
  );
  assert_eq!(stored, preference, "the update echoes the preference");
  assert_eq!(
    get_preference(&petstore).await,
    preference,
    "the store keeps the preference"
  );
}

#[rstest]
#[tokio::test]
async fn absent_preference_members_take_spec_defaults(#[future(awt)] petstore: Petstore) {
  let preference = client::ChatPreference {
    model: client::ChatModelId::Other("chat-9".to_string()),
    object: None,
    fallback: None,
    tier: None,
  };
  assert_eq!(
    to_json(&preference),
    json!({ "model": "chat-9" }),
    "the client omits absent members"
  );

  let stored = put_preference(&petstore, preference).await;

  assert_eq!(
    to_json(&received_preference(&petstore)),
    json!({ "model": "chat-9", "object": "chat.preference", "fallback": "chat-1.1", "tier": "flex" }),
    "the server fills the spec defaults"
  );
  assert_eq!(
    stored,
    client::ChatPreference {
      model: client::ChatModelId::Other("chat-9".to_string()),
      object: Some("chat.preference".to_string()),
      fallback: Some(client::ChatModel::Chat11),
      tier: Some(client::ChatPreferenceTier::Other2("flex".to_string())),
    }
  );
}

/// The server reads a constant as sent so an implementation can reject a misstated one,
/// while the generated client always sends the constant and reports it on input.
#[rstest]
#[tokio::test]
async fn the_server_reads_a_misstated_object_constant_and_the_client_reports_the_constant(
  #[future(awt)] petstore: Petstore,
) {
  assert_eq!(
    to_json(&client::ChatPreference {
      model: client::ChatModelId::Chat10,
      ..Default::default()
    })["object"],
    "chat.preference",
    "the generated client sends the constant"
  );
  let body = json!({ "model": "chat-1.0", "object": "chat.thread" });

  let (status, reply) = send_raw(&petstore, Method::PUT, "/assistant/preferences", Some(&body)).await;

  assert_eq!(status, StatusCode::OK);
  assert_eq!(
    received_preference(&petstore).object.as_deref(),
    Some("chat.thread"),
    "the server received the misstated value"
  );
  let echoed = serde_json::from_value::<client::ChatPreference>(reply).expect("the client decodes the reply");
  assert_eq!(
    echoed.object.as_deref(),
    Some("chat.preference"),
    "the client reports the constant"
  );
  assert_eq!(
    get_preference(&petstore).await.object.as_deref(),
    Some("chat.preference"),
    "the generated client reports the constant for the stored preference"
  );
}

#[rstest]
#[case::model_filter_without_model(
  client::AssistantSettings { model_filter: Some(client::ModelFilter { model: None }), ..Default::default() },
  "modelFilter",
  "{}"
)]
#[case::model_filter_listed(
  client::AssistantSettings {
    model_filter: Some(client::ModelFilter {
      model: Some(client::ModelFilterModel::Enum(client::ModelFilterModelEnum::Model10Draft)),
    }),
    ..Default::default()
  },
  "modelFilter",
  r#"{"model":"model-1.0-draft"}"#
)]
#[case::model_filter_legacy(
  client::AssistantSettings {
    model_filter: Some(client::ModelFilter {
      model: Some(client::ModelFilterModel::Enum2(client::ModelFilterModelEnum2::Legacy01Plus)),
    }),
    ..Default::default()
  },
  "modelFilter",
  r#"{"model":"Legacy 0.1-plus"}"#
)]
#[case::model_selection_default(
  client::AssistantSettings { model_selection: Some(client::ModelSelection { model: None }), ..Default::default() },
  "modelSelection",
  "{}"
)]
#[case::model_selection_legacy(
  client::AssistantSettings {
    model_selection: Some(client::ModelSelection {
      model: Some(client::ModelFilterModel::Enum2(client::ModelFilterModelEnum2::Legacy01Mini)),
    }),
    ..Default::default()
  },
  "modelSelection",
  r#"{"model":"Legacy 0.1-mini"}"#
)]
#[case::model_requirement(
  client::AssistantSettings {
    model_requirement: Some(client::ModelRequirement {
      model: client::ModelFilterModel::Enum(client::ModelFilterModelEnum::Model11),
    }),
    ..Default::default()
  },
  "modelRequirement",
  r#"{"model":"model-1.1"}"#
)]
#[case::model_choice_listed(
  client::AssistantSettings { model_choice: Some(client::ChatModelChoice::chat10()), ..Default::default() },
  "modelChoice",
  r#""chat-1.0""#
)]
#[case::model_choice_unlisted(
  client::AssistantSettings {
    model_choice: Some(client::ChatModelChoice::String("pet-vet-ft".to_string())),
    ..Default::default()
  },
  "modelChoice",
  r#""pet-vet-ft""#
)]
#[case::response_kind_text(
  client::AssistantSettings { response_kind: Some(client::ResponseKind::Text), ..Default::default() },
  "responseKind",
  r#""text""#
)]
#[case::response_kind_json(
  client::AssistantSettings { response_kind: Some(client::ResponseKind::Json), ..Default::default() },
  "responseKind",
  r#""json""#
)]
#[case::usage_auto(
  client::AssistantSettings { usage: Some(client::UsageChoice::Auto), ..Default::default() },
  "usage",
  r#""auto""#
)]
#[case::usage_object(
  client::AssistantSettings {
    usage: Some(client::UsageChoice::Usage(client::Usage { input_tokens: 3, output_tokens: 4 })),
    ..Default::default()
  },
  "usage",
  r#"{"input_tokens":3,"output_tokens":4}"#
)]
#[case::usage_string(
  client::AssistantSettings { usage: Some(client::UsageChoice::String("manual".to_string())), ..Default::default() },
  "usage",
  r#""manual""#
)]
#[case::streaming_on(
  client::AssistantSettings { streaming: Some(client::Toggle::True), ..Default::default() },
  "streaming",
  "true"
)]
#[case::streaming_off(
  client::AssistantSettings { streaming: Some(client::Toggle::False), ..Default::default() },
  "streaming",
  "false"
)]
#[case::quality_auto(
  client::AssistantSettings { quality: Some(client::ResponseQuality::Auto), ..Default::default() },
  "quality",
  r#""auto""#
)]
#[case::quality_one(
  client::AssistantSettings { quality: Some(client::ResponseQuality::Value1), ..Default::default() },
  "quality",
  "1"
)]
#[case::quality_half(
  client::AssistantSettings { quality: Some(client::ResponseQuality::Value0_5), ..Default::default() },
  "quality",
  "0.5"
)]
#[case::quality_true(
  client::AssistantSettings { quality: Some(client::ResponseQuality::True), ..Default::default() },
  "quality",
  "true"
)]
#[case::priority_one(
  client::AssistantSettings { priority: Some(client::Priority::Value1), ..Default::default() },
  "priority",
  "1"
)]
#[case::priority_three(
  client::AssistantSettings { priority: Some(client::Priority::Value3), ..Default::default() },
  "priority",
  "3"
)]
#[case::temperature_number(
  client::AssistantSettings { temperature: Some(client::NullableStringOrNumber::Number(0.7)), ..Default::default() },
  "temperature",
  "0.7"
)]
#[case::temperature_string(
  client::AssistantSettings {
    temperature: Some(client::NullableStringOrNumber::String("warm".to_string())),
    ..Default::default()
  },
  "temperature",
  r#""warm""#
)]
#[case::highlights_single(
  client::AssistantSettings { highlights: Some(client::ArrayOrSingle::text_block("Biscuit".to_string())), ..Default::default() },
  "highlights",
  r#"{"type":"text","text":"Biscuit"}"#
)]
#[case::highlights_array(
  client::AssistantSettings {
    highlights: Some(client::ArrayOrSingle::Array(vec![text("walks"), text("naps")])),
    ..Default::default()
  },
  "highlights",
  r#"[{"type":"text","text":"walks"},{"type":"text","text":"naps"}]"#
)]
#[case::highlights_empty_array(
  client::AssistantSettings { highlights: Some(client::ArrayOrSingle::Array(vec![])), ..Default::default() },
  "highlights",
  "[]"
)]
#[case::citation_citation(
  client::AssistantSettings {
    citation: Some(client::HypotheticalVersion::Citation(client::CitationAnnotation {
      start: 0,
      end: 7,
      build: "b-42".to_string(),
      ..Default::default()
    })),
    ..Default::default()
  },
  "citation",
  r#"{"type":"citation","start":0,"end":7,"build":"b-42"}"#
)]
#[case::citation_link(
  client::AssistantSettings {
    citation: Some(client::HypotheticalVersion::Link(client::LinkAnnotation {
      start: 8,
      end: 12,
      url: "https://vet.example/booster".to_string(),
      ..Default::default()
    })),
    ..Default::default()
  },
  "citation",
  r#"{"type":"link","start":8,"end":12,"url":"https://vet.example/booster"}"#
)]
#[case::sample_rate_lowest(
  client::AssistantSettings { sample_rate: Some(client::SampleRate::Value8000), ..Default::default() },
  "sampleRate",
  "8000"
)]
#[case::sample_rate_cd(
  client::AssistantSettings { sample_rate: Some(client::SampleRate::Value44100), ..Default::default() },
  "sampleRate",
  "44100"
)]
#[case::playback_half(
  client::AssistantSettings { playback_rate: Some(client::PlaybackRate::Value0_5), ..Default::default() },
  "playbackRate",
  "0.5"
)]
#[case::playback_normal(
  client::AssistantSettings { playback_rate: Some(client::PlaybackRate::Value1), ..Default::default() },
  "playbackRate",
  "1.0"
)]
#[case::playback_double(
  client::AssistantSettings { playback_rate: Some(client::PlaybackRate::Value2), ..Default::default() },
  "playbackRate",
  "2.0"
)]
#[case::stop_reason_tool_use(
  client::AssistantSettings { stop_reason: Some(client::StopReason::ToolUse), ..Default::default() },
  "stopReason",
  r#""tool_use""#
)]
#[case::stop_reason_stop_sequence(
  client::AssistantSettings { stop_reason: Some(client::StopReason::StopSequence), ..Default::default() },
  "stopReason",
  r#""stop_sequence""#
)]
#[case::role_user(
  client::AssistantSettings { role: Some(client::Role::User), ..Default::default() },
  "role",
  r#""user""#
)]
#[case::role_assistant(
  client::AssistantSettings { role: Some(client::Role::Assistant), ..Default::default() },
  "role",
  r#""assistant""#
)]
#[tokio::test]
async fn every_settings_member_round_trips_in_its_wire_form(
  #[future(awt)] petstore: Petstore,
  #[case] settings: client::AssistantSettings,
  #[case] member: &str,
  #[case] wire: &str,
) {
  let member_wire = serde_json::to_string(&to_json(&settings)[member]).expect("JSON text");
  assert_eq!(member_wire, wire, "the client's wire form of `{member}`");
  let expected = with_spec_defaults(settings.clone());

  let echoed = put_settings(&petstore.client, settings).await;

  assert_eq!(
    to_json(&received_settings(&petstore)),
    to_json(&expected),
    "the server extracted `{member}` as sent"
  );
  assert_eq!(echoed, expected, "the client parsed `{member}` back unchanged");
}

#[rstest]
#[tokio::test]
async fn absent_settings_take_spec_defaults(#[future(awt)] petstore: Petstore) {
  let settings = client::AssistantSettings {
    response_kind: None,
    usage: None,
    sample_rate: None,
    playback_rate: None,
    ..Default::default()
  };
  assert_eq!(to_json(&settings), json!({}), "the client omits absent members");

  let echoed = put_settings(&petstore.client, settings).await;

  assert_eq!(
    to_json(&received_settings(&petstore)),
    json!({ "responseKind": "json", "usage": "auto", "sampleRate": 24000, "playbackRate": 1.0 }),
    "the server fills the spec defaults"
  );
  assert_eq!(echoed.response_kind, Some(client::ResponseKind::Json));
  assert_eq!(echoed.usage, Some(client::UsageChoice::Auto));
  assert_eq!(echoed.sample_rate, Some(client::SampleRate::Value24000));
  assert_eq!(echoed.playback_rate, Some(client::PlaybackRate::Value1));
}

#[rstest]
#[tokio::test]
async fn nullable_settings_accept_explicit_null(#[future(awt)] petstore: Petstore) {
  let body = json!({ "modelFilter": { "model": null }, "temperature": null, "citation": null });

  let (status, reply) = send_raw(&petstore, Method::PUT, "/assistant/settings", Some(&body)).await;

  assert_eq!(status, StatusCode::OK);
  let received = received_settings(&petstore);
  assert_eq!(received.model_filter, Some(server::ModelFilter { model: None }));
  assert_eq!(received.temperature, None);
  assert_eq!(received.citation, None);
  assert_eq!(
    reply,
    json!({ "modelFilter": {}, "responseKind": "json", "usage": "auto", "sampleRate": 24000, "playbackRate": 1.0 })
  );
}

fn property_config() -> ProptestConfig {
  ProptestConfig {
    failure_persistence: None,
    ..ProptestConfig::with_cases(64)
  }
}

/// Text that satisfies the implicit `minLength: 1` on required strings.
fn required_text() -> impl Strategy<Value = String> {
  "\\PC{1,16}"
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

/// A free-form object whose keys never collide with a schema's named properties.
fn free_form_object() -> impl Strategy<Value = IndexMap<String, Value>> {
  collection::vec(("x_[a-z]{1,6}", json_value()), 0..4).prop_map(|members| members.into_iter().collect())
}

prop_compose! {
  fn citation()(start in 0i64..500, length in 0i64..500, build in required_text()) -> client::CitationAnnotation {
    client::CitationAnnotation { start, end: start + length, build, ..Default::default() }
  }
}

prop_compose! {
  fn link()(start in 0i64..500, length in 0i64..500, url in web_url()) -> client::LinkAnnotation {
    client::LinkAnnotation { start, end: start + length, url, ..Default::default() }
  }
}

fn annotation() -> impl Strategy<Value = client::Annotation> {
  prop_oneof![
    citation().prop_map(client::Annotation::Citation),
    link().prop_map(client::Annotation::Link),
  ]
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
  fn text_block()(
    text in required_text(),
    annotations in option::of(collection::vec(annotation(), 0..3)),
    recipes in option::of(collection::vec(recipe(), 0..2)),
  ) -> client::TextBlock {
    client::TextBlock { text, annotations, recipes, ..Default::default() }
  }
}

fn media_type() -> impl Strategy<Value = client::MediaType> {
  select(vec![
    client::MediaType::ImageJpeg,
    client::MediaType::ImagePng,
    client::MediaType::ImageGif,
    client::MediaType::ImageWebp,
  ])
}

fn image_source() -> impl Strategy<Value = client::ImageSource> {
  prop_oneof![
    (media_type(), collection::vec(any::<u8>(), 0..48)).prop_map(|(media_type, data)| {
      client::ImageSource::Base64(client::Base64ImageSource {
        media_type,
        data,
        ..Default::default()
      })
    }),
    web_url().prop_map(client::ImageSource::url),
  ]
}

prop_compose! {
  fn image_block()(source in image_source(), alt_text in option::of("\\PC{0,16}")) -> client::ImageBlock {
    client::ImageBlock { source: Box::new(source), alt_text, ..Default::default() }
  }
}

prop_compose! {
  fn code_block()(code in required_text(), language in option::of("[a-z]{1,10}")) -> client::CodeBlock {
    client::CodeBlock { code, language, ..Default::default() }
  }
}

prop_compose! {
  fn tool_use_block()(id in "toolu_[a-z0-9]{1,12}", name in "[a-z_]{1,16}", input in free_form_object()) -> client::ToolUseBlock {
    client::ToolUseBlock { id, name, input, ..Default::default() }
  }
}

fn usage_counter() -> impl Strategy<Value = client::UsageCounterUsageCounterKind> {
  prop_oneof![
    (0i64..10_000).prop_map(client::UsageCounterUsageCounterKind::a),
    (0i64..10_000).prop_map(client::UsageCounterUsageCounterKind::b),
  ]
}

fn tool_result_content() -> impl Strategy<Value = client::ToolResultContent> {
  let nested_block = prop_oneof![
    text_block().prop_map(client::ToolResultContentBlock::Text),
    image_block().prop_map(client::ToolResultContentBlock::Image),
  ];
  prop_oneof![
    "\\PC{0,24}".prop_map(client::ToolResultContent::String),
    collection::vec(nested_block, 0..3).prop_map(client::ToolResultContent::Array),
  ]
}

prop_compose! {
  fn tool_result_block()(
    tool_use_id in "toolu_[a-z0-9]{1,12}",
    content in tool_result_content(),
    iterations in option::of(collection::vec(usage_counter(), 0..3)),
    is_error in option::of(any::<bool>()),
  ) -> client::ToolResultBlock {
    client::ToolResultBlock { tool_use_id, content, iterations, is_error, ..Default::default() }
  }
}

fn content_block() -> impl Strategy<Value = client::ContentBlock> {
  prop_oneof![
    text_block().prop_map(client::ContentBlock::Text),
    image_block().prop_map(client::ContentBlock::Image),
    code_block().prop_map(client::ContentBlock::Code),
    tool_use_block().prop_map(client::ContentBlock::ToolUse),
    tool_result_block().prop_map(client::ContentBlock::ToolResult),
  ]
}

#[test]
fn arbitrary_content_block_trees_round_trip() {
  let petstore = start_shared();
  proptest!(property_config(), |(blocks in collection::vec(content_block(), 1..4))| {
    run_case(async || {
      let reply = send_blocks(&petstore, blocks.clone()).await;
      prop_assert_eq!(to_json(&received_message(&petstore).blocks), to_json(&blocks));
      prop_assert_eq!(Some(reply.content.as_ref()), blocks.last());
      prop_assert_eq!(reply.usage.map(|usage| usage.input_tokens), i64::try_from(blocks.len()).ok());
      Ok::<_, TestCaseError>(())
    })?;
  });
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

fn chat_model_choice() -> impl Strategy<Value = client::ChatModelChoice> {
  prop_oneof![
    Just(client::ChatModelChoice::chat10()),
    Just(client::ChatModelChoice::chat11()),
    "[a-z][a-z0-9]{0,11}".prop_map(client::ChatModelChoice::String),
  ]
}

fn usage_choice() -> impl Strategy<Value = client::UsageChoice> {
  prop_oneof![
    Just(client::UsageChoice::Auto),
    (0i64..1_000_000, 0i64..1_000_000).prop_map(|(input_tokens, output_tokens)| {
      client::UsageChoice::Usage(client::Usage {
        input_tokens,
        output_tokens,
      })
    }),
    "[a-z]{1,10}"
      .prop_filter("the constant parses as Auto", |choice| choice != "auto")
      .prop_map(client::UsageChoice::String),
  ]
}

fn temperature() -> impl Strategy<Value = client::NullableStringOrNumber> {
  prop_oneof![
    (-100.0..100.0).prop_map(client::NullableStringOrNumber::Number),
    "\\PC{0,8}".prop_map(client::NullableStringOrNumber::String),
  ]
}

fn highlights() -> impl Strategy<Value = client::ArrayOrSingle> {
  prop_oneof![
    text_block().prop_map(client::ArrayOrSingle::TextBlock),
    collection::vec(text_block(), 0..3).prop_map(client::ArrayOrSingle::Array),
  ]
}

fn citation_union() -> impl Strategy<Value = client::HypotheticalVersion> {
  prop_oneof![
    citation().prop_map(client::HypotheticalVersion::Citation),
    link().prop_map(client::HypotheticalVersion::Link),
  ]
}

prop_compose! {
  fn assistant_settings()(
    (model_filter, model_selection, model_requirement, model_choice) in (
      option::of(option::of(model_union())),
      option::of(option::of(model_union())),
      option::of(model_union()),
      option::of(chat_model_choice()),
    ),
    (response_kind, usage, streaming, quality, priority, temperature) in (
      option::of(select(vec![client::ResponseKind::Text, client::ResponseKind::Json])),
      option::of(usage_choice()),
      option::of(select(vec![client::Toggle::True, client::Toggle::False])),
      option::of(select(vec![
        client::ResponseQuality::Auto,
        client::ResponseQuality::Value1,
        client::ResponseQuality::Value0_5,
        client::ResponseQuality::True,
      ])),
      option::of(select(vec![client::Priority::Value1, client::Priority::Value2, client::Priority::Value3])),
      option::of(temperature()),
    ),
    (highlights, citation, sample_rate, playback_rate, stop_reason, role) in (
      option::of(highlights()),
      option::of(citation_union()),
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

#[test]
fn arbitrary_assistant_settings_round_trip() {
  let petstore = start_shared();
  proptest!(property_config(), |(settings in assistant_settings())| {
    run_case(async || {
      let expected = with_spec_defaults(settings.clone());
      let echoed = put_settings(&petstore.client, settings).await;
      prop_assert_eq!(to_json(&received_settings(&petstore)), to_json(&expected));
      prop_assert_eq!(echoed, expected);
      Ok::<_, TestCaseError>(())
    })?;
  });
}

/// The server echoes `boost`, or the path value when `boost` is absent.
#[rstest]
#[case::path_auto(client::ResponseQuality::Auto, None)]
#[case::path_integer(client::ResponseQuality::Value1, None)]
#[case::path_float(client::ResponseQuality::Value0_5, None)]
#[case::path_boolean(client::ResponseQuality::True, None)]
#[case::query_auto(client::ResponseQuality::Value1, Some(client::ResponseQuality::Auto))]
#[case::query_integer(client::ResponseQuality::Auto, Some(client::ResponseQuality::Value1))]
#[case::query_float(client::ResponseQuality::Auto, Some(client::ResponseQuality::Value0_5))]
#[case::query_boolean(client::ResponseQuality::Auto, Some(client::ResponseQuality::True))]
#[case::query_tiny_float(client::ResponseQuality::Auto, Some(client::ResponseQuality::Value0_000001))]
#[case::query_huge_integer(
  client::ResponseQuality::Auto,
  Some(client::ResponseQuality::Value1000000000000000000000)
)]
#[case::query_large_float(
  client::ResponseQuality::Auto,
  Some(client::ResponseQuality::Value18446744073709552000)
)]
#[tokio::test]
async fn mixed_enum_values_round_trip_through_path_and_query(
  #[future(awt)] petstore: Petstore,
  #[case] path: client::ResponseQuality,
  #[case] boost: Option<client::ResponseQuality>,
) {
  let expected = boost.clone().unwrap_or_else(|| path.clone());
  let request = client::EchoResponseQualityRequest::builder()
    .quality(path.clone())
    .maybe_boost(boost.clone())
    .build()
    .expect("the request builds");
  let client::ApiResponse::Ok(echoed) = petstore
    .client
    .echo_response_quality(request)
    .await
    .expect("the request completes")
  else {
    panic!("echoResponseQuality answers 200 for path {path:?} and boost {boost:?}");
  };

  let Some(Received::EchoResponseQuality(seen)) = petstore.service.last_received() else {
    panic!("the server did not record an echoResponseQuality request");
  };
  assert_eq!(
    to_json(&seen.path.quality),
    to_json(&path),
    "the path value crosses the wire as sent"
  );
  assert_eq!(
    to_json(&seen.query.boost),
    to_json(&boost),
    "the query value crosses the wire as sent"
  );
  assert_eq!(echoed, expected, "path {path:?}, boost {boost:?}");
}

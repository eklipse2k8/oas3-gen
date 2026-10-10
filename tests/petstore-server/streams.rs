use std::collections::BTreeMap;

use axum::response::sse::Event;
use chrono::TimeDelta;
use petstore_server as server;
use proptest::{collection, option, prelude::*, sample::select};
use reqwest::{Method, RequestBuilder, StatusCode};
use rstest::rstest;
use serde_json::json;

use crate::{
  Keyed as _, PET_ID, Raw as _, property_config, send,
  support::{Petstore, State, now, petstore},
};

type WireFrame = BTreeMap<String, String>;

fn wire_frames(body: &str) -> Vec<WireFrame> {
  body
    .split_terminator("\n\n")
    .map(|event| {
      event.lines().fold(WireFrame::new(), |mut fields, line| {
        let (field, value) = line.split_once(": ").unwrap_or((line, ""));
        fields
          .entry(field.to_string())
          .and_modify(|joined| {
            joined.push('\n');
            joined.push_str(value);
          })
          .or_insert_with(|| value.to_string());
        fields
      })
    })
    .collect()
}

fn frame(fields: &[(&str, &str)]) -> WireFrame {
  fields
    .iter()
    .map(|(field, value)| ((*field).to_string(), (*value).to_string()))
    .collect()
}

fn vitals(state: &mut State) {
  state.vitals = vec![
    server::VitalsReading {
      id: "reading-1".to_string(),
      heart_rate_bpm: 72,
      temperature_c: Some(38.5),
      activity: Some(server::VitalsReadingActivity::Running),
      timestamp: Some(now() + TimeDelta::milliseconds(250)),
    },
    server::VitalsReading {
      id: "reading-2".to_string(),
      ..server::VitalsReading::default()
    },
  ];
}

fn assistant_frames(state: &mut State) {
  state.assistant_frames = vec![
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
  ];
}

fn token_frames(state: &mut State) {
  state.token_frames = vec![
    server::AssistantTokenFrame::token("\"healthy\"\nand happy".to_string()),
    server::AssistantTokenFrame::Usage(server::UsageFrame {
      data: server::Usage {
        input_tokens: 3,
        output_tokens: 2,
      },
      id: Some("usage-1".to_string()),
      ..server::UsageFrame::default()
    }),
  ];
}

#[rstest]
#[case::json_items_as_data_only(
  vitals,
  |petstore: &Petstore| petstore.raw(Method::GET, &format!("/pets/{PET_ID}/vitals")).api_key(),
  vec![
    frame(&[(
      "data",
      r#"{"id":"reading-1","heartRateBpm":72,"temperatureC":38.5,"activity":"running","timestamp":"2026-10-09T18:00:00.250Z"}"#,
    )]),
    frame(&[("data", r#"{"id":"reading-2","heartRateBpm":0}"#)]),
  ],
)]
#[case::json_frames_ending_with_the_done_sentinel(
  assistant_frames,
  |petstore: &Petstore| petstore
    .raw(Method::POST, "/assistant/messages/stream?stream=true")
    .api_key()
    .json(&json!({ "blocks": [{ "type": "text", "text": "Summarize Biscuit's visit" }] })),
  vec![
    frame(&[("data", r#"{"type":"message_stop"}"#), ("event", "message_stop"), ("id", "evt-1"), ("retry", "1500")]),
    frame(&[("data", r#"{"type":"ping"}"#), ("event", "ping")]),
    frame(&[("data", "[DONE]")]),
  ],
)]
#[case::text_frames_ending_with_the_x_sse_sentinel(
  token_frames,
  |petstore: &Petstore| petstore.raw(Method::GET, "/assistant/tokens").api_key(),
  vec![
    frame(&[("data", "\"healthy\"\nand happy"), ("event", "token")]),
    frame(&[("data", r#"{"input_tokens":3,"output_tokens":2}"#), ("event", "usage"), ("id", "usage-1")]),
    frame(&[("data", "[END]")]),
  ],
)]
#[tokio::test]
async fn an_event_stream_sends_each_item_as_one_frame(
  #[future(awt)] petstore: Petstore,
  #[case] seed: fn(&mut State),
  #[case] request: fn(&Petstore) -> RequestBuilder,
  #[case] expected: Vec<WireFrame>,
) -> anyhow::Result<()> {
  petstore.service.with_state(seed);

  let reply = send(request(&petstore)).await?;

  let body = reply.text();
  assert_eq!(reply.status, StatusCode::OK, "{body}");
  assert_eq!(reply.media_type.as_deref(), Some("text/event-stream"));
  assert_eq!(wire_frames(&body), expected, "{body:?}");
  Ok(())
}

fn field_text() -> impl Strategy<Value = String> {
  collection::vec(select(vec!['a', 'Z', ' ', ':', '\r', '\n', '\0']), 0..6).prop_map(String::from_iter)
}

proptest! {
  #![proptest_config(property_config(256))]

  #[test]
  fn a_frame_converts_to_an_event_exactly_when_axum_can_send_it(
    event in field_text(),
    id in option::of(field_text()),
    retry in option::of(any::<i64>()),
    data in field_text(),
  ) {
    let event_is_sendable = !event.contains(['\r', '\n']);
    let id_is_sendable = id.as_deref().is_none_or(|id| !id.contains(['\0', '\r', '\n']));
    let retry_is_sendable = retry.is_none_or(|retry| retry >= 0);

    let token = server::AssistantTokenFrame::Token(server::TokenFrame { event: event.clone(), data });
    prop_assert_eq!(Event::try_from(token).is_ok(), event_is_sendable);
    let usage = server::AssistantTokenFrame::Usage(server::UsageFrame {
      event,
      id: id.clone(),
      ..server::UsageFrame::default()
    });
    prop_assert_eq!(Event::try_from(usage).is_ok(), event_is_sendable && id_is_sendable);
    let frame = server::AssistantEventFrame {
      data: Box::new(server::AssistantEvent::Ping(server::PingEvent::default())),
      event: server::AssistantEventFrameEvent::Ping,
      id,
      retry,
    };
    prop_assert_eq!(Event::try_from(frame).is_ok(), id_is_sendable && retry_is_sendable);
  }
}

use std::{convert::Infallible, time::Duration};

use axum::response::sse::{Event, Sse};
use futures::StreamExt as _;
use oas3_gen_support::DiagnosticsError;
use petstore_client::{self as client, ApiResponse, PetstoreClient};
use petstore_server as server;
use rstest::rstest;
use serde_json::{Value, json};

use crate::support::{Petstore, petstore};

async fn token_frames(
  client: &PetstoreClient,
) -> anyhow::Result<Vec<Result<client::AssistantTokenFrame, DiagnosticsError>>> {
  let frames = answered!(
    client.stream_assistant_tokens(client::StreamAssistantTokensRequest {}).await?,
    ApiResponse::Ok(frames) => frames
  );
  Ok(frames.collect().await)
}

fn token_frame(frame: &client::AssistantTokenFrame) -> (&str, Value, Option<&str>) {
  match frame {
    client::AssistantTokenFrame::Token(token) => (token.event().as_str(), json!(token.data()), token.id()),
    client::AssistantTokenFrame::Usage(usage) => (usage.event().as_str(), json!(usage.data()), usage.id()),
  }
}

#[rstest]
#[tokio::test]
async fn frame_streams_carry_every_field_and_end_at_the_sentinel(
  #[future(awt)] petstore: Petstore,
) -> anyhow::Result<()> {
  petstore.service.with_state(|state| {
    state.assistant_frames = vec![
      server::AssistantEventFrame {
        data: Box::new(server::AssistantEvent::message_stop()),
        event: server::AssistantEventFrameEvent::MessageStop,
        id: Some("evt-1".to_string()),
        retry: Some(1500),
      },
      server::AssistantEventFrame {
        data: Box::new(server::AssistantEvent::ping()),
        event: server::AssistantEventFrameEvent::Ping,
        id: None,
        retry: None,
      },
    ];
  });
  let request = client::StreamAssistantMessageRequest::builder()
    .stream(true)
    .body(client::ContentRequest {
      blocks: vec![client::ContentBlock::text("Summarize Biscuit's visit".to_string())],
      ..Default::default()
    })
    .build()?;

  let frames = answered!(
    petstore.client.stream_assistant_message(request).await?,
    ApiResponse::Ok(client::ContentResponseOrAssistantEventFrameStream::AssistantEventFrameStream(frames)) => frames
  );
  let frames = frames
    .collect::<Vec<_>>()
    .await
    .into_iter()
    .collect::<Result<Vec<_>, _>>()?;

  let read = frames
    .iter()
    .map(|frame| {
      (
        frame.data().as_ref().clone(),
        frame.event().clone(),
        frame.id(),
        frame.retry(),
      )
    })
    .collect::<Vec<_>>();
  let retry = Some(Duration::from_millis(1500));
  assert_eq!(
    read,
    [
      (
        client::AssistantEvent::message_stop(),
        client::AssistantEventFrameEvent::MessageStop,
        Some("evt-1"),
        retry
      ),
      (
        client::AssistantEvent::ping(),
        client::AssistantEventFrameEvent::Ping,
        Some("evt-1"),
        retry
      ),
    ],
    "a frame without an id or retry keeps the last ones the stream set"
  );
  assert_eq!(
    received!(petstore.service, StreamAssistantMessage).query.stream,
    Some(true)
  );
  Ok(())
}

#[rstest]
#[tokio::test]
async fn a_union_of_frames_picks_its_member_by_event_name(#[future(awt)] petstore: Petstore) -> anyhow::Result<()> {
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

  let frames = token_frames(&petstore.client)
    .await?
    .into_iter()
    .collect::<Result<Vec<_>, _>>()?;

  assert_eq!(
    frames.iter().map(token_frame).collect::<Vec<_>>(),
    [
      ("token", json!("Biscuit is"), None),
      ("token", json!("\"healthy\"\nand happy"), None),
      (
        "usage",
        json!({ "input_tokens": 3, "output_tokens": 2 }),
        Some("usage-1")
      ),
    ],
    "token text arrives as written, and the usage frame decodes its JSON"
  );
  Ok(())
}

#[rstest]
#[tokio::test]
async fn frames_that_do_not_decode_are_error_items_the_stream_continues_past() -> anyhow::Result<()> {
  let events = [
    Event::default().event("token").data("before"),
    Event::default().event("mystery").data("?"),
    Event::default()
      .event("usage")
      .data(r#"{"input_tokens":"many","output_tokens":1}"#),
    Event::default().event("token").data("after"),
    Event::default().data("[END]"),
    Event::default().event("token").data("ignored"),
  ];
  let router = axum::Router::new().fallback(move || {
    let events = events.clone();
    async move { Sse::new(futures::stream::iter(events.into_iter().map(Ok::<_, Infallible>))) }
  });
  let client = PetstoreClient::with_base_url(crate::support::serve(router).await)?;

  let items = token_frames(&client).await?;

  let [Ok(before), Err(unknown), Err(malformed), Ok(after)] = items.as_slice() else {
    anyhow::bail!("expected a token, two errors, then a token ending at the [END] sentinel; got {items:?}");
  };
  let read = [before, after].map(token_frame).map(|(event, data, _)| (event, data));
  assert_eq!(read, [("token", json!("before")), ("token", json!("after"))]);
  let DiagnosticsError::DeserializationError { path, inner } = unknown else {
    anyhow::bail!("expected an unknown event error, got {unknown:?}");
  };
  assert_eq!(path, "event");
  assert!(inner.to_string().contains("`mystery`"), "{inner}");
  let DiagnosticsError::DeserializationError { path, .. } = malformed else {
    anyhow::bail!("expected a JSON error, got {malformed:?}");
  };
  assert_eq!(path, "data.input_tokens");
  Ok(())
}

#[rstest]
fn server_frames_axum_cannot_send_are_errors_not_panics() {
  let unsendable = [
    Event::try_from(server::TokenFrame {
      event: "to\nken".to_string(),
      data: "text".to_string(),
    }),
    Event::try_from(server::UsageFrame {
      id: Some("usage\r1".to_string()),
      ..Default::default()
    }),
    Event::try_from(server::UsageFrame {
      id: Some("usage\x001".to_string()),
      ..Default::default()
    }),
    Event::try_from(server::AssistantEventFrame {
      data: Box::new(server::AssistantEvent::ping()),
      event: server::AssistantEventFrameEvent::Ping,
      id: None,
      retry: Some(-1),
    }),
  ];
  let sendable = Event::try_from(server::AssistantTokenFrame::Usage(server::UsageFrame {
    id: Some("usage-1".to_string()),
    ..Default::default()
  }));

  assert!(unsendable.iter().all(Result::is_err), "{unsendable:?}");
  assert!(sendable.is_ok(), "{sendable:?}");
}

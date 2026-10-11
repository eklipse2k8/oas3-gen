use std::{fmt::Debug, time::Duration};

use bytes::Bytes;
use futures::{StreamExt as _, executor::block_on};
use oas3_gen_support::{DiagnosticsError, EventStream, SseFrame};
use proptest::{collection, prelude::*, sample::Index};
use rstest::rstest;
use serde::Deserialize;

use crate::property_config;

const BYTE_ORDER_MARK: &[u8] = b"\xEF\xBB\xBF";

type Frame = (Option<String>, String, Option<String>, Option<u64>);

fn response(chunks: Vec<Vec<u8>>) -> reqwest::Response {
  body_response(chunks.into_iter().map(|chunk| Ok(Bytes::from(chunk))).collect())
}

fn body_response(chunks: Vec<std::io::Result<Bytes>>) -> reqwest::Response {
  reqwest::Response::from(http::Response::new(reqwest::Body::wrap_stream(futures::stream::iter(
    chunks,
  ))))
}

fn millis(retry: Option<Duration>) -> Option<u64> {
  retry.and_then(|retry| u64::try_from(retry.as_millis()).ok())
}

fn read(chunks: Vec<Vec<u8>>) -> Result<Vec<Frame>, DiagnosticsError> {
  block_on(EventStream::<SseFrame<String, Option<String>>>::text(response(chunks), None).collect::<Vec<_>>())
    .into_iter()
    .map(|frame| {
      frame.map(|frame| {
        (
          frame.event().clone(),
          frame.data().clone(),
          frame.id().map(str::to_string),
          millis(frame.retry()),
        )
      })
    })
    .collect()
}

fn reference(body: &[u8]) -> Vec<Frame> {
  let text = String::from_utf8_lossy(body);
  let mut rest = text.strip_prefix('\u{FEFF}').unwrap_or(&text);
  let (mut data, mut event, mut id, mut retry) = (String::new(), String::new(), None, None);
  let mut frames = vec![];
  while let Some(end) = rest.find(['\r', '\n']) {
    let line = &rest[..end];
    let ending = if rest[end..].starts_with("\r\n") { 2 } else { 1 };
    rest = &rest[end + ending..];
    if line.is_empty() {
      if !data.is_empty() {
        data.pop();
        frames.push((
          Some(event.clone()).filter(|event| !event.is_empty() && event != "message"),
          std::mem::take(&mut data),
          id.clone(),
          retry,
        ));
      }
      event.clear();
      continue;
    }
    if line.starts_with(':') {
      continue;
    }
    let (field, value) = match line.split_once(':') {
      Some((field, value)) => (field, value.strip_prefix(' ').unwrap_or(value)),
      None => (line, ""),
    };
    match field {
      "event" => value.clone_into(&mut event),
      "data" => {
        data.push_str(value);
        data.push('\n');
      }
      "id" if !value.contains('\0') => id = Some(value.to_string()).filter(|id| !id.is_empty()),
      "retry" if value.bytes().all(|byte| byte.is_ascii_digit()) => retry = value.parse().ok().or(retry),
      _ => {}
    }
  }
  frames
}

fn wire_body() -> impl Strategy<Value = Vec<u8>> {
  let name = prop_oneof![
    Just("data"),
    Just("event"),
    Just("id"),
    Just("retry"),
    Just("Data"),
    Just("other"),
    Just(""),
  ];
  let separator = prop_oneof![Just(&b": "[..]), Just(&b":"[..]), Just(&b""[..])];
  let value = prop_oneof![
    "[ -~]{0,6}".prop_map(String::into_bytes),
    "[0-9]{1,22}".prop_map(String::into_bytes),
    "\\PC{0,3}".prop_map(String::into_bytes),
    Just("\u{FEFF}".as_bytes().to_vec()),
    collection::vec(any::<u8>(), 0..4),
  ];
  let ending = prop_oneof![Just(&b"\n"[..]), Just(&b"\r"[..]), Just(&b"\r\n"[..])];
  let line = prop_oneof![
    4 => (name, separator, value.clone(), ending.clone())
      .prop_map(|(name, separator, value, ending)| [name.as_bytes(), separator, &value, ending].concat()),
    2 => ending.clone().prop_map(<[u8]>::to_vec),
    1 => (value, ending).prop_map(|(value, ending)| [b":", value.as_slice(), ending].concat()),
  ];
  (0..=BYTE_ORDER_MARK.len(), collection::vec(line, 0..32))
    .prop_map(|(mark, lines)| [&BYTE_ORDER_MARK[..mark], &lines.concat()].concat())
}

fn written_body() -> impl Strategy<Value = Vec<u8>> {
  let frame = (
    proptest::option::of("[^\r\n]{0,8}"),
    collection::vec("[^\r\n]{0,16}", 1..4),
    proptest::option::of("[^\r\n\0]{0,8}"),
    proptest::option::of(any::<u32>()),
  );
  let ending = prop_oneof![Just("\n"), Just("\r"), Just("\r\n")];
  (collection::vec(frame, 0..8), ending, any::<bool>()).prop_map(|(frames, ending, mark)| {
    let mut body = if mark { "\u{FEFF}".to_string() } else { String::new() };
    for (event, lines, id, retry) in frames {
      let mut field = |name: &str, value: &str| body.extend([name, ": ", value, ending]);
      field("", "comment");
      if let Some(event) = event {
        field("event", &event);
      }
      for line in lines {
        field("data", &line);
      }
      if let Some(id) = id {
        field("id", &id);
      }
      if let Some(retry) = retry {
        field("retry", &retry.to_string());
      }
      body.push_str(ending);
    }
    body.into_bytes()
  })
}

fn split(body: &[u8], cuts: &[Index]) -> Vec<Vec<u8>> {
  let mut ends = cuts.iter().map(|cut| cut.index(body.len() + 1)).collect::<Vec<_>>();
  ends.sort_unstable();
  ends.push(body.len());
  let mut start = 0;
  ends
    .into_iter()
    .map(|end| {
      let chunk = body[start..end].to_vec();
      start = end;
      chunk
    })
    .collect()
}

proptest! {
  #![proptest_config(property_config(256))]

  #[test]
  fn any_event_stream_in_any_chunks_reads_as_the_whatwg_rules_say(
    body in prop_oneof![wire_body(), written_body()],
    cuts in collection::vec(any::<Index>(), 0..8),
  ) {
    let expected = reference(&body);
    prop_assert_eq!(&read(vec![body.clone()])?, &expected);
    prop_assert_eq!(read(split(&body, &cuts))?, expected);
  }
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
struct Reading {
  id: String,
  value: i64,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Kind {
  Add,
}

fn reading(id: &str, value: i64) -> Reading {
  Reading {
    id: id.to_string(),
    value,
  }
}

type Decode<Data, Event> = fn(reqwest::Response, Option<&'static str>) -> EventStream<SseFrame<Data, Event>>;

#[rstest]
#[case::json_data_with_an_optional_event_name(
  EventStream::json,
  &["event: add\nid: 7\nretry: 2", "50\r\ndata: {\"id\":\"a\",\r\ndata: \"value\":1}\r\n\r", "\ndata: {\"id\":\"b\",\"value\":2}\n\n"],
  vec![
    (Some(Kind::Add), reading("a", 1), Some("7"), Some(250)),
    (None, reading("b", 2), Some("7"), Some(250)),
  ]
)]
#[case::text_fields_read_as_numbers(EventStream::text, &["event: 7\ndata: -42\n\n"], vec![(7_u16, -42_i64, None, None)])]
fn frames_decode_their_data_and_event_as_the_stream_declares<Data: Debug + PartialEq, Event: Debug + PartialEq>(
  #[case] decode: Decode<Data, Event>,
  #[case] chunks: &[&str],
  #[case] expected: Vec<(Event, Data, Option<&str>, Option<u64>)>,
) -> anyhow::Result<()> {
  let chunks = chunks.iter().map(|chunk| chunk.as_bytes().to_vec()).collect();

  let frames = block_on(decode(response(chunks), None).collect::<Vec<_>>())
    .into_iter()
    .collect::<Result<Vec<_>, _>>()?;

  let read = frames
    .iter()
    .map(|frame| (frame.event(), frame.data(), frame.id(), millis(frame.retry())))
    .collect::<Vec<_>>();
  let expected = expected
    .iter()
    .map(|(event, data, id, retry)| (event, data, *id, *retry))
    .collect::<Vec<_>>();
  assert_eq!(read, expected);
  Ok(())
}

#[rstest]
#[case::sequence_element("data: [1]\n\n", "data[0]")]
#[case::not_json("data: DONE\n\n", "data")]
fn decode_errors_name_the_path_under_data(#[case] body: &str, #[case] path: &str) {
  let items = block_on(
    EventStream::<SseFrame<Reading>>::json(response(vec![body.as_bytes().to_vec()]), None).collect::<Vec<_>>(),
  );
  let [Err(DiagnosticsError::DeserializationError { path: error_path, .. })] = items.as_slice() else {
    panic!("expected one decode error for {body:?}, got {items:?}");
  };
  assert_eq!(error_path, path);
}

#[rstest]
fn a_sentinel_ends_the_stream_for_good_and_frames_without_data_still_set_where_it_resumes() {
  let body = "id: 1\ndata: {\"id\":\"a\",\"value\":1}\n\nid: 2\n\nretry: 9000\n\ndata: [DONE]\n\ndata: {\"id\":\"b\",\"value\":2}\n\n";
  let mut stream = EventStream::<SseFrame<Reading>>::json(response(vec![body.as_bytes().to_vec()]), Some("[DONE]"));
  assert_eq!((stream.last_event_id(), stream.retry()), (None, None));

  let first = block_on(stream.next());

  assert!(
    matches!(&first, Some(Ok(frame)) if frame.data() == &reading("a", 1) && frame.id() == Some("1")),
    "{first:?}"
  );
  assert!(block_on(stream.next()).is_none());
  assert!(block_on(stream.next()).is_none());
  assert_eq!(stream.last_event_id().as_deref(), Some("2"));
  assert_eq!(stream.retry(), Some(Duration::from_millis(9000)));
}

#[rstest]
fn a_body_error_ends_the_stream_after_the_frames_before_it() {
  let interrupted = |chunks: &[&'static [u8]]| {
    let mut body = chunks
      .iter()
      .map(|chunk| Ok(Bytes::from_static(chunk)))
      .collect::<Vec<_>>();
    body.insert(chunks.len() - 1, Err(std::io::Error::other("connection reset")));
    block_on(EventStream::<SseFrame<Reading>>::json(body_response(body), None).collect::<Vec<_>>())
  };

  let whole = interrupted(&[
    b"data: {\"id\":\"a\",\"value\":1}\n\n",
    b"data: {\"id\":\"b\",\"value\":2}\n\n",
  ]);
  let cut = interrupted(&[b"data: {\"id\":\"a\",", b"\"value\":1}\n\n"]);

  assert!(
    matches!(whole.as_slice(), [Ok(_), Err(DiagnosticsError::BodyReadError(_))]),
    "{whole:?}"
  );
  assert!(
    matches!(cut.as_slice(), [Err(DiagnosticsError::BodyReadError(_))]),
    "{cut:?}"
  );
}

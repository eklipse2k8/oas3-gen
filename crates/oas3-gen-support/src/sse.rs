//! Server-Sent Events: the `text/event-stream` bodies a generated client reads.
//!
//! [`EventStream`] reads a response body with a parser that follows the
//! [WHATWG event stream interpretation] rules, and decodes each frame it dispatches into a
//! typed [`SseFrame`], or into a [`FromSseFrame`] union of frame types.
//!
//! [WHATWG event stream interpretation]: https://html.spec.whatwg.org/multipage/server-sent-events.html#event-stream-interpretation

use std::{
  fmt,
  pin::Pin,
  sync::{Arc, Mutex, PoisonError},
  task::{Context, Poll},
  time::Duration,
};

use bytes::Bytes;
use futures_core::Stream;
use futures_util::StreamExt as _;
use serde::de::{self, DeserializeOwned, IntoDeserializer as _, Visitor};

use crate::{DiagnosticsError, UTF8_BOM, deserialize_json};

/// The event name of a frame that doesn't set one.
const MESSAGE_EVENT: &str = "message";

/// One Server-Sent Events frame: its `data` and `event` name decoded as the types the
/// stream declares, with the last event ID and reconnection time the stream set.
///
/// Only an [`EventStream`] builds frames, and a frame never changes once read.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SseFrame<Data, Event = String> {
  event: Event,
  data: Data,
  id: Option<Arc<str>>,
  retry: Option<Duration>,
}

impl<Data, Event> SseFrame<Data, Event> {
  /// The event name. A frame without an `event` field is a `message`, and a `message`
  /// is `None` when `Event` is an `Option`.
  #[must_use]
  pub fn event(&self) -> &Event {
    &self.event
  }

  /// The data.
  #[must_use]
  pub fn data(&self) -> &Data {
    &self.data
  }

  /// The last event ID the stream set, which resumes it when sent back as `Last-Event-ID`.
  #[must_use]
  pub fn id(&self) -> Option<&str> {
    self.id.as_deref()
  }

  /// The reconnection time the stream last asked for.
  #[must_use]
  pub fn retry(&self) -> Option<Duration> {
    self.retry
  }

  /// The data, without the rest of the frame.
  #[must_use]
  pub fn into_data(self) -> Data {
    self.data
  }
}

/// A frame as the stream read it, before its `data` and `event` decode.
///
/// A [`FromSseFrame`] type picks how to decode a frame from its event name, then decodes
/// it with [`RawSseFrame::json`] or [`RawSseFrame::text`].
#[derive(Debug)]
pub struct RawSseFrame {
  event: String,
  data: String,
  id: Option<Arc<str>>,
  retry: Option<Duration>,
}

impl RawSseFrame {
  /// The event name: `message` for a frame that doesn't set one.
  #[must_use]
  pub fn event(&self) -> &str {
    if self.event.is_empty() {
      MESSAGE_EVENT
    } else {
      &self.event
    }
  }

  /// The data, its lines joined with newlines.
  #[must_use]
  pub fn data(&self) -> &str {
    &self.data
  }

  /// Decodes the frame, reading `data` as JSON.
  ///
  /// # Errors
  ///
  /// Returns an error naming `event`, or the path under `data`, that doesn't decode.
  pub fn json<Data: DeserializeOwned, Event: DeserializeOwned>(
    &self,
  ) -> Result<SseFrame<Data, Event>, DiagnosticsError> {
    let event = self.decode_event()?;
    let data = deserialize_json(self.data.as_bytes(), |path, inner| {
      DiagnosticsError::DeserializationError {
        path: match path.as_str() {
          "." => "data".to_string(),
          index
            if index
              .strip_prefix('[')
              .is_some_and(|rest| rest.starts_with(|c: char| c.is_ascii_digit())) =>
          {
            format!("data{index}")
          }
          field => format!("data.{field}"),
        },
        inner,
      }
    })?;
    Ok(self.with(event, data))
  }

  /// Decodes the frame, reading `data` as text into a type such as `String` or a number.
  ///
  /// # Errors
  ///
  /// Returns an error naming `event` or `data` when it doesn't decode.
  pub fn text<Data: DeserializeOwned, Event: DeserializeOwned>(
    &self,
  ) -> Result<SseFrame<Data, Event>, DiagnosticsError> {
    let event = self.decode_event()?;
    let data = Data::deserialize(FieldText::data(&self.data)).map_err(|inner| field_error("data", inner))?;
    Ok(self.with(event, data))
  }

  /// The error for a frame whose event name isn't one of `expected`.
  #[must_use]
  pub fn unknown_event(&self, expected: &'static [&'static str]) -> DiagnosticsError {
    field_error("event", de::Error::unknown_variant(self.event(), expected))
  }

  fn decode_event<Event: DeserializeOwned>(&self) -> Result<Event, DiagnosticsError> {
    Event::deserialize(FieldText::event(self.event())).map_err(|inner| field_error("event", inner))
  }

  fn with<Data, Event>(&self, event: Event, data: Data) -> SseFrame<Data, Event> {
    SseFrame {
      event,
      data,
      id: self.id.clone(),
      retry: self.retry,
    }
  }
}

fn field_error(path: &str, inner: serde_json::Error) -> DiagnosticsError {
  DiagnosticsError::DeserializationError {
    path: path.to_string(),
    inner,
  }
}

/// A type a generated client decodes from each frame of a stream, such as a union of frame
/// types told apart by their event names.
pub trait FromSseFrame: Sized {
  /// Decodes `frame`.
  ///
  /// # Errors
  ///
  /// Returns an error when the frame's event name or data doesn't decode as `Self`.
  fn from_sse_frame(frame: &RawSseFrame) -> Result<Self, DiagnosticsError>;
}

/// A `text/event-stream` response body, read as a stream of decoded frames.
///
/// Each frame the body dispatches decodes into one item, and an item that doesn't decode
/// is an `Err` the stream continues past. The stream ends with the body, after an error
/// reading it, or at the frame whose data is the sentinel, which isn't decoded. Once it
/// ends, it only yields `None`.
///
/// ```ignore
/// use futures::StreamExt as _;
///
/// let mut frames = EventStream::<SseFrame<Reading>>::json(response, None);
/// while let Some(frame) = frames.next().await {
///   println!("{:?}", frame?.data());
/// }
/// ```
pub struct EventStream<T> {
  items: Pin<Box<dyn Stream<Item = Result<T, DiagnosticsError>> + Send>>,
  resume: Arc<Mutex<Resume>>,
}

impl<T> fmt::Debug for EventStream<T> {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    f.debug_struct("EventStream").finish_non_exhaustive()
  }
}

impl<Data, Event> EventStream<SseFrame<Data, Event>>
where
  Data: DeserializeOwned + 'static,
  Event: DeserializeOwned + 'static,
{
  /// Reads `response`, decoding each frame's `data` as JSON, until the frame whose data
  /// is `sentinel`.
  #[must_use]
  pub fn json(response: reqwest::Response, sentinel: Option<&'static str>) -> Self {
    Self::read(response, RawSseFrame::json, sentinel)
  }

  /// Reads `response`, decoding each frame's `data` as text, until the frame whose data
  /// is `sentinel`.
  #[must_use]
  pub fn text(response: reqwest::Response, sentinel: Option<&'static str>) -> Self {
    Self::read(response, RawSseFrame::text, sentinel)
  }
}

impl<T: FromSseFrame + 'static> EventStream<T> {
  /// Reads `response`, decoding each frame as `T`, until the frame whose data is `sentinel`.
  #[must_use]
  pub fn frames(response: reqwest::Response, sentinel: Option<&'static str>) -> Self {
    Self::read(response, T::from_sse_frame, sentinel)
  }
}

impl<T: 'static> EventStream<T> {
  fn read(response: reqwest::Response, decode: Decoder<T>, sentinel: Option<&'static str>) -> Self {
    let parser = Parser::default();
    let resume = Arc::clone(&parser.frame.resume);
    let reader = Reader {
      body: Some(response),
      parser,
      decode,
      sentinel,
    };
    let items = futures_util::stream::unfold(reader, async |mut reader| {
      let item = reader.next().await?;
      Some((item, reader))
    });
    Self {
      items: Box::pin(items.fuse()),
      resume,
    }
  }
}

impl<T> EventStream<T> {
  /// The last event ID the stream set, including in a frame without data, which resumes
  /// the stream when sent back as `Last-Event-ID`.
  #[must_use]
  pub fn last_event_id(&self) -> Option<Arc<str>> {
    self.resume().id
  }

  /// The reconnection time the stream last asked for, including in a frame without data.
  #[must_use]
  pub fn retry(&self) -> Option<Duration> {
    self.resume().retry
  }

  fn resume(&self) -> Resume {
    self.resume.lock().unwrap_or_else(PoisonError::into_inner).clone()
  }
}

impl<T> Stream for EventStream<T> {
  type Item = Result<T, DiagnosticsError>;

  fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
    self.items.as_mut().poll_next(cx)
  }
}

type Decoder<T> = fn(&RawSseFrame) -> Result<T, DiagnosticsError>;

/// The state an [`EventStream`] reads with: the body still to read, and the frame it is in.
struct Reader<T> {
  body: Option<reqwest::Response>,
  parser: Parser,
  decode: Decoder<T>,
  sentinel: Option<&'static str>,
}

impl<T> Reader<T> {
  /// The next item, or `None` once the stream has ended.
  async fn next(&mut self) -> Option<Result<T, DiagnosticsError>> {
    loop {
      let body = self.body.as_mut()?;
      if let Some(frame) = self.parser.next_frame() {
        if self.sentinel == Some(frame.data()) {
          return None;
        }
        return Some((self.decode)(frame));
      }
      match body.chunk().await {
        Ok(Some(chunk)) => self.parser.feed(chunk),
        Ok(None) => return None,
        Err(error) => {
          self.body = None;
          return Some(Err(error.into()));
        }
      }
    }
  }
}

/// Splits a body into frames, chunk by chunk.
///
/// Frames are dispatched in place: [`Parser::next_frame`] lends the frame it finished, and
/// the next call reuses its buffers.
#[derive(Debug, Default)]
struct Parser {
  lines: Lines,
  frame: FrameBuffer,
}

impl Parser {
  fn feed(&mut self, chunk: Bytes) {
    self.lines.feed(chunk);
  }

  /// The next frame the chunks fed so far complete.
  fn next_frame(&mut self) -> Option<&RawSseFrame> {
    self.frame.reclaim();
    while let Some(line) = self.lines.next_line() {
      if self.frame.process(line) {
        return Some(&self.frame.raw);
      }
    }
    None
  }
}

/// The lines of a body: the chunk being read and the start of a line it cut off.
#[derive(Debug, Default)]
struct Lines {
  chunk: Bytes,
  position: usize,
  partial: Vec<u8>,
  partial_read: bool,
  after_cr: bool,
  started: bool,
}

impl Lines {
  fn feed(&mut self, chunk: Bytes) {
    self.chunk = chunk;
    self.position = 0;
    if !self.started {
      self.skip_byte_order_mark();
    }
  }

  /// Drops the byte order mark at the start of the body. The bytes that match it so far
  /// wait in `partial`, where they begin the first line if the mark turns out not to be one.
  fn skip_byte_order_mark(&mut self) {
    while let Some(&expected) = UTF8_BOM.get(self.partial.len()) {
      let Some(&byte) = self.chunk.get(self.position) else {
        return;
      };
      if byte != expected {
        self.started = true;
        return;
      }
      self.partial.push(byte);
      self.position += 1;
    }
    self.partial.clear();
    self.started = true;
  }

  /// The next whole line, without its CR, LF, or CRLF ending.
  fn next_line(&mut self) -> Option<&[u8]> {
    if std::mem::take(&mut self.partial_read) {
      self.partial.clear();
    }
    if self.after_cr
      && let Some(&byte) = self.chunk.get(self.position)
    {
      self.after_cr = false;
      if byte == b'\n' {
        self.position += 1;
      }
    }
    let start = self.position;
    let rest = self.chunk.get(start..).unwrap_or_default();
    let Some(length) = memchr::memchr2(b'\n', b'\r', rest) else {
      self.partial.extend_from_slice(rest);
      self.position = self.chunk.len();
      return None;
    };
    let end = start + length;
    self.position = end + 1;
    self.after_cr = self.chunk[end] == b'\r';
    if self.partial.is_empty() {
      return Some(&self.chunk[start..end]);
    }
    self.partial.extend_from_slice(&self.chunk[start..end]);
    self.partial_read = true;
    Some(&self.partial)
  }
}

/// The fields of the frame being read, and the last event ID and reconnection time, which
/// carry over from frame to frame and which the [`EventStream`] shares.
#[derive(Debug)]
struct FrameBuffer {
  raw: RawSseFrame,
  data: Vec<u8>,
  dispatched: bool,
  resume: Arc<Mutex<Resume>>,
}

/// Where a stream resumes from: the last event ID and reconnection time it set.
#[derive(Debug, Clone, Default)]
struct Resume {
  id: Option<Arc<str>>,
  retry: Option<Duration>,
}

impl Default for FrameBuffer {
  fn default() -> Self {
    Self {
      raw: RawSseFrame {
        event: String::new(),
        data: String::new(),
        id: None,
        retry: None,
      },
      data: vec![],
      dispatched: false,
      resume: Arc::default(),
    }
  }
}

impl FrameBuffer {
  /// Clears the frame [`Parser::next_frame`] lent out, keeping its buffers to read the next.
  fn reclaim(&mut self) {
    if std::mem::take(&mut self.dispatched) {
      self.data = std::mem::take(&mut self.raw.data).into_bytes();
      self.data.clear();
      self.raw.event.clear();
    }
  }

  /// Applies one line, returning whether it dispatched the frame.
  fn process(&mut self, line: &[u8]) -> bool {
    if line.is_empty() {
      return self.dispatch();
    }
    let (name, value) = match memchr::memchr(b':', line) {
      Some(0) => return false,
      Some(colon) => {
        let value = &line[colon + 1..];
        (&line[..colon], value.strip_prefix(b" ").unwrap_or(value))
      }
      None => (line, &[][..]),
    };
    match name {
      b"data" => {
        self.data.extend_from_slice(value);
        self.data.push(b'\n');
      }
      b"event" => {
        self.raw.event.clear();
        self.raw.event.push_str(&String::from_utf8_lossy(value));
      }
      b"id" if memchr::memchr(0, value).is_none() => {
        self.raw.id = (!value.is_empty()).then(|| Arc::from(String::from_utf8_lossy(value)));
        self.share_resume();
      }
      b"retry" => {
        if let Some(millis) = retry_millis(value) {
          self.raw.retry = Some(Duration::from_millis(millis));
          self.share_resume();
        }
      }
      _ => {}
    }
    false
  }

  fn share_resume(&self) {
    *self.resume.lock().unwrap_or_else(PoisonError::into_inner) = Resume {
      id: self.raw.id.clone(),
      retry: self.raw.retry,
    };
  }

  /// Ends the frame at a blank line. A frame without data isn't dispatched.
  fn dispatch(&mut self) -> bool {
    if self.data.pop().is_none() {
      self.raw.event.clear();
      return false;
    }
    let data = std::mem::take(&mut self.data);
    self.raw.data =
      String::from_utf8(data).unwrap_or_else(|error| String::from_utf8_lossy(error.as_bytes()).into_owned());
    self.dispatched = true;
    true
  }
}

/// A `retry` value: ASCII digits only, and small enough to count milliseconds in a `u64`.
fn retry_millis(value: &[u8]) -> Option<u64> {
  if !value.iter().all(u8::is_ascii_digit) {
    return None;
  }
  std::str::from_utf8(value).ok()?.parse().ok()
}

/// A frame field read as text: `data`, or the event name, which reads as `None` into an
/// `Option` when it's `message`. Text a number or `bool` parses as reads as one.
struct FieldText<'a> {
  text: &'a str,
  none: bool,
}

impl<'a> FieldText<'a> {
  fn data(text: &'a str) -> Self {
    Self { text, none: false }
  }

  fn event(name: &'a str) -> Self {
    Self {
      text: name,
      none: name == MESSAGE_EVENT,
    }
  }
}

macro_rules! parse_text {
  ($($method:ident: $parsed:ty => $visit:ident),* $(,)?) => {
    $(
      fn $method<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        match self.text.parse::<$parsed>() {
          Ok(value) => visitor.$visit(value),
          Err(_) => visitor.visit_str(self.text),
        }
      }
    )*
  };
}

impl<'de> de::Deserializer<'de> for FieldText<'_> {
  type Error = serde_json::Error;

  fn deserialize_any<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
    visitor.visit_str(self.text)
  }

  fn deserialize_option<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
    if self.none {
      visitor.visit_none()
    } else {
      visitor.visit_some(self)
    }
  }

  parse_text! {
    deserialize_bool: bool => visit_bool,
    deserialize_i8: i8 => visit_i8,
    deserialize_i16: i16 => visit_i16,
    deserialize_i32: i32 => visit_i32,
    deserialize_i64: i64 => visit_i64,
    deserialize_i128: i128 => visit_i128,
    deserialize_u8: u8 => visit_u8,
    deserialize_u16: u16 => visit_u16,
    deserialize_u32: u32 => visit_u32,
    deserialize_u64: u64 => visit_u64,
    deserialize_u128: u128 => visit_u128,
    deserialize_f32: f32 => visit_f32,
    deserialize_f64: f64 => visit_f64,
  }

  fn deserialize_newtype_struct<V: Visitor<'de>>(
    self,
    _name: &'static str,
    visitor: V,
  ) -> Result<V::Value, Self::Error> {
    visitor.visit_newtype_struct(self)
  }

  fn deserialize_enum<V: Visitor<'de>>(
    self,
    _name: &'static str,
    _variants: &'static [&'static str],
    visitor: V,
  ) -> Result<V::Value, Self::Error> {
    visitor.visit_enum(self.text.into_deserializer())
  }

  serde::forward_to_deserialize_any! {
    char str string bytes byte_buf unit unit_struct seq tuple tuple_struct map struct identifier ignored_any
  }
}

#[cfg(test)]
mod tests {
  use std::time::Duration;

  use bytes::Bytes;
  use futures::{StreamExt as _, executor::block_on};
  use proptest::prelude::*;
  use rstest::rstest;
  use serde::Deserialize;

  use super::{EventStream, FromSseFrame, Parser, RawSseFrame, SseFrame};
  use crate::DiagnosticsError;

  /// One frame as a test expects it.
  #[derive(Debug, Clone, Default, PartialEq, Eq)]
  struct Expected {
    event: Option<String>,
    data: String,
    id: Option<String>,
    retry: Option<u64>,
  }

  fn data(data: &str) -> Expected {
    Expected {
      data: data.to_string(),
      ..Expected::default()
    }
  }

  impl Expected {
    fn event(self, event: &str) -> Self {
      Self {
        event: Some(event.to_string()),
        ..self
      }
    }

    fn id(self, id: &str) -> Self {
      Self {
        id: Some(id.to_string()),
        ..self
      }
    }

    fn retry(self, millis: u64) -> Self {
      Self {
        retry: Some(millis),
        ..self
      }
    }
  }

  /// The frames a parser reads from `chunks`, fed in order.
  fn parse<'a>(chunks: impl IntoIterator<Item = &'a [u8]>) -> Vec<Expected> {
    let mut parser = Parser::default();
    let mut frames = vec![];
    for chunk in chunks {
      parser.feed(Bytes::copy_from_slice(chunk));
      while let Some(frame) = parser.next_frame() {
        frames.push(Expected {
          event: Some(frame.event().to_string()).filter(|event| event != "message"),
          data: frame.data().to_string(),
          id: frame.id.as_deref().map(str::to_string),
          retry: frame
            .retry
            .map(|retry| u64::try_from(retry.as_millis()).expect("retry fits in u64")),
        });
      }
    }
    frames
  }

  #[rstest]
  #[case::lf(b"data: a\n\n", vec![data("a")])]
  #[case::crlf(b"data: a\r\n\r\n", vec![data("a")])]
  #[case::cr(b"data: a\r\r", vec![data("a")])]
  #[case::mixed_line_endings(b"data: a\rdata: b\r\ndata: c\n\n", vec![data("a\nb\nc")])]
  #[case::one_leading_space_stripped(b"data:  a\n\n", vec![data(" a")])]
  #[case::no_leading_space(b"data:a\n\n", vec![data("a")])]
  #[case::colon_in_value(b"data: a: b\n\n", vec![data("a: b")])]
  #[case::field_without_colon(b"data\n\n", vec![data("")])]
  #[case::empty_data_lines(b"data:\ndata:\n\n", vec![data("\n")])]
  #[case::comment(b": keep-alive\n\ndata: a\n:\n\n", vec![data("a")])]
  #[case::unknown_fields(b"foo: bar\nData: x\ndata : y\ndata: a\n\n", vec![data("a")])]
  #[case::event_name(b"event: add\ndata: a\n\n", vec![data("a").event("add")])]
  #[case::empty_event_name(b"event:\ndata: a\n\n", vec![data("a")])]
  #[case::explicit_message_event(b"event: message\ndata: a\n\n", vec![data("a")])]
  #[case::last_event_name_wins(b"event: add\nevent: remove\ndata: a\n\n", vec![data("a").event("remove")])]
  #[case::dataless_frame_not_dispatched(b"event: lost\nid: 1\n\ndata: a\n\n", vec![data("a").id("1")])]
  #[case::event_name_resets(b"event: add\ndata: a\n\ndata: b\n\n", vec![data("a").event("add"), data("b")])]
  #[case::id_carries_over(b"id: 1\ndata: a\n\ndata: b\n\n", vec![data("a").id("1"), data("b").id("1")])]
  #[case::empty_id_resets(b"id: 1\ndata: a\n\nid\ndata: b\n\n", vec![data("a").id("1"), data("b")])]
  #[case::id_with_nul_ignored(b"id: 1\ndata: a\n\nid: 2\0\ndata: b\n\n", vec![data("a").id("1"), data("b").id("1")])]
  #[case::retry_carries_over(b"retry: 1500\ndata: a\n\ndata: b\n\n", vec![data("a").retry(1500), data("b").retry(1500)])]
  #[case::retry_needs_digits(b"retry: 15s\ndata: a\n\nretry:\nretry: -1\nretry: 1 5\ndata: b\n\n", vec![data("a"), data("b")])]
  #[case::retry_overflow_ignored(b"retry: 99999999999999999999\ndata: a\n\n", vec![data("a")])]
  #[case::byte_order_mark(b"\xEF\xBB\xBFdata: a\n\n", vec![data("a")])]
  #[case::byte_order_mark_only_at_start(b"data: \xEF\xBB\xBFa\n\n", vec![data("\u{FEFF}a")])]
  #[case::partial_byte_order_mark(b"\xEF\xBBdata: a\n\ndata: b\n\n", vec![data("b")])]
  #[case::invalid_utf8_replaced(b"data: \xFFa\xC3\nevent: \xFE\n\n", vec![data("\u{FFFD}a\u{FFFD}").event("\u{FFFD}")])]
  #[case::unterminated_frame_dropped(b"data: a\n\ndata: b\n", vec![data("a")])]
  #[case::unterminated_line_dropped(b"data: a", vec![])]
  #[case::empty_body(b"", vec![])]
  fn frames_follow_the_event_stream_rules(#[case] body: &[u8], #[case] expected: Vec<Expected>) {
    let text = String::from_utf8_lossy(body);
    assert_eq!(parse([body]), expected, "frames read from {text:?}");
    assert_eq!(
      parse(body.chunks(1)),
      expected,
      "frames read byte by byte from {text:?}"
    );
  }

  /// The lines of an event stream: fields known and unknown, comments, and blank lines,
  /// each with any line ending, whose values may hold any bytes, line breaks included.
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
      "[0-9]{1,5}".prop_map(String::into_bytes),
      "\\PC{0,3}".prop_map(String::into_bytes),
      prop::collection::vec(any::<u8>(), 0..4),
    ];
    let ending = prop_oneof![Just(&b"\n"[..]), Just(&b"\r"[..]), Just(&b"\r\n"[..])];
    let line = prop_oneof![
      4 => (name, separator, value.clone(), ending.clone()).prop_map(|(name, separator, value, ending)| {
        [name.as_bytes(), separator, &value, ending].concat()
      }),
      2 => ending.clone().prop_map(<[u8]>::to_vec),
      1 => (value, ending).prop_map(|(value, ending)| [b":", value.as_slice(), ending].concat()),
    ];
    (any::<bool>(), prop::collection::vec(line, 0..32)).prop_map(|(byte_order_mark, lines)| {
      let start = if byte_order_mark { crate::UTF8_BOM } else { b"" };
      [start.to_vec(), lines.concat()].concat()
    })
  }

  /// The frames the WHATWG algorithm dispatches for `body`, transcribed from the spec as
  /// directly as possible: decode the whole body, drop a leading BOM, then split it into
  /// lines at CRLF, LF, or CR and process each.
  fn reference(body: &[u8]) -> Vec<Expected> {
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
          frames.push(Expected {
            event: Some(event.clone()).filter(|event| !event.is_empty() && event != "message"),
            data: std::mem::take(&mut data),
            id: id.clone(),
            retry,
          });
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

  /// `body` cut at `cuts`, each an index into it.
  fn split(body: &[u8], cuts: &[prop::sample::Index]) -> Vec<Vec<u8>> {
    let mut at = cuts.iter().map(|cut| cut.index(body.len() + 1)).collect::<Vec<_>>();
    at.sort_unstable();
    at.push(body.len());
    let mut start = 0;
    at.into_iter()
      .map(|end| {
        let chunk = body[start..end].to_vec();
        start = end;
        chunk
      })
      .collect()
  }

  /// A frame to write: its event name, data lines, id, and retry.
  type Written = (Option<String>, Vec<String>, Option<String>, Option<u32>);

  fn written_frame() -> impl Strategy<Value = Written> {
    (
      prop::option::of("[^\r\n]{0,8}"),
      prop::collection::vec("[^\r\n]{0,16}", 1..4),
      prop::option::of("[^\r\n\0]{0,8}"),
      prop::option::of(any::<u32>()),
    )
  }

  /// `frames` written as a body with `line_ending`, and the frames a reader must read back.
  fn write(frames: &[Written], line_ending: &str, byte_order_mark: bool) -> (Vec<u8>, Vec<Expected>) {
    let mut body = if byte_order_mark {
      "\u{FEFF}".to_string()
    } else {
      String::new()
    };
    let mut expected = vec![];
    let (mut id, mut retry) = (None, None);
    for (event, lines, frame_id, frame_retry) in frames {
      let mut field = |name: &str, value: &str| body.extend([name, ": ", value, line_ending]);
      field("", "comment");
      if let Some(event) = event {
        field("event", event);
      }
      for line in lines {
        field("data", line);
      }
      if let Some(frame_id) = frame_id {
        field("id", frame_id);
        id = (!frame_id.is_empty()).then(|| frame_id.clone());
      }
      if let Some(frame_retry) = frame_retry {
        field("retry", &frame_retry.to_string());
        retry = Some(u64::from(*frame_retry));
      }
      body.push_str(line_ending);
      expected.push(Expected {
        event: event.clone().filter(|event| !event.is_empty() && event != "message"),
        data: lines.join("\n"),
        id: id.clone(),
        retry,
      });
    }
    (body.into_bytes(), expected)
  }

  proptest! {
    #[test]
    fn any_body_in_any_chunks_reads_as_the_spec_says(
      body in wire_body(),
      cuts in prop::collection::vec(any::<prop::sample::Index>(), 0..8),
    ) {
      let expected = reference(&body);
      prop_assert_eq!(&parse([body.as_slice()]), &expected);
      let chunks = split(&body, &cuts);
      prop_assert_eq!(parse(chunks.iter().map(Vec::as_slice)), expected);
    }

    #[test]
    fn written_frames_read_back(
      frames in prop::collection::vec(written_frame(), 0..8),
      line_ending in prop_oneof![Just("\n"), Just("\r"), Just("\r\n")],
      byte_order_mark in any::<bool>(),
      cuts in prop::collection::vec(any::<prop::sample::Index>(), 0..8),
    ) {
      let (body, expected) = write(&frames, line_ending, byte_order_mark);
      prop_assert_eq!(&parse([body.as_slice()]), &expected);
      let chunks = split(&body, &cuts);
      prop_assert_eq!(parse(chunks.iter().map(Vec::as_slice)), expected);
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
    Remove,
  }

  fn reading(id: &str, value: i64) -> Reading {
    Reading {
      id: id.to_string(),
      value,
    }
  }

  fn response(chunks: &[&'static str]) -> reqwest::Response {
    body_response(
      chunks
        .iter()
        .map(|chunk| Ok(Bytes::from_static(chunk.as_bytes())))
        .collect(),
    )
  }

  fn body_response(chunks: Vec<std::io::Result<Bytes>>) -> reqwest::Response {
    reqwest::Response::from(http::Response::new(reqwest::Body::wrap_stream(futures::stream::iter(
      chunks,
    ))))
  }

  fn collect<T>(stream: EventStream<T>) -> Vec<Result<T, DiagnosticsError>> {
    block_on(stream.collect())
  }

  fn frame<Data, Event>(event: Event, data: Data, id: Option<&str>, retry: Option<u64>) -> SseFrame<Data, Event> {
    SseFrame {
      event,
      data,
      id: id.map(Into::into),
      retry: retry.map(Duration::from_millis),
    }
  }

  #[rstest]
  #[case::whole(&["event: add\nid: 7\nretry: 250\ndata: {\"id\":\"a\",\n", "data: \"value\":1}\n\ndata: {\"id\":\"b\",\"value\":2}\n\n"])]
  #[case::split_mid_line(&["event: ad", "d\r", "\nid: 7\r\nretry: 2", "50\r\ndata: {\"id\":\"a\",\r\ndata: \"value\":1}\r\n\r", "\ndata: {\"id\":\"b\",\"value\":2}\r\n\r\n"])]
  fn json_frames_decode_their_data_and_event(#[case] chunks: &[&'static str]) {
    let frames = collect(EventStream::<SseFrame<Reading, Option<Kind>>>::json(
      response(chunks),
      None,
    ))
    .into_iter()
    .collect::<Result<Vec<_>, _>>()
    .expect("every frame decodes");

    assert_eq!(
      frames,
      [
        frame(Some(Kind::Add), reading("a", 1), Some("7"), Some(250)),
        frame(None, reading("b", 2), Some("7"), Some(250)),
      ],
      "an unnamed event is None, and the id and retry carry over"
    );
    assert_eq!(frames[0].data().value, 1);
    assert_eq!(frames[1].event(), &None);
    assert_eq!(frames[1].id(), Some("7"));
    assert_eq!(frames[1].retry(), Some(Duration::from_millis(250)));
    assert_eq!(frames[1].clone().into_data(), reading("b", 2));
  }

  #[test]
  fn text_frames_keep_their_data_as_written() {
    let frames = collect(EventStream::<SseFrame<String>>::text(
      response(&["data: \"quoted\"\ndata:  two\n\nevent: token\ndata: [1, 2]\n\n"]),
      None,
    ));
    let frames = frames
      .into_iter()
      .collect::<Result<Vec<_>, _>>()
      .expect("text always decodes");
    assert_eq!(
      frames,
      [
        frame("message".to_string(), "\"quoted\"\n two".to_string(), None, None),
        frame("token".to_string(), "[1, 2]".to_string(), None, None),
      ],
      "a frame without an event name is a message"
    );
  }

  #[rstest]
  #[case::nested_field("event: add\ndata: {\"id\":\"a\",\"value\":\"one\"}\n\n", "data.value")]
  #[case::missing_field("event: add\ndata: {\"id\":\"a\"}\n\n", "data")]
  #[case::wrong_shape("event: add\ndata: \"a\"\n\n", "data")]
  #[case::sequence_element("event: add\ndata: [1]\n\n", "data[0]")]
  #[case::not_json("event: add\ndata: DONE\n\n", "data")]
  #[case::unknown_event("event: grow\ndata: {\"id\":\"a\",\"value\":1}\n\n", "event")]
  #[case::required_event_missing("data: {\"id\":\"a\",\"value\":1}\n\n", "event")]
  fn decode_errors_name_the_field_that_failed(#[case] body: &'static str, #[case] path: &str) {
    let items = collect(EventStream::<SseFrame<Reading, Kind>>::json(response(&[body]), None));
    let [Err(DiagnosticsError::DeserializationError { path: error_path, .. })] = items.as_slice() else {
      panic!("expected one decode error for {body:?}, got {items:?}");
    };
    assert_eq!(error_path, path, "the error for {body:?}");
  }

  #[rstest]
  #[case::ends_at_sentinel(Some("[DONE]"), 1)]
  #[case::sentinel_absent(Some("[END]"), 3)]
  #[case::no_sentinel(None, 3)]
  fn a_sentinel_ends_the_stream_undecoded(#[case] sentinel: Option<&'static str>, #[case] items: usize) {
    let body = "data: {\"id\":\"a\",\"value\":1}\n\ndata: [DONE]\n\ndata: {\"id\":\"b\",\"value\":2}\n\n";
    let read = collect(EventStream::<SseFrame<Reading>>::json(response(&[body]), sentinel));
    assert_eq!(read.len(), items, "items read with sentinel {sentinel:?}: {read:?}");
    assert!(read[0].is_ok(), "the first frame decodes");
    if items == 3 {
      assert!(
        read[1].is_err() && read[2].is_ok(),
        "a frame that doesn't decode doesn't end the stream"
      );
    }
  }

  #[derive(Debug, PartialEq)]
  enum Update {
    Token(SseFrame<String>),
    Usage(SseFrame<Reading>),
  }

  impl FromSseFrame for Update {
    fn from_sse_frame(frame: &RawSseFrame) -> Result<Self, DiagnosticsError> {
      match frame.event() {
        "token" => frame.text().map(Self::Token),
        "usage" | "message" => frame.json().map(Self::Usage),
        _ => Err(frame.unknown_event(&["token", "usage", "message"])),
      }
    }
  }

  #[test]
  fn unions_pick_their_member_by_event_name() {
    let body = "event: token\ndata: hi\n\nevent: mystery\ndata: ?\n\nevent: usage\ndata: {\"id\":\"u\",\"value\":3}\n\ndata: {\"id\":\"m\",\"value\":4}\n\n";
    let items = collect(EventStream::<Update>::frames(response(&[body]), None));

    let [Ok(token), Err(mystery), Ok(usage), Ok(unnamed)] = items.as_slice() else {
      panic!("expected a token, an error, then two usage frames; got {items:?}");
    };
    assert_eq!(
      token,
      &Update::Token(frame("token".to_string(), "hi".to_string(), None, None))
    );
    assert_eq!(
      usage,
      &Update::Usage(frame("usage".to_string(), reading("u", 3), None, None))
    );
    assert_eq!(
      unnamed,
      &Update::Usage(frame("message".to_string(), reading("m", 4), None, None)),
      "a frame without an event name is a message"
    );
    let DiagnosticsError::DeserializationError { path, inner } = mystery else {
      panic!("expected a decode error, got {mystery:?}");
    };
    assert_eq!(path, "event");
    assert_eq!(
      inner.to_string(),
      "unknown variant `mystery`, expected one of `token`, `usage`, `message`"
    );
  }

  #[test]
  fn an_explicit_message_event_reads_as_an_unnamed_one() {
    let body = "event: message\ndata: 1\n\ndata: 2\n\nevent: add\ndata: 3\n\n";
    let optional = collect(EventStream::<SseFrame<u8, Option<Kind>>>::text(response(&[body]), None));
    let events = optional
      .iter()
      .map(|frame| frame.as_ref().map(SseFrame::event).ok())
      .collect::<Vec<_>>();
    assert_eq!(events, [Some(&None), Some(&None), Some(&Some(Kind::Add))]);

    let named = collect(EventStream::<SseFrame<u8>>::text(response(&[body]), None));
    let events = named
      .iter()
      .map(|frame| frame.as_ref().map(|frame| frame.event().as_str()).ok())
      .collect::<Vec<_>>();
    assert_eq!(events, [Some("message"), Some("message"), Some("add")]);
  }

  #[test]
  fn text_fields_read_as_numbers() {
    let body = "event: 7\ndata: -42\n\nevent: 8\ndata: 1.5\n\n";
    let integers = collect(EventStream::<SseFrame<i64, u16>>::text(response(&[body]), None));
    let [Ok(first), Err(DiagnosticsError::DeserializationError { path, .. })] = integers.as_slice() else {
      panic!("expected an integer, then an error for 1.5; got {integers:?}");
    };
    assert_eq!((first.event(), first.data()), (&7, &-42));
    assert_eq!(path, "data");
  }

  #[test]
  fn an_ended_stream_only_yields_none() {
    let cases = [
      (None, "data: {\"id\":\"a\",\"value\":1}\n\n"),
      (Some("[DONE]"), "data: [DONE]\n\n"),
    ];
    for (sentinel, body) in cases {
      let mut stream = EventStream::<SseFrame<Reading>>::json(response(&[body]), sentinel);
      while block_on(stream.next()).is_some() {}
      assert!(block_on(stream.next()).is_none(), "polled again after {body:?}");
    }
  }

  #[test]
  fn the_stream_keeps_the_id_and_retry_frames_without_data_set() {
    let body = "id: 1\ndata: {\"id\":\"a\",\"value\":1}\n\nid: 2\n\nretry: 9000\n\n";
    let mut stream = EventStream::<SseFrame<Reading>>::json(response(&[body]), None);
    assert_eq!(
      (stream.last_event_id(), stream.retry()),
      (None, None),
      "nothing is read yet"
    );
    let frames = block_on((&mut stream).collect::<Vec<_>>());
    assert_eq!(frames.len(), 1, "frames without data aren't yielded: {frames:?}");
    assert_eq!(stream.last_event_id().as_deref(), Some("2"));
    assert_eq!(stream.retry(), Some(Duration::from_millis(9000)));
  }

  #[test]
  fn a_body_error_ends_the_stream() {
    let response = body_response(vec![
      Ok(Bytes::from_static(b"data: {\"id\":\"a\",\"value\":1}\n\n")),
      Err(std::io::Error::other("connection reset")),
      Ok(Bytes::from_static(b"data: {\"id\":\"b\",\"value\":2}\n\n")),
    ]);
    let items = collect(EventStream::<SseFrame<Reading>>::json(response, None));
    assert!(
      matches!(items.as_slice(), [Ok(_), Err(DiagnosticsError::BodyReadError(_))]),
      "the frame before the error, then the error; got {items:?}"
    );

    let response = body_response(vec![
      Ok(Bytes::from_static(b"data: {\"id\":\"a\",")),
      Err(std::io::Error::other("connection reset")),
    ]);
    let items = collect(EventStream::<SseFrame<Reading>>::json(response, None));
    assert!(
      matches!(items.as_slice(), [Err(DiagnosticsError::BodyReadError(_))]),
      "a frame the error cuts off is never yielded; got {items:?}"
    );
  }
}

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

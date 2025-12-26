use std::{
  marker::PhantomData,
  pin::Pin,
  task::{Context, Poll},
};

use eventsource_stream::Eventsource;
use futures_core::Stream;
use serde::de::DeserializeOwned;

#[derive(Debug, thiserror::Error)]
pub enum EventStreamError {
  #[error("SSE parse error: {0}")]
  SseParse(#[from] eventsource_stream::EventStreamError<reqwest::Error>),

  #[error("JSON deserialization error at path {path}: {inner}")]
  JsonDeserialize { path: String, inner: serde_json::Error },
}

/// A stream of Server-Sent Events (SSE) that deserializes each event's data as JSON.
///
/// This wraps a `reqwest::Response` and parses the SSE event stream, deserializing
/// each event's `data` field as the type parameter `T`.
///
/// # Example
///
/// ```ignore
/// use futures::StreamExt;
///
/// let response = client.get("/events").send().await?;
/// let mut stream = EventStream::<MyEvent>::from_response(response);
///
/// while let Some(result) = stream.next().await {
///     match result {
///         Ok(event) => println!("Received: {:?}", event),
///         Err(e) => eprintln!("Error: {}", e),
///     }
/// }
/// ```
pub struct EventStream<T> {
  inner: Pin<
    Box<
      dyn Stream<Item = Result<eventsource_stream::Event, eventsource_stream::EventStreamError<reqwest::Error>>> + Send,
    >,
  >,
  _marker: PhantomData<T>,
}

impl<T> std::fmt::Debug for EventStream<T> {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    f.debug_struct("EventStream").finish_non_exhaustive()
  }
}

impl<T> EventStream<T>
where
  T: DeserializeOwned,
{
  /// Create an `EventStream` from an HTTP response.
  ///
  /// The response should have content type `text/event-stream`.
  #[must_use]
  pub fn from_response(response: reqwest::Response) -> Self {
    let stream = response.bytes_stream().eventsource();
    Self {
      inner: Box::pin(stream),
      _marker: PhantomData,
    }
  }

  fn parse_event(data: &str) -> Result<T, EventStreamError> {
    let mut de = serde_json::Deserializer::from_str(data);
    serde_path_to_error::deserialize(&mut de).map_err(|err| EventStreamError::JsonDeserialize {
      path: err.path().to_string(),
      inner: err.into_inner(),
    })
  }
}

impl<T> Stream for EventStream<T>
where
  T: DeserializeOwned + Unpin,
{
  type Item = Result<T, EventStreamError>;

  fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
    loop {
      match self.inner.as_mut().poll_next(cx) {
        Poll::Ready(Some(event_result)) => match event_result {
          Ok(event) => {
            if event.data.is_empty() {
              continue;
            }
            return Poll::Ready(Some(Self::parse_event(&event.data)));
          }
          Err(e) => return Poll::Ready(Some(Err(EventStreamError::SseParse(e)))),
        },
        Poll::Ready(None) => return Poll::Ready(None),
        Poll::Pending => return Poll::Pending,
      }
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[derive(Debug, serde::Deserialize, PartialEq)]
  struct TestEvent {
    id: i32,
    message: String,
  }

  #[test]
  fn test_parse_event_success() {
    let json = r#"{"id": 1, "message": "hello"}"#;
    let result: Result<TestEvent, EventStreamError> = EventStream::<TestEvent>::parse_event(json);

    assert!(result.is_ok());
    let event = result.unwrap();
    assert_eq!(event.id, 1);
    assert_eq!(event.message, "hello");
  }

  #[test]
  fn test_parse_event_invalid_json() {
    let json = r#"{"id": "not_a_number", "message": "hello"}"#;
    let result: Result<TestEvent, EventStreamError> = EventStream::<TestEvent>::parse_event(json);

    assert!(result.is_err());
    match result.unwrap_err() {
      EventStreamError::JsonDeserialize { path, .. } => {
        assert_eq!(path, "id");
      }
      EventStreamError::SseParse(err) => panic!("Expected JsonDeserialize error, got SseParse: {err}"),
    }
  }

  #[test]
  fn test_parse_event_missing_field() {
    let json = r#"{"id": 1}"#;
    let result: Result<TestEvent, EventStreamError> = EventStream::<TestEvent>::parse_event(json);

    assert!(result.is_err());
    match result.unwrap_err() {
      EventStreamError::JsonDeserialize { path, .. } => {
        assert!(path.contains("message") || path == ".");
      }
      EventStreamError::SseParse(err) => panic!("Expected JsonDeserialize error, got SseParse: {err}"),
    }
  }

  #[test]
  fn test_parse_event_empty_json() {
    let json = r"{}";
    let result: Result<TestEvent, EventStreamError> = EventStream::<TestEvent>::parse_event(json);

    assert!(result.is_err());
  }

  #[derive(Debug, serde::Deserialize, PartialEq)]
  struct NestedEvent {
    data: InnerData,
  }

  #[derive(Debug, serde::Deserialize, PartialEq)]
  struct InnerData {
    value: String,
  }

  #[test]
  fn test_parse_nested_event() {
    let json = r#"{"data": {"value": "nested"}}"#;
    let result: Result<NestedEvent, EventStreamError> = EventStream::<NestedEvent>::parse_event(json);

    assert!(result.is_ok());
    let event = result.unwrap();
    assert_eq!(event.data.value, "nested");
  }

  #[test]
  fn test_parse_nested_event_error_path() {
    let json = r#"{"data": {"value": 123}}"#;
    let result: Result<NestedEvent, EventStreamError> = EventStream::<NestedEvent>::parse_event(json);

    assert!(result.is_err());
    match result.unwrap_err() {
      EventStreamError::JsonDeserialize { path, .. } => {
        assert_eq!(path, "data.value");
      }
      EventStreamError::SseParse(err) => panic!("Expected JsonDeserialize error, got SseParse: {err}"),
    }
  }
}

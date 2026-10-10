use std::{
  fmt,
  pin::Pin,
  task::{Context, Poll},
};

use futures_core::Stream;

/// The items a generated server streams as Server-Sent Events (SSE).
///
/// An operation whose response is `text/event-stream` returns one of these from
/// its service method. The generated handler hands it to `axum::response::sse::Sse`,
/// sending each item as the JSON `data` of one event, in order, until the stream ends.
///
/// Build one from any `Send` stream with [`ServerEvents::new`], or collect an
/// iterator of items when they are known up front:
///
/// ```
/// use oas3_gen_support::ServerEvents;
///
/// let events = ["first", "second"].into_iter().collect::<ServerEvents<_>>();
/// ```
pub struct ServerEvents<T> {
  inner: Pin<Box<dyn Stream<Item = T> + Send>>,
}

impl<T> ServerEvents<T> {
  /// Wraps a stream of items.
  pub fn new(stream: impl Stream<Item = T> + Send + 'static) -> Self {
    Self {
      inner: Box::pin(stream),
    }
  }

  /// Transforms each item with `transform` as the stream yields it.
  pub fn map<U, F>(self, transform: F) -> ServerEventsMap<T, F>
  where
    F: FnMut(T) -> U,
  {
    ServerEventsMap {
      events: self,
      transform,
    }
  }
}

impl<T> fmt::Debug for ServerEvents<T> {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    f.debug_struct("ServerEvents").finish_non_exhaustive()
  }
}

impl<T> Stream for ServerEvents<T> {
  type Item = T;

  fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<T>> {
    self.inner.as_mut().poll_next(cx)
  }
}

impl<T: Send + 'static> FromIterator<T> for ServerEvents<T> {
  fn from_iter<I: IntoIterator<Item = T>>(items: I) -> Self {
    Self::new(Ready(items.into_iter().collect::<Vec<_>>().into_iter()))
  }
}

/// The stream [`ServerEvents::map`] returns.
pub struct ServerEventsMap<T, F> {
  events: ServerEvents<T>,
  transform: F,
}

impl<T, F> fmt::Debug for ServerEventsMap<T, F> {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    f.debug_struct("ServerEventsMap").finish_non_exhaustive()
  }
}

impl<T, U, F> Stream for ServerEventsMap<T, F>
where
  F: FnMut(T) -> U + Unpin,
{
  type Item = U;

  fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<U>> {
    let this = self.get_mut();
    Pin::new(&mut this.events)
      .poll_next(cx)
      .map(|item| item.map(&mut this.transform))
  }
}

/// A stream whose items are all available immediately.
struct Ready<I>(I);

impl<I> Unpin for Ready<I> {}

impl<I: Iterator> Stream for Ready<I> {
  type Item = I::Item;

  fn poll_next(mut self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Option<I::Item>> {
    Poll::Ready(self.0.next())
  }

  fn size_hint(&self) -> (usize, Option<usize>) {
    self.0.size_hint()
  }
}

#[cfg(test)]
mod tests {
  use futures::{StreamExt as _, executor::block_on, stream};

  use super::ServerEvents;

  #[test]
  fn yields_items_in_order_and_maps_them() {
    let cases = [vec![], vec![1], vec![1, 2, 3]];
    for items in cases {
      let collected = block_on(
        items
          .clone()
          .into_iter()
          .collect::<ServerEvents<_>>()
          .collect::<Vec<_>>(),
      );
      assert_eq!(collected, items, "collected items for {items:?}");

      let doubled = block_on(
        ServerEvents::new(stream::iter(items.clone()))
          .map(|item| item * 2)
          .collect::<Vec<_>>(),
      );
      let expected = items.iter().map(|item| item * 2).collect::<Vec<_>>();
      assert_eq!(doubled, expected, "mapped items for {items:?}");
    }
  }
}

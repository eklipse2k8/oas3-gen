use std::{fmt::Write as _, hint::black_box};

use bytes::Bytes;
use criterion::{BatchSize, Criterion, Throughput, criterion_group, criterion_main};
use futures::{StreamExt as _, executor::block_on};
use oas3_gen_support::{EventStream, SseFrame};
use serde::de::IgnoredAny;

const FRAMES: usize = 10_000;
const CHUNK_SIZE: usize = 64;

fn json_body() -> Bytes {
  let mut body = String::new();
  for index in 0..FRAMES {
    let _ = write!(
      body,
      "id: evt-{index}\ndata: {{\"id\":\"evt-{index}\",\"value\":{index},\"text\":\"a reading taken at step {index}\"}}\n\n"
    );
  }
  Bytes::from(body)
}

fn text_body() -> Bytes {
  let mut body = String::new();
  for index in 0..FRAMES {
    let _ = write!(body, "event: token\n: keep-alive\n");
    for line in 0..4 {
      let _ = writeln!(body, "data: line {line} of frame {index}, padded to a typical width");
    }
    body.push('\n');
  }
  Bytes::from(body)
}

fn crlf_body() -> Bytes {
  Bytes::from(String::from_utf8_lossy(&json_body()).replace('\n', "\r\n"))
}

fn response(body: &Bytes, chunked: bool) -> reqwest::Response {
  let body = if chunked {
    let chunks = (0..body.len())
      .step_by(CHUNK_SIZE)
      .map(|start| Ok::<_, std::io::Error>(body.slice(start..body.len().min(start + CHUNK_SIZE))))
      .collect::<Vec<_>>();
    reqwest::Body::wrap_stream(futures::stream::iter(chunks))
  } else {
    reqwest::Body::from(body.clone())
  };
  reqwest::Response::from(http::Response::new(body))
}

fn count<T>(stream: EventStream<T>) -> usize {
  block_on(stream.filter(|item| std::future::ready(item.is_ok())).count())
}

fn event_streams(c: &mut Criterion) {
  let bodies = [("json", json_body()), ("text", text_body()), ("crlf", crlf_body())];
  let mut group = c.benchmark_group("event_stream");
  for (name, body) in &bodies {
    group.throughput(Throughput::Bytes(body.len() as u64));
    for chunked in [false, true] {
      let delivery = if chunked { "chunked" } else { "whole" };
      group.bench_function(format!("{name}/{delivery}"), |b| {
        b.iter_batched(
          || response(body, chunked),
          |response| {
            let frames = if *name == "text" {
              count(EventStream::<SseFrame<String>>::text(response, None))
            } else {
              count(EventStream::<SseFrame<IgnoredAny>>::json(response, None))
            };
            assert_eq!(black_box(frames), FRAMES);
          },
          BatchSize::LargeInput,
        );
      });
    }
  }
  group.finish();
}

criterion_group!(benches, event_streams);
criterion_main!(benches);

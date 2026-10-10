#[path = "../petstore-server/support/mod.rs"]
mod support;

macro_rules! answered {
  ($response:expr, $pattern:pat $(if $guard:expr)? => $value:expr) => {
    match $response {
      $pattern $(if $guard)? => $value,
      other => anyhow::bail!("expected `{}`, got {other:?}", stringify!($pattern)),
    }
  };
}

macro_rules! received {
  ($service:expr, $variant:ident) => {
    match $service.last_received() {
      Some($crate::support::Received::$variant(request)) => request,
      other => anyhow::bail!("the service last received {other:?}, not {}", stringify!($variant)),
    }
  };
}

mod bodies;
mod parameters;
mod responses;
mod runtime;
mod schemas;
mod security;
mod smoke;
mod streams;
mod validation;

use std::sync::{Arc, Mutex, PoisonError};

use axum::{
  body::Body,
  extract::{Request, State},
  middleware::Next,
  response::Response,
};
use bytes::Bytes;
use petstore_client::PetstoreClient;
use proptest::test_runner::{Config, TestCaseError};
use support::{API_KEY, Petstore, PetstoreService};

#[derive(Debug, Clone)]
struct WireRequest {
  method: http::Method,
  uri: http::Uri,
  headers: http::HeaderMap,
  body: Bytes,
}

impl WireRequest {
  fn header(&self, name: impl http::header::AsHeaderName) -> Option<&str> {
    self.headers.get(name).and_then(|value| value.to_str().ok())
  }

  fn content_type(&self) -> Option<&str> {
    self.header(http::header::CONTENT_TYPE)
  }
}

type WireLog = Arc<Mutex<Vec<WireRequest>>>;

struct Wiretap {
  petstore: Petstore,
  log: WireLog,
}

impl Wiretap {
  async fn serve(router: axum::Router, service: PetstoreService) -> anyhow::Result<Self> {
    let log = WireLog::default();
    let router = router.layer(axum::middleware::from_fn_with_state(Arc::clone(&log), record));
    let base_url = support::serve(router).await;
    let client = PetstoreClient::with_base_url(&base_url)?.with_api_key_auth(API_KEY);
    Ok(Self {
      petstore: Petstore {
        base_url,
        client,
        service,
      },
      log,
    })
  }

  async fn petstore() -> anyhow::Result<Self> {
    let service = PetstoreService::default();
    Self::serve(petstore_server::router(service.clone()), service).await
  }

  fn client(&self) -> &PetstoreClient {
    &self.petstore.client
  }

  fn requests(&self) -> Vec<WireRequest> {
    std::mem::take(&mut *self.log.lock().unwrap_or_else(PoisonError::into_inner))
  }

  fn last(&self) -> anyhow::Result<WireRequest> {
    self
      .requests()
      .pop()
      .ok_or_else(|| anyhow::anyhow!("the router received no request"))
  }
}

async fn record(State(log): State<WireLog>, request: Request, next: Next) -> Response {
  let (parts, body) = request.into_parts();
  let body = axum::body::to_bytes(body, usize::MAX).await.unwrap_or_default();
  log.lock().unwrap_or_else(PoisonError::into_inner).push(WireRequest {
    method: parts.method.clone(),
    uri: parts.uri.clone(),
    headers: parts.headers.clone(),
    body: body.clone(),
  });
  next.run(Request::from_parts(parts, Body::from(body))).await
}

fn property_config(cases: u32) -> Config {
  Config {
    failure_persistence: None,
    ..Config::with_cases(cases)
  }
}

fn checked(result: anyhow::Result<()>) -> Result<(), TestCaseError> {
  result.map_err(|error| TestCaseError::fail(format!("{error:#}")))
}

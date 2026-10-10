//! Shared harness for the petstore fixture tests.
//!
//! Every test target serves the generated `petstore-server` router, backed by
//! [`PetstoreService`], an in-memory implementation of the generated `ApiServer`
//! trait, on a free local port, and drives it through the generated
//! `petstore-client`. A test that passes therefore proves the generated client and
//! server agree on the wire for that operation, not just that each compiles.

#![allow(dead_code, unreachable_pub, unused_imports)]

pub mod service;

use std::sync::LazyLock;

use chrono::{DateTime, NaiveDate, Utc};
use petstore_client::PetstoreClient;
pub use service::{PetstoreService, Received, State};
use tokio::runtime::Runtime;

/// The key every test client sends in `X-Api-Key`, and the only one staff endpoints accept.
pub const API_KEY: &str = "pk_test_petstore_2026";
/// The bearer token staff endpoints accept.
pub const STAFF_TOKEN: &str = "staff-token-2026";
/// The session cookie staff endpoints accept.
pub const SESSION: &str = "session-2026";
/// The `api_key` query key staff endpoints accept.
pub const QUERY_KEY: &str = "qk_test_2026";
/// The raw `Authorization` value the audit log accepts.
pub const AUDIT_TOKEN: &str = "audit-token-2026";
/// The API revision `showPetById` accepts in `x-api-version`.
pub const API_VERSION: &str = "2026-10";
/// The only compatibility date the document allows in `x-compatibility-date`.
pub const COMPATIBILITY_DATE: NaiveDate = NaiveDate::from_ymd_opt(2026, 6, 9).unwrap();

/// The fixed instant the service stamps on everything it creates, so responses are deterministic.
pub fn now() -> DateTime<Utc> {
  DateTime::from_timestamp(1_791_568_800, 0).expect("a valid timestamp")
}

/// A running petstore: the service behind the router, its base URL, and a client
/// that sends [`API_KEY`].
pub struct Petstore {
  pub base_url: String,
  pub client: PetstoreClient,
  pub service: PetstoreService,
}

impl Petstore {
  /// Serves a fresh, empty store and connects an authenticated client to it.
  pub async fn start() -> Self {
    let service = PetstoreService::default();
    let base_url = serve(petstore_server::router(service.clone())).await;
    let client = PetstoreClient::with_base_url(&base_url)
      .expect("the base URL parses")
      .with_api_key_auth(API_KEY);
    Self {
      base_url,
      client,
      service,
    }
  }

  /// A client for this store that sends no credentials.
  pub fn anonymous(&self) -> PetstoreClient {
    PetstoreClient::with_base_url(&self.base_url).expect("the base URL parses")
  }

  /// The absolute URL of `path` on this store, for requests the generated client can't make.
  pub fn url(&self, path: &str) -> String {
    format!("{}{path}", self.base_url)
  }
}

/// Serves `router` on a free local port and returns its base URL.
pub async fn serve(router: axum::Router) -> String {
  let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
    .await
    .expect("bind a free port");
  let address = listener.local_addr().expect("listener address");
  tokio::spawn(async move { axum::serve(listener, router).await });
  format!("http://{address}")
}

/// A fresh store per test.
#[rstest::fixture]
pub async fn petstore() -> Petstore {
  Petstore::start().await
}

/// The multi-threaded runtime property tests block on.
///
/// `proptest!` cases are synchronous, so each property starts one [`Petstore`] on
/// this runtime and blocks on every case. Servers spawned here keep running between
/// cases on the runtime's worker threads.
pub fn runtime() -> &'static Runtime {
  static RUNTIME: LazyLock<Runtime> = LazyLock::new(|| Runtime::new().expect("a tokio runtime"));
  &RUNTIME
}

/// Runs one property case on [`runtime`].
///
/// `proptest!` cases are synchronous, so each case body is an async closure: it borrows the
/// case's generated inputs and the shared store directly, and `prop_assert!` failures return
/// out of it as the case's `Err`.
pub fn run_case<T>(case: impl AsyncFnOnce() -> T) -> T {
  runtime().block_on(case())
}

/// Starts a store on [`runtime`] for a property test whose cases share it.
pub fn start_shared() -> Petstore {
  runtime().block_on(Petstore::start())
}

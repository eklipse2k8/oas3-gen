#![allow(
  dead_code,
  unused_imports,
  reason = "the client and server targets each use a different part of the shared harness"
)]

pub mod service;

use std::sync::LazyLock;

use chrono::{DateTime, NaiveDate, Utc};
use petstore_client::PetstoreClient;
pub use service::{PetstoreService, Received, State};
use tokio::runtime::Runtime;

pub const API_KEY: &str = "pk_test_petstore_2026";
pub const STAFF_TOKEN: &str = "staff-token-2026";
pub const SESSION: &str = "session-2026";
pub const QUERY_KEY: &str = "qk_test_2026";
pub const AUDIT_TOKEN: &str = "audit-token-2026";
pub const API_VERSION: &str = "2026-10";
pub const COMPATIBILITY_DATE: NaiveDate = NaiveDate::from_ymd_opt(2026, 6, 9).expect("a valid calendar date");

pub fn now() -> DateTime<Utc> {
  DateTime::from_timestamp(1_791_568_800, 0).expect("a valid timestamp")
}

pub struct Petstore {
  pub base_url: String,
  pub client: PetstoreClient,
  pub service: PetstoreService,
}

impl Petstore {
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

  pub fn anonymous(&self) -> PetstoreClient {
    PetstoreClient::with_base_url(&self.base_url).expect("the base URL parses")
  }

  pub fn url(&self, path: &str) -> String {
    format!("{}{path}", self.base_url)
  }
}

pub async fn serve(router: axum::Router) -> String {
  let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
    .await
    .expect("bind a free port");
  let address = listener.local_addr().expect("listener address");
  tokio::spawn(async move { axum::serve(listener, router).await });
  format!("http://{address}")
}

#[rstest::fixture]
pub async fn petstore() -> Petstore {
  Petstore::start().await
}

pub fn runtime() -> &'static Runtime {
  static RUNTIME: LazyLock<Runtime> = LazyLock::new(|| Runtime::new().expect("a tokio runtime"));
  &RUNTIME
}

pub fn run_case<T>(case: impl AsyncFnOnce() -> T) -> T {
  runtime().block_on(case())
}

pub fn start_shared() -> Petstore {
  runtime().block_on(Petstore::start())
}

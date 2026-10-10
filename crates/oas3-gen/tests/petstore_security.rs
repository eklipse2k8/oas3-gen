//! Security schemes and requirements, end to end.
//!
//! The petstore document declares API keys in a header, a query parameter, a cookie,
//! and the raw `Authorization` header, HTTP bearer and basic schemes, `oauth2`, and
//! `openIdConnect`. It combines them per operation: the global `ApiKeyAuth`
//! requirement, optional security (`{}`), opting out (`security: []`), alternatives
//! (OR), and schemes required together (AND). These tests drive the generated
//! client's credential setters against the generated server's credential extractors
//! and prove that every requirement admits what the document allows, refuses the
//! rest before the service runs, delivers each secret byte for byte, and never
//! prints one.

mod support;

use std::sync::{Arc, Mutex};

use axum::{Json, Router, http::HeaderMap, routing::get};
use petstore_client::{
  AdoptPetRequest, Adopter, AdoptionRequest, ApiResponse, CreateApiKeyRequest, DeletePetRequest, DescribePetsRequest,
  Error, Frequency, GetApiKeyRequest, GetHealthRequest, GetInventoryRequest, ListApiKeysRequest,
  ListAuditEventsRequest, ListPetsRequest, LocationHeaders, NewApiKey, NewsletterSignup, PetstoreClient,
  ReplaceApiKeyRequest, RevokeApiKeyRequest, SubscribeNewsletterRequest, WithHeaders,
};
use petstore_server as server;
use proptest::prelude::*;
use reqwest::{
  Method, StatusCode,
  header::{AUTHORIZATION, CONTENT_TYPE, COOKIE},
};
use rstest::rstest;
use secrecy::{ExposeSecret, SecretString};
use support::{
  API_KEY, AUDIT_TOKEN, COMPATIBILITY_DATE, Petstore, PetstoreService, QUERY_KEY, Received, SESSION, STAFF_TOKEN,
  petstore, run_case, start_shared,
};
use uuid::Uuid;

/// The header the `ApiKeyAuth` scheme names.
const X_API_KEY: &str = "x-api-key";
/// A credential value no scheme accepts.
const UNKNOWN_SECRET: &str = "not-a-known-secret";
/// The id of the API key tests seed before reading, replacing, or revoking it.
const SEEDED_KEY_ID: &str = "key_seeded";
/// A valid `NewApiKey` body as raw JSON.
const NEW_API_KEY_JSON: &str = r#"{"name":"ci","value":"sk_ci"}"#;
/// A JSON body cut off mid-object.
const MALFORMED_JSON: &str = r#"{"name": "#;

/// The credentials a caller presents, one slot per scheme the generated client can send.
#[derive(Debug, Clone, Copy)]
struct Credentials {
  api_key_auth: Option<&'static str>,
  bearer_auth: Option<&'static str>,
  query_key: Option<&'static str>,
  session_cookie: Option<&'static str>,
  token_auth: Option<&'static str>,
}

impl Credentials {
  const NONE: Self = Self {
    api_key_auth: None,
    bearer_auth: None,
    query_key: None,
    session_cookie: None,
    token_auth: None,
  };

  const fn with_api_key_auth(self, api_key: &'static str) -> Self {
    Self {
      api_key_auth: Some(api_key),
      ..self
    }
  }

  const fn with_bearer_auth(self, token: &'static str) -> Self {
    Self {
      bearer_auth: Some(token),
      ..self
    }
  }

  const fn with_query_key(self, api_key: &'static str) -> Self {
    Self {
      query_key: Some(api_key),
      ..self
    }
  }

  const fn with_session_cookie(self, session: &'static str) -> Self {
    Self {
      session_cookie: Some(session),
      ..self
    }
  }

  const fn with_token_auth(self, api_key: &'static str) -> Self {
    Self {
      token_auth: Some(api_key),
      ..self
    }
  }

  /// A generated client for `petstore` configured through its credential setters.
  fn client(self, petstore: &Petstore) -> PetstoreClient {
    let mut client = petstore.anonymous();
    if let Some(api_key) = self.api_key_auth {
      client = client.with_api_key_auth(api_key);
    }
    if let Some(token) = self.bearer_auth {
      client = client.with_bearer_auth(token);
    }
    if let Some(api_key) = self.query_key {
      client = client.with_query_key(api_key);
    }
    if let Some(session) = self.session_cookie {
      client = client.with_session_cookie(session);
    }
    if let Some(api_key) = self.token_auth {
      client = client.with_token_auth(api_key);
    }
    client
  }

  /// Attaches the credentials to a raw request where each scheme says they travel.
  fn attach(self, mut request: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
    if let Some(api_key) = self.api_key_auth {
      request = request.header(X_API_KEY, api_key);
    }
    if let Some(token) = self.bearer_auth {
      request = request.bearer_auth(token);
    }
    if let Some(api_key) = self.query_key {
      request = request.query(&[("api_key", api_key)]);
    }
    if let Some(session) = self.session_cookie {
      request = request.header(COOKIE, format!("session={session}"));
    }
    if let Some(api_key) = self.token_auth {
      request = request.header(AUTHORIZATION, api_key);
    }
    request
  }
}

/// The plain text of an optional secret.
fn exposed(secret: Option<&SecretString>) -> Option<&str> {
  secret.map(ExposeSecret::expose_secret)
}

/// The request the service received most recently.
fn last_received(petstore: &Petstore) -> Received {
  petstore
    .service
    .last_received()
    .expect("the service received a request")
}

/// Stores an API key under [`SEEDED_KEY_ID`].
fn seed_api_key(petstore: &Petstore) {
  petstore.service.with_state(|state| {
    state.api_keys.insert(
      SEEDED_KEY_ID.to_string(),
      server::ApiKey {
        id: SEEDED_KEY_ID.to_string(),
        name: "seeded".to_string(),
      },
    );
  });
}

fn list_api_keys_request() -> ListApiKeysRequest {
  ListApiKeysRequest::builder()
    .build()
    .expect("a listing without a limit is valid")
}

fn create_api_key_request() -> CreateApiKeyRequest {
  let body = NewApiKey {
    name: "ci".to_string(),
    value: "sk_ci".to_string(),
  };
  CreateApiKeyRequest::builder()
    .body(body)
    .build()
    .expect("a named key is valid")
}

fn get_seeded_key_request() -> GetApiKeyRequest {
  GetApiKeyRequest::builder()
    .key_id(SEEDED_KEY_ID.to_string())
    .build()
    .expect("a key id is valid")
}

fn replace_seeded_key_request() -> ReplaceApiKeyRequest {
  let body = NewApiKey {
    name: "rotated".to_string(),
    value: "sk_rotated".to_string(),
  };
  ReplaceApiKeyRequest::builder()
    .key_id(SEEDED_KEY_ID.to_string())
    .body(body)
    .build()
    .expect("a named key is valid")
}

fn revoke_seeded_key_request() -> RevokeApiKeyRequest {
  RevokeApiKeyRequest::builder()
    .key_id(SEEDED_KEY_ID.to_string())
    .build()
    .expect("a key id is valid")
}

/// Sends a request the generated client can't make.
async fn send_raw(
  petstore: &Petstore,
  method: Method,
  path: &str,
  credentials: Credentials,
  json_body: Option<&str>,
) -> reqwest::Response {
  let request = credentials.attach(reqwest::Client::new().request(method, petstore.url(path)));
  let request = match json_body {
    Some(body) => request.header(CONTENT_TYPE, "application/json").body(body.to_owned()),
    None => request,
  };
  request.send().await.expect("the store answers")
}

/// A staff operation's parsed answer, reduced to whether it was the declared `401`.
#[derive(Debug)]
enum StaffAnswer {
  Unauthorized(Error),
  Other(#[expect(dead_code, reason = "read through `Debug` in assertion messages")] String),
}

impl<Value: std::fmt::Debug> From<ApiResponse<Value, Error>> for StaffAnswer {
  fn from(response: ApiResponse<Value, Error>) -> Self {
    match response {
      ApiResponse::Unauthorized(error) => Self::Unauthorized(error),
      other => Self::Other(format!("{other:?}")),
    }
  }
}

/// Operations without their own `security`, which inherit the document's `ApiKeyAuth`.
#[derive(Debug, Clone, Copy)]
enum InheritingOperation {
  ListApiKeys,
  GetInventory,
}

impl InheritingOperation {
  /// Calls the operation and reports whether it answered `200 OK`.
  async fn succeeds(self, client: &PetstoreClient) -> bool {
    match self {
      Self::ListApiKeys => matches!(
        client.list_api_keys(list_api_keys_request()).await,
        Ok(ApiResponse::Ok(_))
      ),
      Self::GetInventory => matches!(
        client.get_inventory(GetInventoryRequest {}).await,
        Ok(ApiResponse::Ok(_))
      ),
    }
  }
}

/// The `X-Api-Key` the server extracted for an operation that inherits `ApiKeyAuth`.
fn inherited_api_key(received: &Received) -> Option<&str> {
  match received {
    Received::ListApiKeys(request) => Some(request.credentials.api_key_auth.expose_secret()),
    Received::GetInventory(request) => Some(request.credentials.api_key_auth.expose_secret()),
    _ => None,
  }
}

/// Operations whose `security: []` opts out of the global requirement.
#[derive(Debug, Clone, Copy)]
enum PublicOperation {
  GetHealth,
  DescribePets,
  SubscribeNewsletter,
}

impl PublicOperation {
  /// Calls the operation and reports whether it answered with its documented success status.
  async fn succeeds(self, client: &PetstoreClient) -> bool {
    match self {
      Self::GetHealth => matches!(
        client.get_health(GetHealthRequest {}).await,
        Ok(ApiResponse::NoContent(()))
      ),
      Self::DescribePets => matches!(
        client.describe_pets(DescribePetsRequest {}).await,
        Ok(ApiResponse::NoContent(_))
      ),
      Self::SubscribeNewsletter => {
        let signup = NewsletterSignup {
          email: "ada@example.com".to_string(),
          frequency: Frequency::Weekly,
          ..Default::default()
        };
        let request = SubscribeNewsletterRequest::builder()
          .body(signup)
          .build()
          .expect("the signup is valid");
        matches!(client.subscribe_newsletter(request).await, Ok(ApiResponse::Created(_)))
      }
    }
  }

  /// Whether `received` is a request for this operation.
  fn is(self, received: &Received) -> bool {
    matches!(
      (self, received),
      (Self::GetHealth, Received::GetHealth(_))
        | (Self::DescribePets, Received::DescribePets(_))
        | (Self::SubscribeNewsletter, Received::SubscribeNewsletter(_))
    )
  }
}

/// Staff operations whose document declares `401` with an `Error` body.
#[derive(Debug, Clone, Copy)]
enum StaffOperation {
  ListApiKeys,
  CreateApiKey,
  GetApiKey,
  ReplaceApiKey,
  RevokeApiKey,
}

impl StaffOperation {
  /// Calls the operation through the generated client.
  async fn call(self, client: &PetstoreClient) -> anyhow::Result<StaffAnswer> {
    Ok(match self {
      Self::ListApiKeys => client.list_api_keys(list_api_keys_request()).await?.into(),
      Self::CreateApiKey => client.create_api_key(create_api_key_request()).await?.into(),
      Self::GetApiKey => client.get_api_key(get_seeded_key_request()).await?.into(),
      Self::ReplaceApiKey => client.replace_api_key(replace_seeded_key_request()).await?.into(),
      Self::RevokeApiKey => client.revoke_api_key(revoke_seeded_key_request()).await?.into(),
    })
  }

  /// Sends the same request without the generated client, carrying exactly `credentials`.
  async fn send_without_client(self, petstore: &Petstore, credentials: Credentials) -> reqwest::Response {
    let key_path = format!("/staff/keys/{SEEDED_KEY_ID}");
    let (method, path, json_body) = match self {
      Self::ListApiKeys => (Method::GET, "/staff/keys", None),
      Self::CreateApiKey => (Method::POST, "/staff/keys", Some(NEW_API_KEY_JSON)),
      Self::GetApiKey => (Method::GET, key_path.as_str(), None),
      Self::ReplaceApiKey => (Method::PUT, key_path.as_str(), Some(NEW_API_KEY_JSON)),
      Self::RevokeApiKey => (Method::DELETE, key_path.as_str(), None),
    };
    send_raw(petstore, method, path, credentials, json_body).await
  }
}

/// Which `listAuditEvents` alternative the server should see satisfied.
#[derive(Debug, Clone, Copy)]
enum AuditScheme {
  Bearer,
  Token,
}

#[rstest]
#[case::list_api_keys_inherits_the_global_key(Method::GET, "/staff/keys", Credentials::NONE, None)]
#[case::get_inventory_inherits_the_global_key(Method::GET, "/store/inventory", Credentials::NONE, None)]
#[case::create_pet_inherits_the_global_key(
  Method::POST,
  "/pets",
  Credentials::NONE,
  Some(r#"{"name":"Biscuit","species":"dog"}"#)
)]
#[case::create_api_key_without_a_query_key_or_session(
  Method::POST,
  "/staff/keys",
  Credentials::NONE,
  Some(NEW_API_KEY_JSON)
)]
#[case::get_api_key_with_the_header_key_alone(
  Method::GET,
  "/staff/keys/key_seeded",
  Credentials::NONE.with_api_key_auth(API_KEY),
  None
)]
#[case::get_api_key_with_the_session_alone(
  Method::GET,
  "/staff/keys/key_seeded",
  Credentials::NONE.with_session_cookie(SESSION),
  None
)]
#[case::revoke_api_key_without_a_bearer_token_or_key(Method::DELETE, "/staff/keys/key_seeded", Credentials::NONE, None)]
#[case::delete_pet_without_a_bearer_token_or_key(
  Method::DELETE,
  "/pets/00000000-0000-0000-0000-000000000001",
  Credentials::NONE,
  None
)]
#[case::list_audit_events_without_authorization(Method::GET, "/staff/audit", Credentials::NONE, None)]
#[tokio::test]
async fn a_request_missing_a_required_credential_is_refused_before_the_service_runs(
  #[future(awt)] petstore: Petstore,
  #[case] method: Method,
  #[case] path: &str,
  #[case] credentials: Credentials,
  #[case] json_body: Option<&str>,
) {
  seed_api_key(&petstore);
  let target = format!("{method} {path} with {credentials:?}");
  let status = send_raw(&petstore, method, path, credentials, json_body).await.status();
  assert_eq!(status, StatusCode::UNAUTHORIZED, "{target}");
  assert!(petstore.service.received().is_empty(), "the service ran for {target}");
}

#[rstest]
#[case::list_api_keys(InheritingOperation::ListApiKeys)]
#[case::get_inventory(InheritingOperation::GetInventory)]
#[tokio::test]
async fn an_inherited_requirement_receives_exactly_the_client_api_key(
  #[future(awt)] petstore: Petstore,
  #[case] operation: InheritingOperation,
) {
  assert!(operation.succeeds(&petstore.client).await, "{operation:?}");
  assert_eq!(
    inherited_api_key(&last_received(&petstore)),
    Some(API_KEY),
    "{operation:?}"
  );
}

#[rstest]
#[case::list_api_keys_with_an_unknown_key(StaffOperation::ListApiKeys, Credentials::NONE.with_api_key_auth(UNKNOWN_SECRET))]
#[case::create_api_key_with_an_unknown_query_key(
  StaffOperation::CreateApiKey,
  Credentials::NONE.with_query_key(UNKNOWN_SECRET)
)]
#[case::get_api_key_with_an_unknown_session(
  StaffOperation::GetApiKey,
  Credentials::NONE.with_api_key_auth(API_KEY).with_session_cookie(UNKNOWN_SECRET)
)]
#[case::replace_api_key_with_an_unknown_key(
  StaffOperation::ReplaceApiKey,
  Credentials::NONE.with_api_key_auth(UNKNOWN_SECRET)
)]
#[case::revoke_api_key_with_an_unknown_bearer_token(
  StaffOperation::RevokeApiKey,
  Credentials::NONE.with_bearer_auth(UNKNOWN_SECRET)
)]
#[tokio::test]
async fn credentials_the_service_rejects_parse_as_the_declared_unauthorized_error(
  #[future(awt)] petstore: Petstore,
  #[case] operation: StaffOperation,
  #[case] credentials: Credentials,
) {
  seed_api_key(&petstore);
  let answer = operation.call(&credentials.client(&petstore)).await;
  assert!(
    matches!(answer, Ok(StaffAnswer::Unauthorized(Error { code: 401, .. }))),
    "{operation:?} with {credentials:?}: {answer:?}"
  );
  assert_eq!(
    petstore.service.received().len(),
    1,
    "the service decides {operation:?} with {credentials:?}"
  );
}

/// A request missing a required credential never reaches the service: the generated extractor
/// refuses it with `401` and a plain-text reason naming what is missing. The document's JSON `Error`
/// for `401` is what the service answers when it rejects credential values, so the generated client
/// reports an extractor refusal as an error rather than parsing it as `Unauthorized`.
#[rstest]
#[case::list_api_keys_without_a_key(StaffOperation::ListApiKeys, Credentials::NONE, &["`X-Api-Key`"])]
#[case::create_api_key_without_a_query_key_or_session(
  StaffOperation::CreateApiKey,
  Credentials::NONE,
  &["`QueryKey`", "`SessionCookie`"]
)]
#[case::get_api_key_without_a_session(
  StaffOperation::GetApiKey,
  Credentials::NONE.with_api_key_auth(API_KEY),
  &["`session`"]
)]
#[case::revoke_api_key_without_a_bearer_token_or_key(
  StaffOperation::RevokeApiKey,
  Credentials::NONE,
  &["`ApiKeyAuth`", "`BearerAuth`"]
)]
#[tokio::test]
async fn missing_credentials_are_rejected_before_the_service_runs(
  #[future(awt)] petstore: Petstore,
  #[case] operation: StaffOperation,
  #[case] credentials: Credentials,
  #[case] named_in_reason: &[&str],
) {
  seed_api_key(&petstore);
  let response = operation.send_without_client(&petstore, credentials).await;
  assert_eq!(response.status(), StatusCode::UNAUTHORIZED, "{operation:?}");
  let content_type = response
    .headers()
    .get(CONTENT_TYPE)
    .and_then(|value| value.to_str().ok())
    .unwrap_or_default()
    .to_owned();
  assert!(
    content_type.starts_with("text/plain"),
    "{operation:?} answered `{content_type}`"
  );
  let reason = response.text().await.expect("a UTF-8 reason");
  assert!(
    named_in_reason.iter().all(|name| reason.contains(name)),
    "{operation:?}: `{reason}` must name {named_in_reason:?}"
  );

  let answer = operation.call(&credentials.client(&petstore)).await;
  assert!(
    answer.is_err(),
    "{operation:?} with {credentials:?} must report the extractor 401 as an error, got {answer:?}"
  );
  assert!(
    petstore.service.received().is_empty(),
    "the service ran for {operation:?}"
  );
}

#[rstest]
#[case::anonymous(Credentials::NONE)]
#[case::with_an_api_key(Credentials::NONE.with_api_key_auth(API_KEY))]
#[tokio::test]
async fn list_pets_serves_callers_with_or_without_an_api_key(
  #[future(awt)] petstore: Petstore,
  #[case] credentials: Credentials,
) {
  let request = ListPetsRequest::builder()
    .x_compatibility_date(COMPATIBILITY_DATE)
    .build()
    .expect("the supported compatibility date is valid");
  let response = credentials
    .client(&petstore)
    .list_pets(request)
    .await
    .expect("the store answers");
  assert!(matches!(response, ApiResponse::Ok(_)), "{response:?}");
  let Received::ListPets(request) = last_received(&petstore) else {
    panic!("the service did not receive listPets");
  };
  assert_eq!(
    exposed(request.credentials.api_key_auth.as_ref()),
    credentials.api_key_auth
  );
}

#[rstest]
#[case::get_health(PublicOperation::GetHealth)]
#[case::describe_pets(PublicOperation::DescribePets)]
#[case::subscribe_newsletter(PublicOperation::SubscribeNewsletter)]
#[tokio::test]
async fn operations_that_opt_out_of_security_serve_anonymous_callers(
  #[future(awt)] petstore: Petstore,
  #[case] operation: PublicOperation,
) {
  assert!(operation.succeeds(&petstore.anonymous()).await, "{operation:?}");
  assert!(operation.is(&last_received(&petstore)), "{operation:?}");
}

#[rstest]
#[case::pet_adopted(
  "/webhooks/petAdopted",
  r#"{"adoptionId":7,"status":"approved","occurredAt":"2026-10-09T12:00:00Z"}"#,
  StatusCode::OK,
  |received: &Received| matches!(received, Received::PetAdopted(_))
)]
#[case::inventory_low(
  "/webhooks/inventoryLow",
  r#"{"species":"cat","remaining":2}"#,
  StatusCode::NO_CONTENT,
  |received: &Received| matches!(received, Received::InventoryLow(_))
)]
#[tokio::test]
async fn webhooks_need_no_credentials(
  #[future(awt)] petstore: Petstore,
  #[case] path: &str,
  #[case] event: &str,
  #[case] acknowledged: StatusCode,
  #[case] is_event: fn(&Received) -> bool,
) {
  let status = send_raw(&petstore, Method::POST, path, Credentials::NONE, Some(event))
    .await
    .status();
  assert_eq!(status, acknowledged, "POST {path}");
  assert!(is_event(&last_received(&petstore)), "POST {path}");
}

#[rstest]
#[case::query_key_alone(Credentials::NONE.with_query_key(QUERY_KEY))]
#[case::session_cookie_alone(Credentials::NONE.with_session_cookie(SESSION))]
#[case::both(Credentials::NONE.with_query_key(QUERY_KEY).with_session_cookie(SESSION))]
#[tokio::test]
async fn create_api_key_accepts_a_query_key_or_a_session_cookie(
  #[future(awt)] petstore: Petstore,
  #[case] credentials: Credentials,
) {
  let response = credentials
    .client(&petstore)
    .create_api_key(create_api_key_request())
    .await
    .expect("the store answers");
  let ApiResponse::Created(key) = response else {
    panic!("{credentials:?} must create a key, got {response:?}");
  };
  assert_eq!(key.name, "ci");
  let Received::CreateApiKey(request) = last_received(&petstore) else {
    panic!("the service did not receive createApiKey");
  };
  assert_eq!(exposed(request.credentials.query_key.as_ref()), credentials.query_key);
  assert_eq!(
    exposed(request.credentials.session_cookie.as_ref()),
    credentials.session_cookie
  );
}

#[rstest]
#[tokio::test]
async fn get_api_key_answers_when_the_header_key_and_session_arrive_together(#[future(awt)] petstore: Petstore) {
  seed_api_key(&petstore);
  let client = Credentials::NONE
    .with_api_key_auth(API_KEY)
    .with_session_cookie(SESSION)
    .client(&petstore);
  let response = client
    .get_api_key(get_seeded_key_request())
    .await
    .expect("the store answers");
  let ApiResponse::Ok(key) = response else {
    panic!("both credentials must read the key, got {response:?}");
  };
  assert_eq!(key.id, SEEDED_KEY_ID);
  let Received::GetApiKey(request) = last_received(&petstore) else {
    panic!("the service did not receive getApiKey");
  };
  assert_eq!(request.credentials.api_key_auth.expose_secret(), API_KEY);
  assert_eq!(request.credentials.session_cookie.expose_secret(), SESSION);
}

/// The server trims quotes around the session cookie, and percent-decodes it as it does cookie
/// parameters, before the key check.
#[rstest]
#[case::quoted(format!("session=\"{SESSION}\""))]
#[case::percent_encoded(format!("session={}", SESSION.replace('-', "%2D")))]
#[tokio::test]
async fn get_api_key_reads_the_session_cookie_value(#[future(awt)] petstore: Petstore, #[case] cookie: String) {
  seed_api_key(&petstore);
  let response = reqwest::Client::new()
    .get(petstore.url(&format!("/staff/keys/{SEEDED_KEY_ID}")))
    .header(X_API_KEY, API_KEY)
    .header(COOKIE, cookie)
    .send()
    .await
    .expect("the store answers");
  assert_eq!(response.status(), StatusCode::OK);
  let Received::GetApiKey(request) = last_received(&petstore) else {
    panic!("the service did not receive getApiKey");
  };
  assert_eq!(request.credentials.session_cookie.expose_secret(), SESSION);
}

/// The client sends an API key cookie verbatim, so a value arrives unchanged when it is a
/// cookie-octet string the server decodes to itself. Space and semicolon can't ride a `Cookie`
/// header, and a valid percent-escape is decoded by the server, so neither is a case here.
#[rstest]
#[case::plain(SESSION)]
#[case::plus_and_equals("a+b=c")]
#[case::percent_sign("p%q")]
#[tokio::test]
async fn session_cookie_values_arrive_verbatim(#[future(awt)] petstore: Petstore, #[case] session: &'static str) {
  let response = Credentials::NONE
    .with_session_cookie(session)
    .client(&petstore)
    .create_api_key(create_api_key_request())
    .await;
  assert!(response.is_ok(), "{response:?}");
  let Received::CreateApiKey(request) = last_received(&petstore) else {
    panic!("the service did not receive createApiKey");
  };
  assert_eq!(exposed(request.credentials.session_cookie.as_ref()), Some(session));
}

/// An API key cookie reaches the wire as the caller wrote it. Third-party servers don't
/// percent-decode cookies, so an encoded value would not match what they issued.
#[rstest]
#[case::base64_with_slash_and_equals("dGVzdC9rZXk/Pw==")]
#[case::email_and_colon("user@example.com:secret")]
#[tokio::test]
async fn session_cookie_api_key_is_sent_verbatim_on_the_wire(#[case] session: &'static str) {
  let wire_cookie = Arc::new(Mutex::new(None));
  let recorder = Arc::clone(&wire_cookie);
  let router = server::router(PetstoreService::default()).layer(axum::middleware::from_fn(
    move |request: axum::extract::Request, next: axum::middleware::Next| {
      *recorder.lock().expect("the wire lock is never poisoned") = request
        .headers()
        .get(COOKIE)
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned);
      next.run(request)
    },
  ));
  let base_url = support::serve(router).await;
  let response = PetstoreClient::with_base_url(&base_url)
    .expect("the base URL parses")
    .with_session_cookie(session)
    .create_api_key(create_api_key_request())
    .await;
  assert!(response.is_ok(), "{response:?}");
  assert_eq!(
    wire_cookie.lock().expect("the wire lock is never poisoned").as_deref(),
    Some(format!("session={session}").as_str()),
    "{session:?}"
  );
}

#[rstest]
#[case::bearer_token(Credentials::NONE.with_bearer_auth(STAFF_TOKEN))]
#[case::api_key(Credentials::NONE.with_api_key_auth(API_KEY))]
#[case::both(Credentials::NONE.with_bearer_auth(STAFF_TOKEN).with_api_key_auth(API_KEY))]
#[tokio::test]
async fn revoke_api_key_accepts_a_bearer_token_or_an_api_key(
  #[future(awt)] petstore: Petstore,
  #[case] credentials: Credentials,
) {
  seed_api_key(&petstore);
  let response = credentials
    .client(&petstore)
    .revoke_api_key(revoke_seeded_key_request())
    .await
    .expect("the store answers");
  assert!(
    matches!(response, ApiResponse::NoContent(())),
    "{credentials:?}: {response:?}"
  );
  let Received::RevokeApiKey(request) = last_received(&petstore) else {
    panic!("the service did not receive revokeApiKey");
  };
  assert_eq!(
    exposed(request.credentials.bearer_auth.as_ref()),
    credentials.bearer_auth
  );
  assert_eq!(
    exposed(request.credentials.api_key_auth.as_ref()),
    credentials.api_key_auth
  );
  assert!(
    !petstore
      .service
      .with_state(|state| state.api_keys.contains_key(SEEDED_KEY_ID))
  );
}

#[rstest]
#[case::bearer_token(Credentials::NONE.with_bearer_auth(STAFF_TOKEN))]
#[case::api_key(Credentials::NONE.with_api_key_auth(API_KEY))]
#[tokio::test]
async fn delete_pet_accepts_a_bearer_token_or_an_api_key(
  #[future(awt)] petstore: Petstore,
  #[case] credentials: Credentials,
) {
  let pet_id = petstore.service.insert_pet(server::Pet {
    id: Uuid::from_u128(1),
    name: "Biscuit".to_string(),
    species: server::Species::Dog,
    ..Default::default()
  });
  let request = DeletePetRequest::builder()
    .pet_id(pet_id)
    .build()
    .expect("a pet id is valid");
  let response = credentials
    .client(&petstore)
    .delete_pet(request)
    .await
    .expect("the store answers");
  assert!(
    matches!(response, ApiResponse::NoContent(())),
    "{credentials:?}: {response:?}"
  );
  let Received::DeletePet(request) = last_received(&petstore) else {
    panic!("the service did not receive deletePet");
  };
  assert_eq!(
    exposed(request.credentials.bearer_auth.as_ref()),
    credentials.bearer_auth
  );
  assert_eq!(
    exposed(request.credentials.api_key_auth.as_ref()),
    credentials.api_key_auth
  );
  assert!(!petstore.service.with_state(|state| state.pets.contains_key(&pet_id)));
}

#[rstest]
#[case::canonical_scheme("Bearer ", true)]
#[case::lowercase_scheme("bearer ", true)]
#[case::uppercase_scheme("BEARER ", true)]
#[case::extra_spaces("Bearer    ", true)]
#[case::basic_scheme("Basic ", false)]
#[case::no_separator("Bearer", false)]
#[tokio::test]
async fn revoke_api_key_reads_the_bearer_scheme_leniently(
  #[future(awt)] petstore: Petstore,
  #[case] prefix: &str,
  #[case] accepted: bool,
) {
  seed_api_key(&petstore);
  let authorization = format!("{prefix}{STAFF_TOKEN}");
  let response = reqwest::Client::new()
    .delete(petstore.url(&format!("/staff/keys/{SEEDED_KEY_ID}")))
    .header(AUTHORIZATION, &authorization)
    .send()
    .await
    .expect("the store answers");
  if accepted {
    assert_eq!(response.status(), StatusCode::NO_CONTENT, "`{authorization}`");
    let Received::RevokeApiKey(request) = last_received(&petstore) else {
      panic!("the service did not receive revokeApiKey");
    };
    assert_eq!(
      exposed(request.credentials.bearer_auth.as_ref()),
      Some(STAFF_TOKEN),
      "`{authorization}`"
    );
  } else {
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED, "`{authorization}`");
    assert!(petstore.service.received().is_empty(), "`{authorization}`");
  }
}

#[rstest]
#[case::bearer_token(Credentials::NONE.with_bearer_auth(STAFF_TOKEN), AuditScheme::Bearer)]
#[case::token(Credentials::NONE.with_token_auth(AUDIT_TOKEN), AuditScheme::Token)]
#[case::both_sends_the_token(
  Credentials::NONE.with_bearer_auth(STAFF_TOKEN).with_token_auth(AUDIT_TOKEN),
  AuditScheme::Token
)]
#[tokio::test]
async fn list_audit_events_accepts_a_bearer_token_or_a_raw_authorization_key(
  #[future(awt)] petstore: Petstore,
  #[case] credentials: Credentials,
  #[case] scheme: AuditScheme,
) {
  let response = credentials
    .client(&petstore)
    .list_audit_events(ListAuditEventsRequest {})
    .await
    .expect("the store answers");
  let ApiResponse::Ok(events) = response else {
    panic!("{credentials:?} must list audit events, got {response:?}");
  };
  let Received::ListAuditEvents(request) = last_received(&petstore) else {
    panic!("the service did not receive listAuditEvents");
  };
  let bearer_auth = exposed(request.credentials.bearer_auth.as_ref());
  let token_auth = exposed(request.credentials.token_auth.as_ref());
  match scheme {
    AuditScheme::Bearer => {
      assert_eq!(events.first().map(String::as_str), Some("authenticated by bearer"));
      assert_eq!(bearer_auth, Some(STAFF_TOKEN));
      assert_eq!(token_auth, Some(format!("Bearer {STAFF_TOKEN}").as_str()));
    }
    AuditScheme::Token => {
      assert_eq!(events.first().map(String::as_str), Some("authenticated by token"));
      assert_eq!(bearer_auth, None);
      assert_eq!(token_auth, Some(AUDIT_TOKEN));
    }
  }
}

#[tokio::test]
async fn a_client_holding_both_audit_credentials_sends_a_single_authorization_header() {
  let wire_log = Arc::new(Mutex::new(Vec::<HeaderMap>::new()));
  let handler_log = Arc::clone(&wire_log);
  let router = Router::new().route(
    "/staff/audit",
    get(async move |headers: HeaderMap| {
      handler_log
        .lock()
        .expect("the wire log is never poisoned")
        .push(headers);
      Json(Vec::<String>::new())
    }),
  );
  let base_url = support::serve(router).await;
  let client = PetstoreClient::with_base_url(&base_url)
    .expect("the base URL parses")
    .with_bearer_auth(STAFF_TOKEN)
    .with_token_auth(AUDIT_TOKEN);

  let response = client
    .list_audit_events(ListAuditEventsRequest {})
    .await
    .expect("the recording endpoint answers");
  assert!(matches!(response, ApiResponse::Ok(_)), "{response:?}");

  let wire_log = wire_log.lock().expect("the wire log is never poisoned");
  let [headers] = wire_log.as_slice() else {
    panic!("expected one request, got {}", wire_log.len());
  };
  let authorization = headers
    .get_all(AUTHORIZATION)
    .iter()
    .map(|value| value.to_str().expect("an ASCII header"))
    .collect::<Vec<_>>();
  assert_eq!(authorization, [AUDIT_TOKEN]);
}

#[rstest]
#[case::no_credentials(Credentials::NONE)]
#[case::api_key(Credentials::NONE.with_api_key_auth(API_KEY))]
#[tokio::test]
async fn replace_api_key_leaves_credentials_optional_because_basic_auth_is_unsupported(
  #[future(awt)] petstore: Petstore,
  #[case] credentials: Credentials,
) {
  seed_api_key(&petstore);
  let response = credentials
    .client(&petstore)
    .replace_api_key(replace_seeded_key_request())
    .await
    .expect("the store answers");
  let ApiResponse::Ok(key) = response else {
    panic!("{credentials:?} must replace the key, got {response:?}");
  };
  assert_eq!((key.id.as_str(), key.name.as_str()), (SEEDED_KEY_ID, "rotated"));
  let Received::ReplaceApiKey(request) = last_received(&petstore) else {
    panic!("the service did not receive replaceApiKey");
  };
  assert_eq!(
    exposed(request.credentials.api_key_auth.as_ref()),
    credentials.api_key_auth
  );
}

#[rstest]
#[tokio::test]
async fn replace_api_key_ignores_basic_credentials(#[future(awt)] petstore: Petstore) {
  seed_api_key(&petstore);
  let response = reqwest::Client::new()
    .put(petstore.url(&format!("/staff/keys/{SEEDED_KEY_ID}")))
    .basic_auth("staff", Some("hunter2"))
    .header(CONTENT_TYPE, "application/json")
    .body(NEW_API_KEY_JSON)
    .send()
    .await
    .expect("the store answers");
  assert_eq!(response.status(), StatusCode::OK);
  let Received::ReplaceApiKey(request) = last_received(&petstore) else {
    panic!("the service did not receive replaceApiKey");
  };
  assert!(request.credentials.api_key_auth.is_none());
}

#[rstest]
#[case::anonymous(Credentials::NONE)]
#[case::with_an_api_key(Credentials::NONE.with_api_key_auth(API_KEY))]
#[tokio::test]
async fn adopt_pet_leaves_credentials_optional_because_oauth_and_openid_connect_are_unsupported(
  #[future(awt)] petstore: Petstore,
  #[case] credentials: Credentials,
) {
  let pet_id = petstore.service.insert_pet(server::Pet {
    id: Uuid::from_u128(2),
    name: "Biscuit".to_string(),
    species: server::Species::Dog,
    ..Default::default()
  });
  let application = AdoptionRequest {
    pet_id,
    adopter: Adopter {
      name: "Ada".to_string(),
      email: "ada@example.com".to_string(),
      ..Default::default()
    },
    ..Default::default()
  };
  let request = AdoptPetRequest::builder()
    .body(application)
    .build()
    .expect("the application is valid");
  let response = credentials
    .client(&petstore)
    .adopt_pet(request)
    .await
    .expect("the store answers");
  let ApiResponse::Accepted(WithHeaders {
    headers: LocationHeaders { location },
    body: adoption,
  }) = response
  else {
    panic!("{credentials:?} must adopt, got {response:?}");
  };
  assert_eq!(adoption.pet_id, pet_id);
  assert_eq!(location, format!("/adoptions/{}", adoption.id));
  let Received::AdoptPet(request) = last_received(&petstore) else {
    panic!("the service did not receive adoptPet");
  };
  assert_eq!(
    exposed(request.credentials.api_key_auth.as_ref()),
    credentials.api_key_auth
  );
}

#[rstest]
#[case::create_pet_body_without_a_key(
  Method::POST,
  "/pets",
  Credentials::NONE,
  Some(MALFORMED_JSON),
  StatusCode::UNAUTHORIZED
)]
#[case::create_pet_body_with_a_key(
  Method::POST,
  "/pets",
  Credentials::NONE.with_api_key_auth(API_KEY),
  Some(MALFORMED_JSON),
  StatusCode::BAD_REQUEST
)]
#[case::create_api_key_body_without_credentials(
  Method::POST,
  "/staff/keys",
  Credentials::NONE,
  Some(MALFORMED_JSON),
  StatusCode::UNAUTHORIZED
)]
#[case::create_api_key_body_with_a_session(
  Method::POST,
  "/staff/keys",
  Credentials::NONE.with_session_cookie(SESSION),
  Some(MALFORMED_JSON),
  StatusCode::BAD_REQUEST
)]
#[case::list_api_keys_limit_without_a_key(
  Method::GET,
  "/staff/keys?limit=many",
  Credentials::NONE,
  None,
  StatusCode::UNAUTHORIZED
)]
#[case::list_api_keys_limit_with_a_key(
  Method::GET,
  "/staff/keys?limit=many",
  Credentials::NONE.with_api_key_auth(API_KEY),
  None,
  StatusCode::BAD_REQUEST
)]
#[case::replace_api_key_body_with_optional_credentials(
  Method::PUT,
  "/staff/keys/key_seeded",
  Credentials::NONE,
  Some(MALFORMED_JSON),
  StatusCode::BAD_REQUEST
)]
#[tokio::test]
async fn credentials_are_checked_before_the_body_or_query_is_parsed(
  #[future(awt)] petstore: Petstore,
  #[case] method: Method,
  #[case] path: &str,
  #[case] credentials: Credentials,
  #[case] json_body: Option<&str>,
  #[case] expected: StatusCode,
) {
  let target = format!("{method} {path} with {credentials:?}");
  let status = send_raw(&petstore, method, path, credentials, json_body).await.status();
  assert_eq!(status, expected, "{target}");
  assert!(petstore.service.received().is_empty(), "the service ran for {target}");
}

#[rstest]
#[case::api_key(API_KEY)]
#[case::bearer_token(STAFF_TOKEN)]
#[case::query_key(QUERY_KEY)]
#[case::session_cookie(SESSION)]
#[case::token(AUDIT_TOKEN)]
#[tokio::test]
async fn debug_output_redacts_every_secret(#[future(awt)] petstore: Petstore, #[case] secret: &str) {
  seed_api_key(&petstore);
  let client = Credentials::NONE
    .with_api_key_auth(API_KEY)
    .with_bearer_auth(STAFF_TOKEN)
    .with_query_key(QUERY_KEY)
    .with_session_cookie(SESSION)
    .with_token_auth(AUDIT_TOKEN)
    .client(&petstore);

  let client_debug = format!("{client:?}");
  assert!(client_debug.contains("[REDACTED]"), "{client_debug}");
  assert!(
    !client_debug.contains(secret),
    "the client's Debug leaks `{secret}`: {client_debug}"
  );

  let created = client.create_api_key(create_api_key_request()).await;
  assert!(matches!(created, Ok(ApiResponse::Created(_))), "{created:?}");
  let read = client.get_api_key(get_seeded_key_request()).await;
  assert!(matches!(read, Ok(ApiResponse::Ok(_))), "{read:?}");
  let audited = client.list_audit_events(ListAuditEventsRequest {}).await;
  assert!(matches!(audited, Ok(ApiResponse::Ok(_))), "{audited:?}");
  let revoked = client.revoke_api_key(revoke_seeded_key_request()).await;
  assert!(matches!(revoked, Ok(ApiResponse::NoContent(()))), "{revoked:?}");

  let server_debug = format!("{:?}", petstore.service.received());
  assert!(server_debug.contains("[REDACTED]"), "{server_debug}");
  assert!(
    !server_debug.contains(secret),
    "the server credentials' Debug leaks `{secret}`: {server_debug}"
  );
}

/// Cases per property. Failures are reported with their minimal input instead of being saved beside the source.
fn property_config() -> ProptestConfig {
  ProptestConfig {
    failure_persistence: None,
    ..ProptestConfig::with_cases(48)
  }
}

prop_compose! {
  /// A cookie value of RFC 6265 cookie-octets without `%`, so the server decodes it to itself.
  fn session_cookie_value()(
    chunks in prop::collection::vec("[!#$&'()*+\\-./0-9:<=>?@A-Z\\[\\]^_`a-z{|}~]{1,8}", 1..8)
  ) -> String {
    chunks.concat()
  }
}

#[test]
fn any_header_safe_api_key_arrives_byte_for_byte() {
  let petstore = start_shared();
  proptest!(property_config(), |(api_key in "[!-~]{1,64}")| {
    run_case(async || {
      let response = petstore
        .anonymous()
        .with_api_key_auth(api_key.clone())
        .get_inventory(GetInventoryRequest {})
        .await;
      prop_assert!(matches!(response, Ok(ApiResponse::Ok(_))), "{response:?}");
      let Received::GetInventory(request) = last_received(&petstore) else {
        return Err(TestCaseError::fail("the service did not receive getInventory"));
      };
      prop_assert_eq!(request.credentials.api_key_auth.expose_secret(), api_key.as_str());
      Ok::<_, TestCaseError>(())
    })?;
  });
}

#[test]
fn any_bearer_token_arrives_byte_for_byte() {
  let petstore = start_shared();
  proptest!(property_config(), |(token in "[A-Za-z0-9._~+/\\-]{1,64}={0,2}")| {
    run_case(async || {
      let response = petstore
        .anonymous()
        .with_bearer_auth(token.clone())
        .list_audit_events(ListAuditEventsRequest {})
        .await;
      prop_assert!(response.is_ok(), "{response:?}");
      let Received::ListAuditEvents(request) = last_received(&petstore) else {
        return Err(TestCaseError::fail("the service did not receive listAuditEvents"));
      };
      let authorization = format!("Bearer {token}");
      prop_assert_eq!(exposed(request.credentials.bearer_auth.as_ref()), Some(token.as_str()));
      prop_assert_eq!(exposed(request.credentials.token_auth.as_ref()), Some(authorization.as_str()));
      Ok::<_, TestCaseError>(())
    })?;
  });
}

#[test]
fn any_session_cookie_arrives_byte_for_byte() {
  let petstore = start_shared();
  proptest!(property_config(), |(session in session_cookie_value())| {
    run_case(async || {
      let response = petstore
        .anonymous()
        .with_session_cookie(session.clone())
        .create_api_key(create_api_key_request())
        .await;
      prop_assert!(response.is_ok(), "{response:?}");
      let Received::CreateApiKey(request) = last_received(&petstore) else {
        return Err(TestCaseError::fail("the service did not receive createApiKey"));
      };
      prop_assert_eq!(exposed(request.credentials.session_cookie.as_ref()), Some(session.as_str()));
      Ok::<_, TestCaseError>(())
    })?;
  });
}

#[test]
fn any_query_key_arrives_byte_for_byte() {
  let petstore = start_shared();
  proptest!(property_config(), |(api_key in "\\PC{1,32}")| {
    run_case(async || {
      let response = petstore
        .anonymous()
        .with_query_key(api_key.clone())
        .create_api_key(create_api_key_request())
        .await;
      prop_assert!(response.is_ok(), "{response:?}");
      let Received::CreateApiKey(request) = last_received(&petstore) else {
        return Err(TestCaseError::fail("the service did not receive createApiKey"));
      };
      prop_assert_eq!(exposed(request.credentials.query_key.as_ref()), Some(api_key.as_str()));
      Ok::<_, TestCaseError>(())
    })?;
  });
}

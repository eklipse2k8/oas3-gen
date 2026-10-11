use petstore_client::{self as client, PetstoreClient};
use proptest::prelude::*;
use rstest::rstest;
use secrecy::ExposeSecret;
use uuid::Uuid;

use crate::{
  Wiretap, checked, property_config,
  support::{COMPATIBILITY_DATE, Petstore, PetstoreService, run_case, start_shared},
};

const API_KEY: &str = "pk/live=key:1";
const BEARER: &str = "staff.token/2026==";
const QUERY_KEY: &str = "q key&=1";
const SESSION: &str = "dGVzdC9rZXk/Pw==";
const TOKEN: &str = "audit token 2026";

fn credentialed(base_url: &str) -> anyhow::Result<PetstoreClient> {
  Ok(
    PetstoreClient::with_base_url(base_url)?
      .with_api_key_auth(API_KEY)
      .with_bearer_auth(BEARER)
      .with_query_key(QUERY_KEY)
      .with_session_cookie(SESSION)
      .with_token_auth(TOKEN),
  )
}

async fn call_every_secured_shape(client: &PetstoreClient) -> anyhow::Result<()> {
  let key = || client::NewApiKey {
    name: "ci".to_string(),
    value: "sk_ci".to_string(),
  };
  let key_id = "key_1".to_string();
  client.describe_pets(client::DescribePetsRequest {}).await?;
  let signup = client::NewsletterSignup {
    email: "ada@example.com".to_string(),
    ..Default::default()
  };
  client
    .subscribe_newsletter(client::SubscribeNewsletterRequest::builder().body(signup).build()?)
    .await?;
  client
    .list_pets(
      client::ListPetsRequest::builder()
        .x_compatibility_date(COMPATIBILITY_DATE)
        .build()?,
    )
    .await?;
  client.get_inventory(client::GetInventoryRequest {}).await?;
  client
    .delete_pet(client::DeletePetRequest::builder().pet_id(Uuid::nil()).build()?)
    .await?;
  client
    .create_api_key(client::CreateApiKeyRequest::builder().body(key()).build()?)
    .await?;
  client
    .get_api_key(client::GetApiKeyRequest::builder().key_id(key_id.clone()).build()?)
    .await?;
  client
    .replace_api_key(
      client::ReplaceApiKeyRequest::builder()
        .key_id(key_id)
        .body(key())
        .build()?,
    )
    .await?;
  client.list_audit_events(client::ListAuditEventsRequest {}).await?;
  let application = client::AdoptionRequest {
    adopter: client::Adopter {
      name: "Ada".to_string(),
      email: "ada@example.com".to_string(),
      ..Default::default()
    },
    ..Default::default()
  };
  client
    .adopt_pet(client::AdoptPetRequest::builder().body(application).build()?)
    .await?;
  Ok(())
}

#[rstest]
#[tokio::test]
async fn each_operation_sends_only_the_credentials_its_security_names() -> anyhow::Result<()> {
  let router = axum::Router::new().fallback(|| async { (http::StatusCode::IM_A_TEAPOT, "captured") });
  let wiretap = Wiretap::serve(router, PetstoreService::default()).await?;
  let client = credentialed(&wiretap.petstore.base_url)?;

  call_every_secured_shape(&client).await?;

  let requests = wiretap.requests();
  let sent = requests
    .iter()
    .map(|request| {
      (
        format!("{} {}", request.method, request.uri.path()),
        request.header("x-api-key"),
        request.header(http::header::AUTHORIZATION),
        request.uri.query(),
      )
    })
    .collect::<Vec<_>>();
  let expected = [
    ("OPTIONS /pets", None, None, None),
    ("POST /store/newsletter", None, None, None),
    ("GET /pets", Some(API_KEY), None, None),
    ("GET /store/inventory", Some(API_KEY), None, None),
    (
      "DELETE /pets/00000000-0000-0000-0000-000000000000",
      Some(API_KEY),
      Some("Bearer staff.token/2026=="),
      None,
    ),
    ("POST /staff/keys", None, None, Some("api_key=q+key%26%3D1")),
    ("GET /staff/keys/key_1", Some(API_KEY), None, None),
    ("PUT /staff/keys/key_1", Some(API_KEY), None, None),
    ("GET /staff/audit", None, Some(TOKEN), None),
    ("POST /adoptions", Some(API_KEY), None, None),
  ]
  .map(|(target, api_key, authorization, query)| (target.to_string(), api_key, authorization, query));
  assert_eq!(sent, expected);
  let cookie = format!("session={SESSION}");
  assert!(
    requests
      .iter()
      .all(|request| request.header(http::header::COOKIE) == Some(cookie.as_str())),
    "the session cookie travels verbatim with every request: {requests:?}"
  );
  let debug = format!("{client:?}");
  for secret in [API_KEY, BEARER, QUERY_KEY, SESSION, TOKEN] {
    assert!(!debug.contains(secret), "Debug shows `{secret}`: {debug}");
  }
  Ok(())
}

#[derive(Debug)]
struct Secrets {
  api_key: String,
  bearer: String,
  query_key: String,
  session: String,
}

async fn secrets_arrive(petstore: &Petstore, secrets: Secrets) -> anyhow::Result<()> {
  petstore
    .anonymous()
    .with_api_key_auth(secrets.api_key.clone())
    .get_inventory(client::GetInventoryRequest {})
    .await?;
  let received = received!(petstore.service, GetInventory).credentials;
  anyhow::ensure!(received.api_key_auth.expose_secret() == secrets.api_key);

  petstore
    .anonymous()
    .with_bearer_auth(secrets.bearer.clone())
    .list_audit_events(client::ListAuditEventsRequest {})
    .await?;
  let received = received!(petstore.service, ListAuditEvents).credentials;
  anyhow::ensure!(received.bearer_auth.as_ref().map(ExposeSecret::expose_secret) == Some(secrets.bearer.as_str()));
  let authorization = format!("Bearer {}", secrets.bearer);
  anyhow::ensure!(received.token_auth.as_ref().map(ExposeSecret::expose_secret) == Some(authorization.as_str()));

  let key = client::NewApiKey {
    name: "ci".to_string(),
    value: "sk_ci".to_string(),
  };
  petstore
    .anonymous()
    .with_query_key(secrets.query_key.clone())
    .with_session_cookie(secrets.session.clone())
    .create_api_key(client::CreateApiKeyRequest::builder().body(key).build()?)
    .await?;
  let received = received!(petstore.service, CreateApiKey).credentials;
  anyhow::ensure!(received.query_key.as_ref().map(ExposeSecret::expose_secret) == Some(secrets.query_key.as_str()));
  anyhow::ensure!(received.session_cookie.as_ref().map(ExposeSecret::expose_secret) == Some(secrets.session.as_str()));
  Ok(())
}

prop_compose! {
  fn secrets()(
    api_key in "[!-~]{1,64}",
    bearer in "[A-Za-z0-9._~+/\\-]{1,64}={0,2}",
    query_key in "\\PC{1,32}",
    session in "[!#$&'()*+\\-./0-9:<=>?@A-Z\\[\\]^_`a-z{|}~]{1,32}",
  ) -> Secrets {
    Secrets { api_key, bearer, query_key, session }
  }
}

#[rstest]
fn any_credential_arrives_byte_for_byte() {
  let petstore = start_shared();
  proptest!(property_config(48), |(generated in secrets())| {
    checked(run_case(async || secrets_arrive(&petstore, generated).await))?;
  });
}

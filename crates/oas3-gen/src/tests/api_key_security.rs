use std::future::{Future, ready};

use secrecy::{ExposeSecret as _, SecretString};

use crate::fixtures::{api_key_security as client, api_key_security_server as server};

#[derive(Clone)]
struct StubVault;

impl server::ApiServer for StubVault {
  fn list_secrets(
    &self,
    request: server::ListSecretsRequest,
  ) -> impl Future<Output = anyhow::Result<server::ApiResponse<server::Secret200Response, server::Error>>> + Send {
    ready(Ok(server::ApiResponse::Ok(vec![server::Secret {
      id: request.credentials.api_key_auth.expose_secret().to_string(),
      name: "listed".to_string(),
    }])))
  }

  fn create_secret(
    &self,
    request: server::CreateSecretRequest,
  ) -> impl Future<Output = anyhow::Result<server::ApiResponse<server::Secret, server::Error>>> + Send {
    let credentials = request.credentials;
    ready(Ok(server::ApiResponse::Created(server::Secret {
      id: format!(
        "{:?}|{:?}",
        credentials.query_key.as_ref().map(secrecy::ExposeSecret::expose_secret),
        credentials
          .session_cookie
          .as_ref()
          .map(secrecy::ExposeSecret::expose_secret)
      ),
      name: request.body.name,
    })))
  }

  fn get_secret(
    &self,
    request: server::GetSecretRequest,
  ) -> impl Future<Output = anyhow::Result<server::ApiResponse<server::Secret, server::Error>>> + Send {
    let credentials = request.credentials;
    ready(Ok(server::ApiResponse::Ok(server::Secret {
      id: format!(
        "{}|{}",
        credentials.api_key_auth.expose_secret(),
        credentials.session_cookie.expose_secret()
      ),
      name: request.path.secret_id,
    })))
  }

  fn replace_secret(
    &self,
    request: server::ReplaceSecretRequest,
  ) -> impl Future<Output = anyhow::Result<server::ApiResponse<server::Secret, server::Error>>> + Send {
    ready(Ok(server::ApiResponse::Ok(server::Secret {
      id: format!(
        "{:?}",
        request
          .credentials
          .api_key_auth
          .as_ref()
          .map(secrecy::ExposeSecret::expose_secret)
      ),
      name: request.body.name,
    })))
  }

  fn delete_secret(
    &self,
    request: server::DeleteSecretRequest,
  ) -> impl Future<Output = anyhow::Result<server::ApiResponse<(), server::Error>>> + Send {
    let credentials = request.credentials;
    let recognized = credentials
      .bearer_auth
      .as_ref()
      .is_some_and(|token| token.expose_secret() == "t1")
      || credentials
        .api_key_auth
        .as_ref()
        .is_some_and(|key| key.expose_secret() == "k1");
    ready(Ok(if recognized {
      server::ApiResponse::NoContent
    } else {
      server::ApiResponse::Unauthorized(server::Error {
        code: "invalid_credentials".to_string(),
        message: "The credentials are not recognized.".to_string(),
      })
    }))
  }

  fn list_audit_events(
    &self,
    request: server::ListAuditEventsRequest,
  ) -> impl Future<Output = anyhow::Result<server::ApiResponse<Vec<String>, ()>>> + Send {
    let credentials = request.credentials;
    let received = [&credentials.bearer_auth, &credentials.token_auth]
      .map(|credential| format!("{:?}", credential.as_ref().map(secrecy::ExposeSecret::expose_secret)));
    ready(Ok(server::ApiResponse::Ok(received.to_vec())))
  }

  fn get_health(
    &self,
    _request: server::GetHealthRequest,
  ) -> impl Future<Output = anyhow::Result<server::ApiResponse<(), ()>>> + Send {
    ready(Ok(server::ApiResponse::NoContent))
  }
}

async fn serve_vault() -> String {
  let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
    .await
    .expect("bind a free port");
  let address = listener.local_addr().expect("listener address");
  tokio::spawn(async move { axum::serve(listener, server::router(StubVault)).await });
  format!("http://{address}")
}

async fn status_and_text(request: reqwest::RequestBuilder) -> (reqwest::StatusCode, String) {
  let response = request.send().await.unwrap();
  (response.status(), response.text().await.unwrap())
}

#[tokio::test]
async fn header_key_from_global_security_is_required() {
  let base_url = serve_vault().await;
  let keyed = client::KeyVaultClient::with_base_url(&base_url)
    .unwrap()
    .with_api_key_auth("k1");

  let refused = status_and_text(reqwest::Client::new().get(format!("{base_url}/secrets"))).await;
  assert_eq!(
    refused,
    (
      reqwest::StatusCode::UNAUTHORIZED,
      "missing API key in the `X-Api-Key` header".to_string()
    )
  );

  match keyed
    .list_secrets(client::ListSecretsRequest::builder().build().unwrap())
    .await
    .unwrap()
  {
    client::ApiResponse::Ok(secrets) => assert_eq!(secrets[0].id, "k1", "server should receive the header key"),
    other => panic!("expected 200, got {other:?}"),
  }
}

#[tokio::test]
async fn alternative_requirements_accept_either_key() {
  let base_url = serve_vault().await;
  let anonymous = || client::KeyVaultClient::with_base_url(&base_url).unwrap();
  let cases = [
    (anonymous().with_query_key("q"), r#"Some("q")|None"#),
    (anonymous().with_session_cookie("c"), r#"None|Some("c")"#),
    (
      anonymous().with_query_key("q").with_session_cookie("c"),
      r#"Some("q")|Some("c")"#,
    ),
  ];
  let request = || {
    client::CreateSecretRequest::builder()
      .body(client::NewSecret {
        name: "db".to_string(),
        value: "hunter2".to_string(),
      })
      .build()
      .unwrap()
  };

  for (keyed, expected) in cases {
    match keyed.create_secret(request()).await.unwrap() {
      client::ApiResponse::Created(secret) => assert_eq!(secret.id, expected, "credentials received"),
      other => panic!("expected 201 for {expected}, got {other:?}"),
    }
  }

  let refused = status_and_text(
    reqwest::Client::new()
      .post(format!("{base_url}/secrets"))
      .json(&serde_json::json!({ "name": "db", "value": "hunter2" })),
  )
  .await;
  assert_eq!(
    refused,
    (
      reqwest::StatusCode::UNAUTHORIZED,
      "missing credentials for `QueryKey` or `SessionCookie`".to_string()
    )
  );
}

#[tokio::test]
async fn combined_requirement_needs_every_key() {
  let base_url = serve_vault().await;
  let both = client::KeyVaultClient::with_base_url(&base_url)
    .unwrap()
    .with_api_key_auth("k1")
    .with_session_cookie("\"c\"");

  let refused = status_and_text(
    reqwest::Client::new()
      .get(format!("{base_url}/secrets/s1"))
      .header("x-api-key", "k1"),
  )
  .await;
  assert_eq!(
    refused,
    (
      reqwest::StatusCode::UNAUTHORIZED,
      "missing API key in the `session` cookie".to_string()
    )
  );

  let request = client::GetSecretRequest::builder()
    .secret_id("s1".to_string())
    .build()
    .unwrap();
  match both.get_secret(request).await.unwrap() {
    client::ApiResponse::Ok(secret) => assert_eq!((secret.id.as_str(), secret.name.as_str()), ("k1|c", "s1")),
    other => panic!("expected 200, got {other:?}"),
  }
}

#[tokio::test]
async fn bearer_token_or_header_key_is_required() {
  let base_url = serve_vault().await;
  let anonymous = || client::KeyVaultClient::with_base_url(&base_url).unwrap();
  let request = || {
    client::DeleteSecretRequest::builder()
      .secret_id("s1".to_string())
      .build()
      .unwrap()
  };
  let cases = [
    (anonymous().with_bearer_auth("t1"), true, "bearer token"),
    (anonymous().with_api_key_auth("k1"), true, "header key"),
    (anonymous().with_bearer_auth("t2"), false, "unrecognized bearer token"),
  ];

  for (keyed, recognized, label) in cases {
    let deleted = keyed.delete_secret(request()).await.unwrap();
    assert_eq!(
      matches!(deleted, client::ApiResponse::NoContent),
      recognized,
      "{label}: {deleted:?}"
    );
  }

  let http = reqwest::Client::new();
  let url = format!("{base_url}/secrets/s1");
  let missing = "missing credentials for `ApiKeyAuth` or `BearerAuth`";
  let cases = [
    (http.delete(&url), reqwest::StatusCode::UNAUTHORIZED, missing),
    (
      http.delete(&url).header("authorization", "Basic dDE="),
      reqwest::StatusCode::UNAUTHORIZED,
      missing,
    ),
    (
      http.delete(&url).header("authorization", "bearer t1"),
      reqwest::StatusCode::NO_CONTENT,
      "",
    ),
    (
      http.delete(&url).header("authorization", "Bearer   t1"),
      reqwest::StatusCode::NO_CONTENT,
      "",
    ),
  ];

  for (request, status, text) in cases {
    assert_eq!(status_and_text(request).await, (status, text.to_string()));
  }
}

#[tokio::test]
async fn one_authorization_header_carries_the_last_credential() {
  let base_url = serve_vault().await;
  let anonymous = || client::KeyVaultClient::with_base_url(&base_url).unwrap();
  let cases = [
    (
      anonymous().with_bearer_auth("t1"),
      [r#"Some("t1")"#, r#"Some("Bearer t1")"#],
    ),
    (anonymous().with_token_auth("a1"), ["None", r#"Some("a1")"#]),
    (
      anonymous().with_bearer_auth("t1").with_token_auth("a1"),
      ["None", r#"Some("a1")"#],
    ),
  ];

  for (keyed, expected) in cases {
    match keyed
      .list_audit_events(client::ListAuditEventsRequest {})
      .await
      .unwrap()
    {
      client::ApiResponse::Ok(received) => assert_eq!(received, expected, "credentials received"),
      other => panic!("expected 200 for {expected:?}, got {other:?}"),
    }
  }

  let refused = status_and_text(reqwest::Client::new().get(format!("{base_url}/audit"))).await;
  assert_eq!(
    refused,
    (
      reqwest::StatusCode::UNAUTHORIZED,
      "missing credentials for `BearerAuth` or `TokenAuth`".to_string()
    )
  );
}

#[tokio::test]
async fn unchecked_alternatives_and_public_operations_need_no_key() {
  let base_url = serve_vault().await;
  let anonymous = client::KeyVaultClient::with_base_url(&base_url).unwrap();

  let replaced = anonymous
    .replace_secret(
      client::ReplaceSecretRequest::builder()
        .secret_id("s1".to_string())
        .body(client::NewSecret {
          name: "db".to_string(),
          value: "hunter2".to_string(),
        })
        .build()
        .unwrap(),
    )
    .await
    .unwrap();
  match replaced {
    client::ApiResponse::Ok(secret) => assert_eq!(
      secret.id, "None",
      "a basic alternative the server can't check should not require the API key"
    ),
    other => panic!("expected 200, got {other:?}"),
  }

  let health = anonymous.get_health(client::GetHealthRequest {}).await.unwrap();
  assert!(
    matches!(health, client::ApiResponse::NoContent),
    "an operation with `security: []` should not require credentials: {health:?}"
  );
}

#[tokio::test]
async fn credentials_are_checked_before_the_body() {
  let base_url = serve_vault().await;
  let http = reqwest::Client::new();
  let cases = [
    (
      http
        .post(format!("{base_url}/secrets"))
        .header("content-type", "application/json")
        .body("{"),
      reqwest::StatusCode::UNAUTHORIZED,
    ),
    (
      http
        .post(format!("{base_url}/secrets?api_key=q"))
        .header("content-type", "application/json")
        .body("{"),
      reqwest::StatusCode::BAD_REQUEST,
    ),
    (
      http
        .get(format!("{base_url}/secrets?limit=many"))
        .header("x-api-key", "k1"),
      reqwest::StatusCode::BAD_REQUEST,
    ),
  ];

  for (request, expected) in cases {
    let (status, text) = status_and_text(request).await;
    assert_eq!(status, expected, "axum rejection: {text}");
  }
}

#[test]
fn debug_output_hides_credentials() {
  let client = client::KeyVaultClient::with_base_url("http://localhost")
    .unwrap()
    .with_api_key_auth("sk_live_1")
    .with_session_cookie("sk_live_4")
    .with_bearer_auth("sk_live_5");
  let credentials = server::ApiKeyAuthAndSessionCookieCredentials {
    api_key_auth: SecretString::from("sk_live_2"),
    session_cookie: SecretString::from("sk_live_3"),
  };

  for debug in [format!("{client:?}"), format!("{credentials:?}")] {
    assert!(!debug.contains("sk_live"), "secret leaked: {debug}");
    assert!(debug.contains("[REDACTED]"), "secret placeholder missing: {debug}");
  }
}

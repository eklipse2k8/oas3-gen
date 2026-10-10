use futures::future::BoxFuture;
use petstore_client::{self as client, ApiResponse, ErrorOrProblem};
use rstest::rstest;

type Parse = fn(reqwest::Response) -> BoxFuture<'static, anyhow::Result<()>>;

macro_rules! parses_as {
  ($request:ident, $pattern:pat $(if $guard:expr)?) => {
    |response: reqwest::Response| -> BoxFuture<'static, anyhow::Result<()>> {
      Box::pin(async move {
        answered!(client::$request::parse_response(response).await?, $pattern $(if $guard)? => ());
        Ok(())
      })
    }
  };
}

const ERROR: &str = r#"{"code":418,"message":"short and stout"}"#;
const JSON: (&str, &str) = ("content-type", "application/json");

fn shipped_order() -> client::Order {
  client::Order {
    id: Some(9_007_199_254_740_993),
    pet_id: uuid::Uuid::from_u128(0x8f3c_2a4e_1b2c_4d5e_8f90_1234_5678_9abc),
    quantity: Some(1),
    ship_date: chrono::DateTime::from_timestamp_millis(1_791_570_600_250),
    status: None,
    complete: Some(false),
  }
}

fn misstated_preference() -> client::ChatPreference {
  client::ChatPreference {
    model: client::ChatModelId::default(),
    object: Some("chat.thread".to_string()),
    fallback: Some(client::ChatModel::Chat11),
    tier: Some(client::ChatPreferenceTier::Other2("flex".to_string())),
  }
}

#[rstest]
#[case::bodiless_status(304, &[], "", parses_as!(ShowPetByIdRequest, ApiResponse::NotModified))]
#[case::problem_document(
  404,
  &[("content-type", "application/problem+json")],
  r#"{"status":404,"title":"Not Found"}"#,
  parses_as!(ShowPetByIdRequest, ApiResponse::NotFound(ErrorOrProblem::Problem(problem)) if problem.status == Some(404))
)]
#[case::status_range(
  418,
  &[JSON],
  ERROR,
  parses_as!(
    ShowPetByIdRequest,
    ApiResponse::ClientError(status, ErrorOrProblem::Error(error)) if status == http::StatusCode::IM_A_TEAPOT && error.code == 418
  )
)]
#[case::default_response(
  503,
  &[JSON],
  ERROR,
  parses_as!(ListPetsRequest, ApiResponse::Unknown(status, _) if status == http::StatusCode::SERVICE_UNAVAILABLE)
)]
#[case::declared_status_in_an_undeclared_media_type(
  401,
  &[("content-type", "text/plain; charset=utf-8")],
  "missing `QueryKey`",
  parses_as!(
    CreateApiKeyRequest,
    ApiResponse::Other(response) if response.status() == http::StatusCode::UNAUTHORIZED
  )
)]
#[case::undeclared_status(
  401,
  &[JSON],
  ERROR,
  parses_as!(DeletePetRequest, ApiResponse::Other(response) if response.status() == http::StatusCode::UNAUTHORIZED)
)]
#[case::formats_absent_members_and_precision(
  200,
  &[JSON],
  r#"{"id":9007199254740993,"petId":"8F3C2A4E-1B2C-4D5E-8F90-123456789ABC","shipDate":"2026-10-09T20:30:00.25+02:00"}"#,
  parses_as!(GetOrderByIdRequest, ApiResponse::Ok(order) if order == shipped_order())
)]
#[case::null_and_misstated_constant_members(
  200,
  &[JSON],
  r#"{"model":null,"object":"chat.thread"}"#,
  parses_as!(GetChatPreferenceRequest, ApiResponse::Ok(preference) if preference == misstated_preference())
)]
#[tokio::test]
async fn responses_decode_into_the_variant_their_status_and_media_type_declare(
  #[case] status: u16,
  #[case] headers: &[(&str, &str)],
  #[case] body: &'static str,
  #[case] parse: Parse,
) -> anyhow::Result<()> {
  let response = headers
    .iter()
    .fold(http::Response::builder().status(status), |builder, (name, value)| {
      builder.header(*name, *value)
    })
    .body(body)?;

  parse(reqwest::Response::from(response)).await
}

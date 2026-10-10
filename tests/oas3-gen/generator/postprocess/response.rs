use oas3_gen::generator::GenerationTarget;
use rstest::rstest;

use super::GeneratedFile;

#[rstest]
fn response_types_are_shared_and_named_around_schemas() -> anyhow::Result<()> {
  let generated = GeneratedFile::types(spec!("postprocess/response_names.json"), GenerationTarget::Client)?;

  assert_eq!(
    generated.response_types(),
    [
      "GetUserRequest -> ApiResponseType<User,BasicErrorOrValidationError>",
      "GetTeamRequest -> ApiResponseType<WithHeaders,BasicErrorOrValidationError>",
      "ListPetsRequest -> ApiResponseType<WithHeaders2<LinkHeaders,ApiResponse>,()>",
      "ListCatsRequest -> ApiResponseType<WithHeaders2<LinkHeaders,User>,()>",
      "CountPetsRequest -> ApiResponseType<WithHeaders2<LinkHeaders2,i64>,()>",
    ]
  );
  assert_eq!(
    generated.variants("BasicErrorOrValidationError")?,
    ["BasicError(BasicError)", "ValidationError(ValidationError)"]
  );
  Ok(())
}

#[rstest]
fn exact_statuses_dispatch_before_ranges_and_default_reads_every_media_type() -> anyhow::Result<()> {
  let generated = GeneratedFile::types(spec!("postprocess/response_dispatch.json"), GenerationTarget::Client)?;

  assert_eq!(
    generated.dispatch("GetPetRequest")?,
    [
      "status==http::StatusCode::NOT_FOUND",
      "status==http::StatusCode::OK",
      "status.is_client_error()",
      "content_type.ends_with(\"/json\")||content_type.ends_with(\"+json\")",
      "content_type.ends_with(\"/xml\")||content_type.ends_with(\"+xml\")",
    ]
  );
  Ok(())
}

#[rstest]
fn headers_merge_across_a_status_class() -> anyhow::Result<()> {
  let generated = GeneratedFile::types(spec!("postprocess/response_dispatch.json"), GenerationTarget::Client)?;

  let response_types = generated.response_types();
  assert!(
    response_types
      .contains(&"ListPetsRequest -> ApiResponse<WithHeaders<LinkAndXTotalHeaders,Option<Pets>>,Error>".to_string()),
    "success statuses share one headers struct and the bodiless 201 makes the body optional: {response_types:?}"
  );
  assert_eq!(
    generated.fields("LinkAndXTotalHeaders")?,
    ["link: String", "x_total: Option<i64>"],
    "a header is required only when every status in the class requires it"
  );
  let variants = generated.variants("ApiResponse")?;
  assert!(
    variants.contains(&"Created(Value)".to_string()),
    "a bodiless status whose class declares headers carries the value: {variants:?}"
  );
  Ok(())
}

#[rstest]
#[case::client_keeps_unread_responses(
  GenerationTarget::Client,
  &["Ok(Value)", "Unknown(http::StatusCode,Failure)", "Other(Box<reqwest::Response>)"]
)]
#[case::server_drops_it_when_every_operation_declares_default(
  GenerationTarget::Server,
  &["Ok(Value)", "Unknown(http::StatusCode,Failure)"]
)]
fn other_variant_depends_on_the_target(
  #[case] target: GenerationTarget,
  #[case] expected: &[&str],
) -> anyhow::Result<()> {
  let generated = GeneratedFile::types(spec!("postprocess/default_responses.json"), target)?;

  assert_eq!(generated.variants("ApiResponse")?, expected);
  Ok(())
}

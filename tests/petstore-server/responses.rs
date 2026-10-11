use petstore_server as server;
use reqwest::{Method, RequestBuilder, StatusCode};
use rstest::rstest;
use uuid::Uuid;

use crate::{
  Keyed as _, PET_ID, PNG_SIGNATURE, Raw as _, seed_pet, send,
  support::{API_VERSION, Petstore, petstore},
};

const UNKNOWN_PET: Uuid = Uuid::from_u128(0xDEAD_BEEF);

#[rstest]
#[case::binary_download(
  |petstore: &Petstore| petstore.raw(Method::GET, &format!("/pets/{PET_ID}/photo")).api_key(),
  StatusCode::OK,
  "image/png",
  Some(PNG_SIGNATURE.as_slice()),
)]
#[case::text_download(
  |petstore: &Petstore| petstore.raw(Method::GET, &format!("/pets/{PET_ID}/card")).api_key(),
  StatusCode::OK,
  "text/plain",
  Some(b"Biscuit (dog)\nStatus: available\nNotes: Loves naps".as_slice()),
)]
#[case::problem_branch_of_an_error_union(
  |petstore: &Petstore| petstore
    .raw(Method::GET, &format!("/pets/{UNKNOWN_PET}"))
    .api_key()
    .header(server::X_API_VERSION, API_VERSION),
  StatusCode::NOT_FOUND,
  "application/problem+json",
  None,
)]
#[case::error_branch_of_an_error_union(
  |petstore: &Petstore| petstore
    .raw(Method::GET, &format!("/pets/{PET_ID}"))
    .api_key()
    .header(server::X_API_VERSION, "1999-01"),
  StatusCode::BAD_REQUEST,
  "application/json",
  None,
)]
#[tokio::test]
async fn a_response_is_sent_under_the_media_type_its_status_declares(
  #[future(awt)] petstore: Petstore,
  #[case] request: fn(&Petstore) -> RequestBuilder,
  #[case] status: StatusCode,
  #[case] media_type: &str,
  #[case] body: Option<&[u8]>,
) -> anyhow::Result<()> {
  seed_pet(&petstore);
  petstore.service.with_state(|state| {
    state.photos.insert(PET_ID, PNG_SIGNATURE.to_vec());
    state.notes.insert(PET_ID, "Loves naps".to_string());
  });

  let reply = send(request(&petstore)).await?;

  assert_eq!(reply.status, status, "{}", reply.text());
  assert_eq!(reply.media_type.as_deref(), Some(media_type));
  if let Some(body) = body {
    assert_eq!(reply.body, body);
  }
  Ok(())
}

mod credentials;
mod requests;
mod responses;
mod streams;
mod support;

use petstore_server as server;
use proptest::prelude::ProptestConfig;
use reqwest::{Method, RequestBuilder, StatusCode, header::CONTENT_TYPE};
use support::{API_KEY, Petstore};
use uuid::Uuid;

const PET_ID: Uuid = Uuid::from_u128(0xB15C_0175);
const PNG_SIGNATURE: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];

trait Raw {
  fn raw(&self, method: Method, path: &str) -> RequestBuilder;
}

impl Raw for Petstore {
  fn raw(&self, method: Method, path: &str) -> RequestBuilder {
    reqwest::Client::new().request(method, self.url(path))
  }
}

trait Keyed {
  fn api_key(self) -> Self;
}

impl Keyed for RequestBuilder {
  fn api_key(self) -> Self {
    self.header(server::X_API_KEY, API_KEY)
  }
}

struct Reply {
  status: StatusCode,
  media_type: Option<String>,
  body: Vec<u8>,
}

impl Reply {
  fn text(&self) -> String {
    String::from_utf8_lossy(&self.body).into_owned()
  }
}

async fn send(request: RequestBuilder) -> anyhow::Result<Reply> {
  let response = request.send().await?;
  let status = response.status();
  let media_type = response
    .headers()
    .get(CONTENT_TYPE)
    .map(|value| value.to_str().map(essence))
    .transpose()?;
  let body = response.bytes().await?.to_vec();
  Ok(Reply {
    status,
    media_type,
    body,
  })
}

fn essence(content_type: &str) -> String {
  mediatype::MediaType::parse(content_type).map_or_else(
    |_| content_type.to_string(),
    |media_type| media_type.essence().to_string(),
  )
}

fn seed_pet(petstore: &Petstore) {
  petstore.service.insert_pet(server::Pet {
    id: PET_ID,
    name: "Biscuit".to_string(),
    species: server::Species::Dog,
    status: Some(server::PetStatus::Available),
    ..server::Pet::default()
  });
}

fn property_config(cases: u32) -> ProptestConfig {
  ProptestConfig {
    failure_persistence: None,
    ..ProptestConfig::with_cases(cases)
  }
}

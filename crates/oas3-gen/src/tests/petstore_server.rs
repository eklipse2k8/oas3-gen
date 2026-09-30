use std::future::{Future, ready};

use axum::{
  extract::{Path, Query, State},
  response::IntoResponse,
};
use http::HeaderMap;
use validator::Validate;

use crate::fixtures::petstore_server::*;

#[test]
fn test_list_pets_request_compiles() {
  let request = ListPetsRequest::builder()
    .api_version("v1".to_string())
    .x_sort_order(ListPetsRequestHeaderXSortOrder::Asc)
    .x_only(vec![ListPetsRequestHeaderXonly::Bird, ListPetsRequestHeaderXonly::Fish])
    .x_compatibility_date(chrono::NaiveDate::from_ymd_opt(2026, 6, 9).unwrap())
    .limit(50)
    .build();
  assert!(request.is_ok(), "request should be valid");
  let request = request.unwrap();
  assert_eq!(request.query.limit, Some(50), "limit should be 50");

  let headers: HeaderMap = request.header.try_into().unwrap();
  assert_eq!(
    headers.get("x-sort-order").unwrap().to_str().unwrap(),
    "asc",
    "header map should contain x-sort-order"
  );
  assert_eq!(
    headers.get("x-only").unwrap().to_str().unwrap(),
    "bird,fish",
    "header map should contain x-only values"
  );
  assert_eq!(
    headers.get("x-compatibility-date").unwrap().to_str().unwrap(),
    "2026-06-09",
    "header map should contain required x-compatibility-date"
  );
}

#[test]
fn test_list_pets_request_query_validation() {
  let valid_query = ListPetsRequestQuery { limit: Some(50) };
  assert!(valid_query.validate().is_ok(), "limit=50 should be valid");

  let min_query = ListPetsRequestQuery { limit: Some(1) };
  assert!(min_query.validate().is_ok(), "limit=1 should be valid");

  let max_query = ListPetsRequestQuery { limit: Some(100) };
  assert!(max_query.validate().is_ok(), "limit=100 should be valid");

  let none_query = ListPetsRequestQuery { limit: None };
  assert!(none_query.validate().is_ok(), "limit=None should be valid");

  let below_min_query = ListPetsRequestQuery { limit: Some(0) };
  assert!(
    below_min_query.validate().is_err(),
    "limit=0 should fail validation (below min=1)"
  );

  let above_max_query = ListPetsRequestQuery { limit: Some(101) };
  assert!(
    above_max_query.validate().is_err(),
    "limit=101 should fail validation (above max=100)"
  );
}

#[test]
fn test_show_pet_by_id_request_compiles() {
  let request = ShowPetByIdRequest::builder()
    .pet_id("pet-123".to_string())
    .x_api_version("v1".to_string())
    .build();
  assert!(request.is_ok(), "request should be valid");
  let request = request.unwrap();
  assert_eq!(request.path.pet_id, "pet-123", "pet_id should match");
  assert_eq!(request.header.x_api_version, "v1", "x_api_version should match");
}

#[test]
fn test_show_pet_by_id_path_validation() {
  let valid_path = ShowPetByIdRequestPath {
    pet_id: "1".to_string(),
  };
  assert!(valid_path.validate().is_ok(), "non-empty pet_id should be valid");

  let empty_path = ShowPetByIdRequestPath { pet_id: String::new() };
  assert!(
    empty_path.validate().is_err(),
    "empty pet_id should fail validation (min length=1)"
  );
}

#[test]
fn test_show_pet_by_id_header_validation() {
  let valid_header = ShowPetByIdRequestHeader {
    x_api_version: "v1".to_string(),
  };
  assert!(
    valid_header.validate().is_ok(),
    "non-empty x_api_version should be valid"
  );

  let empty_header = ShowPetByIdRequestHeader {
    x_api_version: String::new(),
  };
  assert!(
    empty_header.validate().is_err(),
    "empty x_api_version should fail validation (min length=1)"
  );
}

#[test]
fn test_show_pet_by_id_header_to_header_map() {
  let header = ShowPetByIdRequestHeader {
    x_api_version: "v2".to_string(),
  };
  let header_map: http::HeaderMap = header.try_into().expect("valid header");
  assert_eq!(
    header_map.get("x-api-version").map(|v| v.to_str().unwrap()),
    Some("v2"),
    "header map should contain x-api-version"
  );
}

#[test]
fn test_create_pets_request_compiles() {
  let request = CreatePetsRequest::builder()
    .api_version("v1".to_string())
    .build()
    .unwrap();
  assert!(request.validate().is_ok(), "request with api_version should be valid");
  assert_eq!(request.path.api_version, "v1", "api_version should match");
}

#[test]
fn test_pet_struct_compiles() {
  let pet = Pet {
    id: 1,
    name: "Fluffy".to_string(),
    tag: Some("cat".to_string()),
    ..Default::default()
  };
  assert_eq!(pet.id, 1, "id should match");
  assert_eq!(pet.name, "Fluffy", "name should match");
  assert_eq!(pet.tag, Some("cat".to_string()), "tag should match");
}

#[test]
fn test_error_struct_compiles() {
  let error = Error {
    code: 404,
    message: "Not found".to_string(),
  };
  assert_eq!(error.code, 404, "code should match");
  assert_eq!(error.message, "Not found", "message should match");
}

#[test]
fn test_pets_type_alias() {
  let pets: Pets = vec![
    Pet {
      id: 1,
      name: "Fluffy".to_string(),
      tag: None,
      ..Default::default()
    },
    Pet {
      id: 2,
      name: "Rex".to_string(),
      tag: Some("dog".to_string()),
      ..Default::default()
    },
  ];
  assert_eq!(pets.len(), 2, "should have 2 pets");
}

#[test]
fn test_query_deserialization() {
  let query: ListPetsRequestQuery = serde_json::from_str(r#"{"limit": 10}"#).expect("deserialization should succeed");
  assert_eq!(query.limit, Some(10), "limit should be deserialized");

  let query_none: ListPetsRequestQuery = serde_json::from_str(r"{}").expect("deserialization should succeed");
  assert_eq!(query_none.limit, None, "missing fields should be None");
}

#[derive(Debug, Clone, Copy)]
enum Outcome {
  Declared,
  Default,
  Failure,
}

#[derive(Clone)]
struct StubService {
  outcome: Outcome,
}

impl StubService {
  fn upstream_error() -> Error {
    Error {
      code: 502,
      message: "upstream unavailable".to_string(),
    }
  }
}

impl ApiServer for StubService {
  fn list_pets(
    &self,
    _request: ListPetsRequest,
  ) -> impl Future<Output = anyhow::Result<ApiResponse<WithHeaders<XNextHeaders, Pets>, Error>>> + Send {
    ready(match self.outcome {
      Outcome::Declared => Ok(ApiResponse::Ok(WithHeaders {
        headers: XNextHeaders {
          x_next: Some("/v1/pets?page=2".to_string()),
        },
        body: vec![Pet {
          id: 1,
          name: "Fluffy".to_string(),
          tag: None,
          ..Default::default()
        }],
      })),
      Outcome::Default => Ok(ApiResponse::Unknown(
        http::StatusCode::BAD_GATEWAY,
        Self::upstream_error(),
      )),
      Outcome::Failure => Err(anyhow::anyhow!("service failure")),
    })
  }

  fn create_pets(
    &self,
    _request: CreatePetsRequest,
  ) -> impl Future<Output = anyhow::Result<ApiResponse<WithHeaders<LocationHeaders, ()>, Error>>> + Send {
    ready(Ok(ApiResponse::Created(WithHeaders {
      headers: LocationHeaders {
        location: "/v1/pets/7".to_string(),
      },
      body: (),
    })))
  }

  fn list_cats(
    &self,
    _request: ListCatsRequest,
  ) -> impl Future<Output = anyhow::Result<ApiResponse<WithHeaders<XNextHeaders, Cats>, Error>>> + Send {
    ready(Ok(ApiResponse::Ok(WithHeaders {
      headers: XNextHeaders::default(),
      body: vec![],
    })))
  }

  fn show_pet_by_id(
    &self,
    _request: ShowPetByIdRequest,
  ) -> impl Future<Output = anyhow::Result<ApiResponse<WithHeaders<XCacheHeaders, Pet>, Error>>> + Send {
    ready(Ok(ApiResponse::Ok(WithHeaders {
      headers: XCacheHeaders {
        x_cache: Some(ShowPetByIdResponseHeaderXCache::Hit),
      },
      body: Pet {
        id: 42,
        name: "Rex".to_string(),
        tag: Some("dog".to_string()),
        ..Default::default()
      },
    })))
  }

  fn upload_pet_image(
    &self,
    _request: UploadPetImageRequest,
  ) -> impl Future<Output = anyhow::Result<ApiResponse<Pet, Error>>> + Send {
    ready(Ok(ApiResponse::Unknown(
      http::StatusCode::SERVICE_UNAVAILABLE,
      Self::upstream_error(),
    )))
  }
}

#[tokio::test]
async fn test_list_pets_handler_maps_response_enum_to_status() {
  let cases = [
    (Outcome::Declared, http::StatusCode::OK),
    (Outcome::Default, http::StatusCode::BAD_GATEWAY),
    (Outcome::Failure, http::StatusCode::INTERNAL_SERVER_ERROR),
  ];

  for (outcome, expected) in cases {
    let response = list_pets(
      State(StubService { outcome }),
      Path(ListPetsRequestPath {
        api_version: "v1".to_string(),
      }),
      Query(ListPetsRequestQuery { limit: None }),
      HeaderMap::new(),
    )
    .await
    .into_response();
    assert_eq!(response.status(), expected, "status mismatch for {outcome:?}");
  }
}

#[tokio::test]
async fn test_list_pets_handler_writes_response_headers() {
  let cases = [(Outcome::Declared, Some("/v1/pets?page=2")), (Outcome::Default, None)];

  for (outcome, expected) in cases {
    let response = list_pets(
      State(StubService { outcome }),
      Path(ListPetsRequestPath {
        api_version: "v1".to_string(),
      }),
      Query(ListPetsRequestQuery { limit: None }),
      HeaderMap::new(),
    )
    .await
    .into_response();
    assert_eq!(
      response.headers().get("x-next").map(|v| v.to_str().unwrap()),
      expected,
      "x-next header mismatch for {outcome:?}"
    );
  }
}

#[tokio::test]
async fn test_handlers_map_unit_and_body_variants_to_status() {
  let service = StubService {
    outcome: Outcome::Declared,
  };

  let created = create_pets(
    State(service.clone()),
    Path(CreatePetsRequestPath {
      api_version: "v1".to_string(),
    }),
  )
  .await
  .into_response();
  assert_eq!(
    created.status(),
    http::StatusCode::CREATED,
    "bodiless variant should answer with its status"
  );
  assert_eq!(
    created.headers().get("location").map(|v| v.to_str().unwrap()),
    Some("/v1/pets/7"),
    "bodiless variant should carry its declared headers"
  );

  let shown = show_pet_by_id(
    State(service),
    Path(ShowPetByIdRequestPath {
      pet_id: "42".to_string(),
    }),
    HeaderMap::new(),
  )
  .await
  .into_response();
  assert_eq!(
    shown.status(),
    http::StatusCode::OK,
    "body variant should answer with its status and a JSON body"
  );
  assert_eq!(
    shown.headers().get("x-cache").map(|v| v.to_str().unwrap()),
    Some("hit"),
    "enum header should be written with its wire value"
  );
}

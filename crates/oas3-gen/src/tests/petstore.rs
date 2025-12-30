use validator::Validate;

use crate::fixtures::petstore::*;

#[test]
fn test_list_pets_request_compiles() {
  let request = ListPetsRequest::builder().limit(50).build();
  assert!(request.is_ok(), "request should be valid");
  let request = request.unwrap();
  assert_eq!(request.query.limit, Some(50), "limit should be 50");
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
  let request = CreatePetsRequest {};
  assert!(request.validate().is_ok(), "empty request should be valid");
}

#[test]
fn test_pet_struct_compiles() {
  let pet = Pet {
    id: 1,
    name: "Fluffy".to_string(),
    tag: Some("cat".to_string()),
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
    },
    Pet {
      id: 2,
      name: "Rex".to_string(),
      tag: Some("dog".to_string()),
    },
  ];
  assert_eq!(pets.len(), 2, "should have 2 pets");
}

#[test]
fn test_query_serialization() {
  let query = ListPetsRequestQuery { limit: Some(10) };
  let serialized = serde_json::to_string(&query).expect("serialization should succeed");
  assert!(
    serialized.contains("10"),
    "serialized output should contain limit value"
  );

  let query_none = ListPetsRequestQuery { limit: None };
  let serialized_none = serde_json::to_string(&query_none).expect("serialization should succeed");
  assert_eq!(serialized_none, "{}", "None fields should be skipped");
}

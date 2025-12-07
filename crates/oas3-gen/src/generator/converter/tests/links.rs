use oas3::{Spec, spec::Response};
use serde_json::json;

use crate::generator::{ast::RuntimeExpression, converter::links::LinkConverter};

fn create_spec_from_json(json_value: serde_json::Value) -> Spec {
  serde_json::from_value(json_value).expect("valid spec JSON")
}

fn create_empty_spec() -> Spec {
  create_spec_from_json(json!({
    "openapi": "3.0.0",
    "info": { "title": "Test API", "version": "1.0.0" },
    "paths": {}
  }))
}

fn create_spec_with_component_links(links_json: &serde_json::Value) -> Spec {
  create_spec_from_json(json!({
    "openapi": "3.0.0",
    "info": { "title": "Test API", "version": "1.0.0" },
    "paths": {},
    "components": {
      "links": links_json.clone()
    }
  }))
}

fn create_response_with_links(links_json: &serde_json::Value) -> Response {
  serde_json::from_value(json!({
    "description": "Test response",
    "links": links_json.clone()
  }))
  .expect("valid response JSON")
}

#[test]
fn test_extract_inline_link() {
  let spec = create_empty_spec();
  let converter = LinkConverter::new(&spec);

  let response = create_response_with_links(&json!({
    "GetBurger": {
      "operationId": "getBurger",
      "parameters": {
        "burgerId": "$response.body#/id"
      },
      "description": "Get the created burger"
    }
  }));

  let result = converter.extract_links_from_response(&response);

  assert_eq!(result.len(), 1, "should extract one link");
  let link = &result[0];
  assert_eq!(link.name, "GetBurger", "link name mismatch");
  assert_eq!(link.target_operation_id, "getBurger", "operation ID mismatch");
  assert_eq!(
    link.description,
    Some("Get the created burger".to_string()),
    "description mismatch"
  );
  assert_eq!(link.parameters.len(), 1, "should have one parameter");

  let param = link.parameters.get("burgerId").expect("burgerId param should exist");
  assert!(
    matches!(param, RuntimeExpression::ResponseBodyPath { json_pointer } if json_pointer == "/id"),
    "parameter should be ResponseBodyPath with /id pointer"
  );
}

#[test]
fn test_extract_ref_link() {
  let spec = create_spec_with_component_links(&json!({
    "LocateBurger": {
      "operationId": "locateBurger",
      "parameters": {
        "burgerId": "$response.body#/id"
      }
    }
  }));

  let converter = LinkConverter::new(&spec);

  let response = create_response_with_links(&json!({
    "LocateBurger": {
      "$ref": "#/components/links/LocateBurger"
    }
  }));

  let result = converter.extract_links_from_response(&response);

  assert_eq!(result.len(), 1, "should extract one link from ref");
  let link = &result[0];
  assert_eq!(link.target_operation_id, "locateBurger", "operation ID mismatch");
}

#[test]
fn test_extract_multiple_links() {
  let spec = create_empty_spec();
  let converter = LinkConverter::new(&spec);

  let response = create_response_with_links(&json!({
    "GetBurger": {
      "operationId": "getBurger",
      "parameters": {
        "burgerId": "$response.body#/id"
      }
    },
    "DeleteBurger": {
      "operationId": "deleteBurger",
      "parameters": {
        "burgerId": "$response.body#/id"
      }
    }
  }));

  let result = converter.extract_links_from_response(&response);

  assert_eq!(result.len(), 2, "should extract two links");
  let names: Vec<_> = result.iter().map(|l| &l.name).collect();
  assert!(names.contains(&&"GetBurger".to_string()), "should contain GetBurger");
  assert!(
    names.contains(&&"DeleteBurger".to_string()),
    "should contain DeleteBurger"
  );
}

#[test]
fn test_extract_empty_links() {
  let spec = create_empty_spec();
  let converter = LinkConverter::new(&spec);

  let response: Response = serde_json::from_value(json!({
    "description": "Test response"
  }))
  .expect("valid response JSON");

  let result = converter.extract_links_from_response(&response);

  assert!(result.is_empty(), "should return empty vec for no links");
}

#[test]
fn test_skip_unresolved_ref() {
  let spec = create_empty_spec();
  let converter = LinkConverter::new(&spec);

  let response = create_response_with_links(&json!({
    "MissingLink": {
      "$ref": "#/components/links/NonExistent"
    }
  }));

  let result = converter.extract_links_from_response(&response);

  assert!(result.is_empty(), "should skip unresolved link refs");
}

#[test]
fn test_skip_operation_ref_link() {
  let spec = create_empty_spec();
  let converter = LinkConverter::new(&spec);

  let response = create_response_with_links(&json!({
    "ExternalLink": {
      "operationRef": "https://example.com/spec.json#/paths/~1burgers/get",
      "parameters": {}
    }
  }));

  let result = converter.extract_links_from_response(&response);

  assert!(result.is_empty(), "should skip operationRef links");
}

#[test]
fn test_link_with_multiple_parameters() {
  let spec = create_empty_spec();
  let converter = LinkConverter::new(&spec);

  let response = create_response_with_links(&json!({
    "ComplexLink": {
      "operationId": "complexOperation",
      "parameters": {
        "pathId": "$response.body#/id",
        "queryFilter": "$request.query.filter",
        "literalValue": "static-value"
      }
    }
  }));

  let result = converter.extract_links_from_response(&response);

  assert_eq!(result.len(), 1, "should extract one link");
  let link = &result[0];
  assert_eq!(link.parameters.len(), 3, "should have three parameters");

  let path_param = link.parameters.get("pathId").unwrap();
  assert!(matches!(path_param, RuntimeExpression::ResponseBodyPath { json_pointer } if json_pointer == "/id"));

  let query_param = link.parameters.get("queryFilter").unwrap();
  assert!(matches!(query_param, RuntimeExpression::RequestQueryParam { name } if name == "filter"));

  let literal_param = link.parameters.get("literalValue").unwrap();
  assert!(matches!(literal_param, RuntimeExpression::Literal { value } if value == "static-value"));
}

#[test]
fn test_link_with_request_path_param() {
  let spec = create_empty_spec();
  let converter = LinkConverter::new(&spec);

  let response = create_response_with_links(&json!({
    "PathLink": {
      "operationId": "pathOperation",
      "parameters": {
        "id": "$request.path.resourceId"
      }
    }
  }));

  let result = converter.extract_links_from_response(&response);

  let link = &result[0];
  let param = link.parameters.get("id").unwrap();
  assert!(
    matches!(param, RuntimeExpression::RequestPathParam { name } if name == "resourceId"),
    "should parse request path param"
  );
}

#[test]
fn test_link_with_request_body_expression() {
  let spec = create_empty_spec();
  let converter = LinkConverter::new(&spec);

  let response = create_response_with_links(&json!({
    "BodyLink": {
      "operationId": "bodyOperation",
      "parameters": {
        "data": "$request.body#/nested/field"
      }
    }
  }));

  let result = converter.extract_links_from_response(&response);

  let link = &result[0];
  let param = link.parameters.get("data").unwrap();
  assert!(
    matches!(param, RuntimeExpression::RequestBody { json_pointer: Some(p) } if p == "/nested/field"),
    "should parse request body with JSON pointer"
  );
}

#[test]
fn test_link_without_parameters() {
  let spec = create_empty_spec();
  let converter = LinkConverter::new(&spec);

  let response = create_response_with_links(&json!({
    "SimpleLink": {
      "operationId": "simpleOperation"
    }
  }));

  let result = converter.extract_links_from_response(&response);

  assert_eq!(result.len(), 1, "should extract link without parameters");
  let link = &result[0];
  assert_eq!(link.target_operation_id, "simpleOperation");
  assert!(link.parameters.is_empty(), "should have no parameters");
}

#[test]
fn test_link_with_server_url() {
  let spec = create_empty_spec();
  let converter = LinkConverter::new(&spec);

  let response = create_response_with_links(&json!({
    "ExternalLink": {
      "operationId": "externalOperation",
      "server": {
        "url": "https://external-api.example.com/v2"
      }
    }
  }));

  let result = converter.extract_links_from_response(&response);

  assert_eq!(result.len(), 1, "should extract one link");
  let link = &result[0];
  assert_eq!(link.target_operation_id, "externalOperation");
  assert_eq!(
    link.server_url,
    Some("https://external-api.example.com/v2".to_string()),
    "should extract server URL"
  );
}

#[test]
fn test_link_without_server_url() {
  let spec = create_empty_spec();
  let converter = LinkConverter::new(&spec);

  let response = create_response_with_links(&json!({
    "LocalLink": {
      "operationId": "localOperation"
    }
  }));

  let result = converter.extract_links_from_response(&response);

  assert_eq!(result.len(), 1, "should extract one link");
  let link = &result[0];
  assert!(link.server_url.is_none(), "should have no server URL");
}

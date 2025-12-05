use crate::generator::ast::RuntimeExpression;

pub fn parse(input: &str) -> RuntimeExpression {
  if !input.starts_with('$') {
    return RuntimeExpression::Literal {
      value: input.to_string(),
    };
  }

  if let Some(rest) = input.strip_prefix("$response.body") {
    return if let Some(pointer) = rest.strip_prefix('#') {
      RuntimeExpression::ResponseBodyPath {
        json_pointer: pointer.to_string(),
      }
    } else {
      RuntimeExpression::ResponseBodyPath {
        json_pointer: String::new(),
      }
    };
  }

  if let Some(rest) = input.strip_prefix("$request.query.") {
    return RuntimeExpression::RequestQueryParam { name: rest.to_string() };
  }

  if let Some(rest) = input.strip_prefix("$request.path.") {
    return RuntimeExpression::RequestPathParam { name: rest.to_string() };
  }

  if let Some(rest) = input.strip_prefix("$request.header.") {
    return RuntimeExpression::RequestHeader { name: rest.to_string() };
  }

  if let Some(rest) = input.strip_prefix("$request.body") {
    return if let Some(pointer) = rest.strip_prefix('#') {
      RuntimeExpression::RequestBody {
        json_pointer: Some(pointer.to_string()),
      }
    } else {
      RuntimeExpression::RequestBody { json_pointer: None }
    };
  }

  RuntimeExpression::Unsupported
}

pub fn json_pointer_to_field_path(pointer: &str) -> Vec<String> {
  if pointer.is_empty() {
    return vec![];
  }

  pointer
    .trim_start_matches('/')
    .split('/')
    .map(|segment| segment.replace("~1", "/").replace("~0", "~"))
    .collect()
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn test_response_body_path() {
    let expr = parse("$response.body#/id");
    assert!(matches!(
        expr,
        RuntimeExpression::ResponseBodyPath { json_pointer } if json_pointer == "/id"
    ));
  }

  #[test]
  fn test_response_body_nested_path() {
    let expr = parse("$response.body#/data/items/0/id");
    assert!(matches!(
        expr,
        RuntimeExpression::ResponseBodyPath { json_pointer } if json_pointer == "/data/items/0/id"
    ));
  }

  #[test]
  fn test_response_body_no_path() {
    let expr = parse("$response.body");
    assert!(matches!(
        expr,
        RuntimeExpression::ResponseBodyPath { json_pointer } if json_pointer.is_empty()
    ));
  }

  #[test]
  fn test_request_query_param() {
    let expr = parse("$request.query.filter");
    assert!(matches!(
        expr,
        RuntimeExpression::RequestQueryParam { name } if name == "filter"
    ));
  }

  #[test]
  fn test_request_path_param() {
    let expr = parse("$request.path.id");
    assert!(matches!(
        expr,
        RuntimeExpression::RequestPathParam { name } if name == "id"
    ));
  }

  #[test]
  fn test_request_header() {
    let expr = parse("$request.header.Authorization");
    assert!(matches!(
        expr,
        RuntimeExpression::RequestHeader { name } if name == "Authorization"
    ));
  }

  #[test]
  fn test_request_body() {
    let expr = parse("$request.body");
    assert!(matches!(expr, RuntimeExpression::RequestBody { json_pointer: None }));
  }

  #[test]
  fn test_request_body_path() {
    let expr = parse("$request.body#/nested/field");
    assert!(matches!(
        expr,
        RuntimeExpression::RequestBody { json_pointer: Some(ref p) } if p == "/nested/field"
    ));
  }

  #[test]
  fn test_literal_value() {
    let expr = parse("some literal value");
    assert!(matches!(
        expr,
        RuntimeExpression::Literal { value } if value == "some literal value"
    ));
  }

  #[test]
  fn test_unsupported_expressions() {
    assert!(matches!(
      parse("$response.header.X-Request-Id"),
      RuntimeExpression::Unsupported
    ));
    assert!(matches!(parse("$unknown.something"), RuntimeExpression::Unsupported));
    assert!(matches!(parse("$url"), RuntimeExpression::Unsupported));
    assert!(matches!(parse("$method"), RuntimeExpression::Unsupported));
    assert!(matches!(parse("$statusCode"), RuntimeExpression::Unsupported));
  }

  #[test]
  fn test_json_pointer_to_field_path() {
    assert_eq!(
      json_pointer_to_field_path("/foo/bar/0/baz"),
      vec!["foo", "bar", "0", "baz"]
    );
  }

  #[test]
  fn test_json_pointer_empty() {
    assert!(json_pointer_to_field_path("").is_empty());
  }

  #[test]
  fn test_json_pointer_escape_sequences() {
    assert_eq!(json_pointer_to_field_path("/a~1b/c~0d"), vec!["a/b", "c~d"]);
  }
}

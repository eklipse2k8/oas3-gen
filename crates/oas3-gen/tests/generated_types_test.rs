//! Integration tests for generated types

// Include the generated types for testing
#[path = "../examples/generated_types.rs"]
mod generated_types;

use generated_types::*;

#[test]
fn test_struct_with_defaults() {
  // Test that structs with default values can be created using Default::default()
  let error: Apierror = Default::default();
  assert_eq!(error.message, "Internal server error");
  assert_eq!(error.r#type, "api_error");

  let auth_error: AuthenticationError = Default::default();
  assert_eq!(auth_error.message, "Authentication error");
  assert_eq!(auth_error.r#type, "authentication_error");
}

#[test]
fn test_struct_deserialization() {
  // Test that structs can be deserialized from JSON
  let json = r#"{"message":"Test error","type":"test_error"}"#;
  let error: Result<Apierror, _> = serde_json::from_str(json);
  assert!(error.is_ok());
  let error = error.unwrap();
  assert_eq!(error.message, "Test error");
  assert_eq!(error.r#type, "test_error");
}

#[test]
fn test_struct_serialization() {
  // Test that structs can be serialized to JSON
  let error = Apierror {
    message: "Custom error".to_string(),
    r#type: "custom_type".to_string(),
  };
  let json = serde_json::to_string(&error);
  assert!(json.is_ok());
  assert!(json.unwrap().contains("Custom error"));
}

#[test]
fn test_enum_variants() {
  // Test that enums work correctly
  let beta1 = AnthropicBeta::String("test".to_string());
  let json1 = serde_json::to_string(&beta1);
  assert!(json1.is_ok());

  let beta2 = AnthropicBeta::Enum("value".to_string());
  let json2 = serde_json::to_string(&beta2);
  assert!(json2.is_ok());
}

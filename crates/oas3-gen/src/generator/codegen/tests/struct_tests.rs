use std::collections::BTreeMap;

use proc_macro2::TokenStream;

use crate::generator::{
  ast::{FieldDef, StructDef, StructKind, TypeRef},
  codegen::{TypeUsage, Visibility, structs},
};

fn create_test_struct(name: &str, kind: StructKind) -> StructDef {
  StructDef {
    name: name.to_string(),
    docs: vec![],
    fields: vec![FieldDef {
      name: "field1".to_string(),
      docs: vec![],
      rust_type: TypeRef::new("String"),
      serde_attrs: vec![],
      extra_attrs: vec![],
      validation_attrs: vec!["length(min = 1)".to_string()],
      regex_validation: None,
      default_value: None,
      read_only: false,
      write_only: false,
      deprecated: false,
      multiple_of: None,
    }],
    derives: vec![
      "Debug".to_string(),
      "Clone".to_string(),
      "Serialize".to_string(),
      "Deserialize".to_string(),
    ],
    serde_attrs: vec![],
    outer_attrs: vec![],
    methods: vec![],
    kind,
  }
}

fn contains_derive(tokens: &TokenStream, derive_name: &str) -> bool {
  let code = tokens.to_string();
  code.contains("derive (") && code.contains(derive_name)
}

fn contains_validation(tokens: &TokenStream) -> bool {
  let code = tokens.to_string();
  code.contains("validate") && (code.contains("# [validate") || code.contains("#[validate"))
}

#[test]
fn test_schema_struct_no_type_usage() {
  let def = create_test_struct("TestSchema", StructKind::Schema);
  let type_usage = BTreeMap::new();
  let regex_lookup = BTreeMap::new();

  let result = structs::generate_struct(&def, &regex_lookup, &type_usage, Visibility::Public);
  let code = result.to_string();

  assert!(contains_derive(&result, "Debug"), "Should contain Debug derive");
  assert!(contains_derive(&result, "Clone"), "Should contain Clone derive");
  assert!(contains_derive(&result, "Serialize"), "Should contain Serialize derive");
  assert!(
    contains_derive(&result, "Deserialize"),
    "Should contain Deserialize derive"
  );
  assert!(contains_validation(&result), "Should include validation attributes");
  assert!(code.contains("pub struct TestSchema"), "Should be public struct");
}

#[test]
fn test_schema_struct_with_request_only_usage() {
  let def = create_test_struct("RequestSchema", StructKind::Schema);
  let mut type_usage = BTreeMap::new();
  type_usage.insert("RequestSchema".to_string(), TypeUsage::RequestOnly);
  let regex_lookup = BTreeMap::new();

  let result = structs::generate_struct(&def, &regex_lookup, &type_usage, Visibility::Public);
  assert!(contains_derive(&result, "Serialize"), "Should contain Serialize derive");
  assert!(
    contains_derive(&result, "validator :: Validate"),
    "Should contain Validate derive"
  );
  assert!(contains_validation(&result), "Should include validation attributes");
}

#[test]
fn test_schema_struct_with_response_only_usage() {
  let def = create_test_struct("ResponseSchema", StructKind::Schema);
  let mut type_usage = BTreeMap::new();
  type_usage.insert("ResponseSchema".to_string(), TypeUsage::ResponseOnly);
  let regex_lookup = BTreeMap::new();

  let result = structs::generate_struct(&def, &regex_lookup, &type_usage, Visibility::Public);
  assert!(
    contains_derive(&result, "Deserialize"),
    "Should contain Deserialize derive"
  );
  assert!(contains_validation(&result), "Should include validation attributes");
}

#[test]
fn test_schema_struct_with_bidirectional_usage() {
  let def = create_test_struct("BidirectionalSchema", StructKind::Schema);
  let mut type_usage = BTreeMap::new();
  type_usage.insert("BidirectionalSchema".to_string(), TypeUsage::Bidirectional);
  let regex_lookup = BTreeMap::new();

  let result = structs::generate_struct(&def, &regex_lookup, &type_usage, Visibility::Public);
  assert!(contains_derive(&result, "Serialize"), "Should contain Serialize derive");
  assert!(
    contains_derive(&result, "Deserialize"),
    "Should contain Deserialize derive"
  );
  assert!(
    contains_derive(&result, "validator :: Validate"),
    "Should contain Validate derive"
  );
  assert!(contains_validation(&result), "Should include validation attributes");
}

#[test]
fn test_operation_request_struct() {
  let def = create_test_struct("GetUsersRequest", StructKind::OperationRequest);
  let mut type_usage = BTreeMap::new();
  type_usage.insert("GetUsersRequest".to_string(), TypeUsage::RequestOnly);
  let regex_lookup = BTreeMap::new();

  let result = structs::generate_struct(&def, &regex_lookup, &type_usage, Visibility::Public);
  let code = result.to_string();
  assert!(contains_derive(&result, "Debug"), "Should contain Debug derive");
  assert!(contains_derive(&result, "Clone"), "Should contain Clone derive");
  assert!(
    contains_derive(&result, "validator :: Validate"),
    "Should contain Validate derive"
  );
  assert!(
    contains_derive(&result, "oas3_gen_support :: Default"),
    "Should contain Default derive"
  );
  assert!(!code.contains("Serialize"), "Should NOT contain Serialize derive");
  assert!(!code.contains("Deserialize"), "Should NOT contain Deserialize derive");
  assert!(contains_validation(&result), "Should include validation attributes");
}

#[test]
fn test_request_body_struct_request_only() {
  let def = create_test_struct("CreateUserRequestBody", StructKind::RequestBody);
  let mut type_usage = BTreeMap::new();
  type_usage.insert("CreateUserRequestBody".to_string(), TypeUsage::RequestOnly);
  let regex_lookup = BTreeMap::new();

  let result = structs::generate_struct(&def, &regex_lookup, &type_usage, Visibility::Public);
  let code = result.to_string();
  assert!(contains_derive(&result, "Debug"), "Should contain Debug derive");
  assert!(contains_derive(&result, "Clone"), "Should contain Clone derive");
  assert!(contains_derive(&result, "Serialize"), "Should contain Serialize derive");
  assert!(
    contains_derive(&result, "validator :: Validate"),
    "Should contain Validate derive"
  );
  assert!(
    contains_derive(&result, "oas3_gen_support :: Default"),
    "Should contain Default derive"
  );
  assert!(!code.contains("Deserialize"), "Should NOT contain Deserialize derive");
  assert!(contains_validation(&result), "Should include validation attributes");
}

#[test]
fn test_request_body_struct_response_only() {
  let def = create_test_struct("GetUserResponseBody", StructKind::RequestBody);
  let mut type_usage = BTreeMap::new();
  type_usage.insert("GetUserResponseBody".to_string(), TypeUsage::ResponseOnly);
  let regex_lookup = BTreeMap::new();

  let result = structs::generate_struct(&def, &regex_lookup, &type_usage, Visibility::Public);
  let code = result.to_string();
  assert!(contains_derive(&result, "Debug"), "Should contain Debug derive");
  assert!(contains_derive(&result, "Clone"), "Should contain Clone derive");
  assert!(
    contains_derive(&result, "Deserialize"),
    "Should contain Deserialize derive"
  );
  assert!(
    contains_derive(&result, "oas3_gen_support :: Default"),
    "Should contain Default derive"
  );
  assert!(!code.contains("Serialize"), "Should NOT contain Serialize derive");
  assert!(
    !code.contains("validator :: Validate"),
    "Should NOT contain Validate derive"
  );
  assert!(
    !contains_validation(&result),
    "Should NOT include validation attributes"
  );
}

#[test]
fn test_request_body_struct_bidirectional() {
  let def = create_test_struct("UpdateUserRequestBody", StructKind::RequestBody);
  let mut type_usage = BTreeMap::new();
  type_usage.insert("UpdateUserRequestBody".to_string(), TypeUsage::Bidirectional);
  let regex_lookup = BTreeMap::new();

  let result = structs::generate_struct(&def, &regex_lookup, &type_usage, Visibility::Public);
  assert!(contains_derive(&result, "Debug"), "Should contain Debug derive");
  assert!(contains_derive(&result, "Clone"), "Should contain Clone derive");
  assert!(contains_derive(&result, "Serialize"), "Should contain Serialize derive");
  assert!(
    contains_derive(&result, "Deserialize"),
    "Should contain Deserialize derive"
  );
  assert!(
    contains_derive(&result, "validator :: Validate"),
    "Should contain Validate derive"
  );
  assert!(
    contains_derive(&result, "oas3_gen_support :: Default"),
    "Should contain Default derive"
  );
  assert!(contains_validation(&result), "Should include validation attributes");
}

#[test]
fn test_request_body_struct_no_usage_defaults_to_bidirectional() {
  let def = create_test_struct("UnknownRequestBody", StructKind::RequestBody);
  let type_usage = BTreeMap::new();
  let regex_lookup = BTreeMap::new();

  let result = structs::generate_struct(&def, &regex_lookup, &type_usage, Visibility::Public);
  assert!(contains_derive(&result, "Serialize"), "Should contain Serialize derive");
  assert!(
    contains_derive(&result, "Deserialize"),
    "Should contain Deserialize derive"
  );
  assert!(
    contains_derive(&result, "validator :: Validate"),
    "Should contain Validate derive"
  );
}

#[test]
fn test_visibility_crate() {
  let def = create_test_struct("CrateStruct", StructKind::Schema);
  let type_usage = BTreeMap::new();
  let regex_lookup = BTreeMap::new();

  let result = structs::generate_struct(&def, &regex_lookup, &type_usage, Visibility::Crate);
  let code = result.to_string();
  assert!(
    code.contains("pub (crate) struct CrateStruct"),
    "Should be pub(crate) struct"
  );
}

#[test]
fn test_visibility_file() {
  let def = create_test_struct("FileStruct", StructKind::Schema);
  let type_usage = BTreeMap::new();
  let regex_lookup = BTreeMap::new();

  let result = structs::generate_struct(&def, &regex_lookup, &type_usage, Visibility::File);
  let code = result.to_string();
  assert!(code.contains("struct FileStruct"), "Should be file-private struct");
  assert!(!code.contains("pub struct"), "Should NOT have pub modifier");
}

#[test]
fn test_visibility_parsing_and_tokenization() {
  assert_eq!(Visibility::parse("public").unwrap().to_tokens().to_string(), "pub");
  assert_eq!(
    Visibility::parse("crate").unwrap().to_tokens().to_string(),
    "pub (crate)"
  );
  assert_eq!(Visibility::parse("file").unwrap().to_tokens().to_string(), "");
  assert!(Visibility::parse("invalid").is_none());
}

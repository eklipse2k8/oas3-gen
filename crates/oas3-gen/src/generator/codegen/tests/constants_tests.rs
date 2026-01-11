use quote::ToTokens as _;

use crate::generator::{
  ast::{
    FieldDef, FieldNameToken, RustType, StructDef, StructToken, TypeAliasDef, TypeAliasToken, TypeRef,
    ValidationAttribute, constants::HttpHeaderRef,
  },
  codegen::constants::{HeaderConstantsFragment, RegexConstantsResult},
};

fn make_field(name: &str, pattern: Option<&str>) -> FieldDef {
  FieldDef::builder()
    .name(FieldNameToken::from_raw(name))
    .rust_type(TypeRef::default())
    .validation_attrs(
      pattern
        .map(|p| vec![ValidationAttribute::Regex(p.to_string())])
        .unwrap_or_default(),
    )
    .build()
}

fn make_struct(name: &str, fields: Vec<FieldDef>) -> RustType {
  RustType::Struct(StructDef {
    name: StructToken::new(name),
    fields,
    ..Default::default()
  })
}

#[test]
fn test_regex_constants_result_empty() {
  let types: Vec<RustType> = vec![];
  let result = RegexConstantsResult::from_types(&types);

  assert!(result.lookup.is_empty());
  assert!(result.into_token_stream().is_empty());
}

#[test]
fn test_regex_constants_result_no_regex_fields() {
  let struct_type = make_struct("User", vec![make_field("name", None), make_field("email", None)]);
  let types: Vec<RustType> = vec![struct_type];
  let result = RegexConstantsResult::from_types(&types);

  assert!(result.lookup.is_empty());
  assert!(result.into_token_stream().is_empty());
}

#[test]
fn test_regex_constants_result_single_field() {
  let struct_type = make_struct("User", vec![make_field("email", Some(r"^[\w.-]+@[\w.-]+$"))]);
  let types: Vec<RustType> = vec![struct_type];
  let result = RegexConstantsResult::from_types(&types);

  assert_eq!(result.lookup.len(), 1);
  let code = result.into_token_stream().to_string();
  assert!(
    code.contains("REGEX_USER_EMAIL"),
    "should contain constant name: {code}"
  );
  assert!(code.contains("LazyLock"), "should use LazyLock: {code}");
  assert!(code.contains("Regex :: new"), "should create regex: {code}");
}

#[test]
fn test_regex_constants_result_multiple_fields_same_struct() {
  let struct_type = make_struct(
    "User",
    vec![
      make_field("email", Some(r"^[\w.-]+@[\w.-]+$")),
      make_field("phone", Some(r"^\+?[0-9]{10,15}$")),
    ],
  );
  let types: Vec<RustType> = vec![struct_type];
  let result = RegexConstantsResult::from_types(&types);

  assert_eq!(result.lookup.len(), 2);
  let code = result.into_token_stream().to_string();
  assert!(code.contains("REGEX_USER_EMAIL"));
  assert!(code.contains("REGEX_USER_PHONE"));
}

#[test]
fn test_regex_constants_result_deduplicates_patterns() {
  let email_pattern = r"^[\w.-]+@[\w.-]+$";
  let struct1 = make_struct("User", vec![make_field("email", Some(email_pattern))]);
  let struct2 = make_struct("Contact", vec![make_field("email", Some(email_pattern))]);
  let types: Vec<RustType> = vec![struct1, struct2];
  let result = RegexConstantsResult::from_types(&types);

  assert_eq!(result.lookup.len(), 2, "both fields should be in lookup");

  let const_names: std::collections::HashSet<_> = result.lookup.values().collect();
  assert_eq!(const_names.len(), 1, "both should reference same constant");

  let code = result.into_token_stream().to_string();
  let static_count = code.matches("static REGEX_").count();
  assert_eq!(static_count, 1, "should only generate one regex constant");
}

#[test]
fn test_regex_constants_result_skips_non_structs() {
  let struct_type = make_struct("User", vec![make_field("email", Some(r"pattern"))]);
  let alias = RustType::TypeAlias(TypeAliasDef {
    name: TypeAliasToken::new("UserId"),
    target: TypeRef::new("String"),
    ..Default::default()
  });
  let types: Vec<RustType> = vec![struct_type, alias];
  let result = RegexConstantsResult::from_types(&types);

  assert_eq!(result.lookup.len(), 1);
}

#[test]
fn test_header_constants_fragment_empty() {
  let fragment = HeaderConstantsFragment::new(vec![]);
  assert!(fragment.into_token_stream().is_empty());
}

#[test]
fn test_header_constants_fragment_single() {
  let fragment = HeaderConstantsFragment::new(vec![HttpHeaderRef::from("x-request-id")]);
  let code = fragment.into_token_stream().to_string();

  assert!(code.contains("X_REQUEST_ID"), "should contain constant name: {code}");
  assert!(
    code.contains("HeaderName :: from_static"),
    "should use from_static: {code}"
  );
  assert!(
    code.contains("\"x-request-id\""),
    "should contain original header: {code}"
  );
}

#[test]
fn test_header_constants_fragment_multiple() {
  let fragment = HeaderConstantsFragment::new(vec![
    HttpHeaderRef::from("x-request-id"),
    HttpHeaderRef::from("x-correlation-id"),
    HttpHeaderRef::from("content-type"),
  ]);
  let code = fragment.into_token_stream().to_string();

  assert!(code.contains("X_REQUEST_ID"));
  assert!(code.contains("X_CORRELATION_ID"));
  assert!(code.contains("CONTENT_TYPE"));
}

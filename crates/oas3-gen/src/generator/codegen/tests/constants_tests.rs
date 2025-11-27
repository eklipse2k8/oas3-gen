use crate::generator::{
  ast::{
    FieldDef, RustType, StructDef, StructToken, TypeAliasDef, TypeRef, ValidationAttribute, TypeAliasToken,
    tokens::{FieldNameToken, HeaderToken},
  },
  codegen::constants::{generate_header_constants, generate_regex_constants},
};

fn make_field(name: &str, pattern: Option<&str>) -> FieldDef {
  FieldDef {
    name: FieldNameToken::new(name),
    validation_attrs: pattern
      .map(|p| vec![ValidationAttribute::Regex(p.to_string())])
      .unwrap_or_default(),
    ..Default::default()
  }
}

fn make_struct(name: &str, fields: Vec<FieldDef>) -> RustType {
  RustType::Struct(StructDef {
    name: StructToken::new(name),
    fields,
    ..Default::default()
  })
}

#[test]
fn test_generate_regex_constants_empty() {
  let types: Vec<&RustType> = vec![];
  let (tokens, lookup) = generate_regex_constants(&types);

  assert!(lookup.is_empty());
  assert!(tokens.is_empty());
}

#[test]
fn test_generate_regex_constants_no_regex_fields() {
  let struct_type = make_struct("User", vec![make_field("name", None), make_field("email", None)]);
  let types: Vec<&RustType> = vec![&struct_type];
  let (tokens, lookup) = generate_regex_constants(&types);

  assert!(lookup.is_empty());
  assert!(tokens.is_empty());
}

#[test]
fn test_generate_regex_constants_single_field() {
  let struct_type = make_struct("User", vec![make_field("email", Some(r"^[\w.-]+@[\w.-]+$"))]);
  let types: Vec<&RustType> = vec![&struct_type];
  let (tokens, lookup) = generate_regex_constants(&types);

  assert_eq!(lookup.len(), 1);
  let code = tokens.to_string();
  assert!(
    code.contains("REGEX_USER_EMAIL"),
    "should contain constant name: {code}"
  );
  assert!(code.contains("LazyLock"), "should use LazyLock: {code}");
  assert!(code.contains("Regex :: new"), "should create regex: {code}");
}

#[test]
fn test_generate_regex_constants_multiple_fields_same_struct() {
  let struct_type = make_struct(
    "User",
    vec![
      make_field("email", Some(r"^[\w.-]+@[\w.-]+$")),
      make_field("phone", Some(r"^\+?[0-9]{10,15}$")),
    ],
  );
  let types: Vec<&RustType> = vec![&struct_type];
  let (tokens, lookup) = generate_regex_constants(&types);

  assert_eq!(lookup.len(), 2);
  let code = tokens.to_string();
  assert!(code.contains("REGEX_USER_EMAIL"));
  assert!(code.contains("REGEX_USER_PHONE"));
}

#[test]
fn test_generate_regex_constants_deduplicates_patterns() {
  let email_pattern = r"^[\w.-]+@[\w.-]+$";
  let struct1 = make_struct("User", vec![make_field("email", Some(email_pattern))]);
  let struct2 = make_struct("Contact", vec![make_field("email", Some(email_pattern))]);
  let types: Vec<&RustType> = vec![&struct1, &struct2];
  let (tokens, lookup) = generate_regex_constants(&types);

  assert_eq!(lookup.len(), 2, "both fields should be in lookup");

  let const_names: std::collections::HashSet<_> = lookup.values().collect();
  assert_eq!(const_names.len(), 1, "both should reference same constant");

  let code = tokens.to_string();
  let static_count = code.matches("static REGEX_").count();
  assert_eq!(static_count, 1, "should only generate one regex constant");
}

#[test]
fn test_generate_regex_constants_skips_non_structs() {
  let struct_type = make_struct("User", vec![make_field("email", Some(r"pattern"))]);
  let alias = RustType::TypeAlias(TypeAliasDef {
    name: TypeAliasToken::new("UserId"),
    target: TypeRef::new("String"),
    ..Default::default()
  });
  let types: Vec<&RustType> = vec![&struct_type, &alias];
  let (_, lookup) = generate_regex_constants(&types);

  assert_eq!(lookup.len(), 1);
}

#[test]
fn test_generate_header_constants_empty() {
  let headers: Vec<HeaderToken> = vec![];
  let tokens = generate_header_constants(&headers);

  assert!(tokens.is_empty());
}

#[test]
fn test_generate_header_constants_single() {
  let headers: Vec<HeaderToken> = vec![HeaderToken::from("x-request-id")];
  let tokens = generate_header_constants(&headers);
  let code = tokens.to_string();

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
fn test_generate_header_constants_multiple() {
  let headers: Vec<HeaderToken> = vec![
    HeaderToken::from("x-request-id"),
    HeaderToken::from("x-correlation-id"),
    HeaderToken::from("content-type"),
  ];
  let tokens = generate_header_constants(&headers);
  let code = tokens.to_string();

  assert!(code.contains("X_REQUEST_ID"));
  assert!(code.contains("X_CORRELATION_ID"));
  assert!(code.contains("CONTENT_TYPE"));
}

use std::collections::BTreeMap;

use crate::generator::{
  ast::{
    ContentCategory, Documentation, EnumToken, EnumVariantToken, FieldDef, FieldNameToken, MethodNameToken,
    ResponseMediaType, ResponseStatusCategory, ResponseVariant, ResponseVariantCategory, RustType, StatusCodeToken,
    StatusHandler, StructDef, StructKind, StructMethod, StructMethodKind, StructToken, TypeRef, ValidationAttribute,
  },
  codegen::{self, Visibility, structs},
};

fn base_struct(kind: StructKind) -> StructDef {
  StructDef {
    name: StructToken::new("Sample"),
    docs: Documentation::from_lines(["Sample struct"]),
    fields: vec![
      FieldDef::builder()
        .name(FieldNameToken::new("field"))
        .rust_type(TypeRef::new("String"))
        .validation_attrs(vec![ValidationAttribute::Length {
          min: Some(1),
          max: None,
        }])
        .build(),
    ],
    serde_attrs: vec![],
    outer_attrs: vec![],
    methods: vec![],
    kind,
    ..Default::default()
  }
}

fn make_response_parser_struct(variant: ResponseVariant) -> StructDef {
  let mut def = base_struct(StructKind::OperationRequest);
  let category = variant
    .media_types
    .first()
    .map_or(ContentCategory::Json, |m| m.category);
  let status_code = variant.status_code;

  def.methods.push(StructMethod {
    name: MethodNameToken::new("parse_response"),
    docs: Documentation::from_lines(["Parse response"]),
    kind: StructMethodKind::ParseResponse {
      response_enum: EnumToken::new("ResponseEnum"),
      status_handlers: vec![StatusHandler {
        status_code,
        dispatch: ResponseStatusCategory::Single(ResponseVariantCategory { category, variant }),
      }],
      default_handler: None,
    },
  });
  def
}

#[test]
fn generates_struct_with_supplied_derives() {
  let def = base_struct(StructKind::Schema);
  let tokens = structs::StructGenerator::new(&def, &BTreeMap::new(), Visibility::Public).emit();
  let code = tokens.to_string();
  assert!(code.contains("derive"), "missing derive attribute");
  assert!(code.contains("Debug"), "missing Debug derive");
  assert!(code.contains("Clone"), "missing Clone derive");
  assert!(code.contains("pub struct Sample"), "missing struct declaration");
}

#[test]
fn test_validation_attribute_generation() {
  let cases = [(true, true, "validation present"), (false, false, "validation absent")];
  for (has_validation, should_contain_validate, desc) in cases {
    let mut def = base_struct(StructKind::Schema);
    if !has_validation {
      def.fields[0].validation_attrs.clear();
    }
    let tokens = structs::StructGenerator::new(&def, &BTreeMap::new(), Visibility::Public).emit();
    let code = tokens.to_string();
    assert_eq!(
      code.contains("validate"),
      should_contain_validate,
      "validation attribute mismatch for case: {desc}"
    );
  }
}

#[test]
fn renders_response_parser_method() {
  let def = make_response_parser_struct(
    ResponseVariant::builder()
      .status_code(StatusCodeToken::Ok200)
      .variant_name(EnumVariantToken::new("Ok"))
      .media_types(vec![ResponseMediaType::new("application/json")])
      .build(),
  );
  let tokens = structs::StructGenerator::new(&def, &BTreeMap::new(), Visibility::Public).emit();
  let code = tokens.to_string();
  assert!(code.contains("fn parse_response"), "missing parse_response method");
  assert!(code.contains("ResponseEnum"), "missing ResponseEnum type");
}

#[test]
fn test_text_response_parsing() {
  let cases = [
    (
      TypeRef::new("String"),
      "req . text () . await ?",
      "text/plain String response",
    ),
    (
      TypeRef::new("i32"),
      "req . text () . await ? . parse :: < i32 > () ?",
      "text/plain i32 response with parsing",
    ),
  ];
  for (st, expected_code, desc) in cases {
    let def = make_response_parser_struct(
      ResponseVariant::builder()
        .status_code(StatusCodeToken::Ok200)
        .variant_name(EnumVariantToken::new("Ok"))
        .media_types(vec![ResponseMediaType::with_schema("text/plain", Some(st.clone()))])
        .maybe_schema_type(Some(st))
        .build(),
    );
    let tokens = structs::StructGenerator::new(&def, &BTreeMap::new(), Visibility::Public).emit();
    let code = tokens.to_string();
    assert!(code.contains(expected_code), "missing expected code for {desc}");
    assert!(
      code.contains("Ok (ResponseEnum :: Ok (data))"),
      "missing success return for {desc}"
    );
  }
}

#[test]
fn renders_json_parser_for_custom_struct() {
  let def = make_response_parser_struct(
    ResponseVariant::builder()
      .status_code(StatusCodeToken::Ok200)
      .variant_name(EnumVariantToken::new("Ok"))
      .media_types(vec![ResponseMediaType::with_schema(
        "application/json",
        Some(TypeRef::new("MyStruct")),
      )])
      .schema_type(TypeRef::new("MyStruct"))
      .build(),
  );
  let tokens = structs::StructGenerator::new(&def, &BTreeMap::new(), Visibility::Public).emit();
  let code = tokens.to_string();
  assert!(
    code.contains("json_with_diagnostics"),
    "missing json_with_diagnostics call"
  );
  assert!(code.contains("MyStruct"), "missing MyStruct type");
}

#[test]
fn test_binary_response_parsing() {
  let def = make_response_parser_struct(
    ResponseVariant::builder()
      .status_code(StatusCodeToken::Ok200)
      .variant_name(EnumVariantToken::new("Ok"))
      .media_types(vec![ResponseMediaType::with_schema(
        "application/octet-stream",
        Some(TypeRef::new("Vec<u8>")),
      )])
      .schema_type(TypeRef::new("Vec<u8>"))
      .build(),
  );
  let tokens = structs::StructGenerator::new(&def, &BTreeMap::new(), Visibility::Public).emit();
  let code = tokens.to_string();
  assert!(
    code.contains("req . bytes () . await ? . to_vec ()"),
    "missing bytes conversion for binary content"
  );
}

#[test]
fn test_event_stream_response_generates_from_response() {
  let def = make_response_parser_struct(
    ResponseVariant::builder()
      .status_code(StatusCodeToken::Ok200)
      .variant_name(EnumVariantToken::new("Ok"))
      .media_types(vec![ResponseMediaType::with_schema(
        "text/event-stream",
        Some(TypeRef::new("StreamEvent")),
      )])
      .schema_type(TypeRef::new("oas3_gen_support::EventStream<StreamEvent>"))
      .build(),
  );
  let tokens = structs::StructGenerator::new(&def, &BTreeMap::new(), Visibility::Public).emit();
  let code = tokens.to_string();
  assert!(
    code.contains("from_response"),
    "EventStream response should call from_response: {code}"
  );
}

#[test]
fn test_serde_import_generation() {
  let def = base_struct(StructKind::Schema);
  let tokens = codegen::generate(&[RustType::Struct(def)], Visibility::Public);
  let code = tokens.to_string();
  assert!(code.contains("Debug"), "missing Debug derive");
  assert!(code.contains("Clone"), "missing Clone derive");
}

#[test]
fn test_header_params_struct_generates_try_from_header_map() {
  let def = StructDef {
    name: StructToken::new("RequestHeader"),
    docs: Documentation::from_lines(["Header parameters"]),
    fields: vec![
      FieldDef::builder()
        .name(FieldNameToken::new("x_api_key"))
        .rust_type(TypeRef::new("String"))
        .original_name("X-Api-Key")
        .build(),
      FieldDef::builder()
        .name(FieldNameToken::new("x_request_id"))
        .rust_type(TypeRef::new("String").with_option())
        .original_name("X-Request-ID")
        .build(),
    ],
    kind: StructKind::HeaderParams,
    ..Default::default()
  };

  let tokens = structs::StructGenerator::new(&def, &BTreeMap::new(), Visibility::Public).emit();
  let code = tokens.to_string();

  assert!(
    code.contains("impl core :: convert :: TryFrom < & RequestHeader > for http :: HeaderMap"),
    "missing TryFrom impl for reference header: {code}"
  );
  assert!(
    code.contains("impl core :: convert :: TryFrom < RequestHeader > for http :: HeaderMap"),
    "missing TryFrom impl for owned header: {code}"
  );
  assert!(
    code.contains("type Error = http :: header :: InvalidHeaderValue"),
    "missing Error type: {code}"
  );
  assert!(code.contains("X_API_KEY"), "missing X_API_KEY constant: {code}");
  assert!(code.contains("X_REQUEST_ID"), "missing X_REQUEST_ID constant: {code}");
  assert!(
    code.contains("if let Some (value) = & headers . x_request_id"),
    "missing optional field handling: {code}"
  );
}

#[test]
fn test_non_header_params_struct_does_not_generate_try_from_header_map() {
  let def = base_struct(StructKind::Schema);
  let tokens = structs::StructGenerator::new(&def, &BTreeMap::new(), Visibility::Public).emit();
  let code = tokens.to_string();

  assert!(
    !code.contains("impl TryFrom"),
    "Schema struct should not have TryFrom impl: {code}"
  );
}

#[test]
fn test_header_params_with_primitive_types() {
  use crate::generator::ast::RustPrimitive;

  let def = StructDef {
    name: StructToken::new("IntHeader"),
    docs: Documentation::default(),
    fields: vec![
      FieldDef::builder()
        .name(FieldNameToken::new("x_count"))
        .rust_type(TypeRef::new(RustPrimitive::I32))
        .original_name("X-Count")
        .build(),
    ],
    kind: StructKind::HeaderParams,
    ..Default::default()
  };

  let tokens = structs::StructGenerator::new(&def, &BTreeMap::new(), Visibility::Public).emit();
  let code = tokens.to_string();

  assert!(
    code.contains("to_string ()"),
    "primitive types should use to_string() conversion: {code}"
  );
}

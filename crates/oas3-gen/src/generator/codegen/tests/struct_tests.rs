use std::collections::{BTreeMap, HashSet};

use crate::generator::{
  ast::{
    ContentCategory, EnumToken, EnumVariantToken, FieldDef, FieldNameToken, MethodNameToken, ResponseVariant, RustType,
    StatusCodeToken, StructDef, StructKind, StructMethod, StructMethodKind, StructToken, TypeRef, ValidationAttribute,
  },
  codegen::{self, Visibility, structs},
};

fn base_struct(kind: StructKind) -> StructDef {
  StructDef {
    name: StructToken::new("Sample"),
    docs: vec!["Sample struct".to_string()],
    fields: vec![FieldDef {
      name: FieldNameToken::new("field"),
      rust_type: TypeRef::new("String"),
      serde_attrs: vec![],
      validation_attrs: vec![ValidationAttribute::Length {
        min: Some(1),
        max: None,
      }],
      default_value: None,
      ..Default::default()
    }],
    serde_attrs: vec![],
    outer_attrs: vec![],
    methods: vec![],
    kind,
    ..Default::default()
  }
}

fn make_response_parser_struct(variant: ResponseVariant) -> StructDef {
  let mut def = base_struct(StructKind::OperationRequest);
  def.methods.push(StructMethod {
    name: MethodNameToken::new("parse_response"),
    docs: vec!["Parse response".to_string()],
    kind: StructMethodKind::ParseResponse {
      response_enum: EnumToken::new("ResponseEnum"),
      variants: vec![variant],
    },
  });
  def
}

#[test]
fn generates_struct_with_supplied_derives() {
  let def = base_struct(StructKind::Schema);
  let tokens = structs::StructGenerator::new(&BTreeMap::new(), Visibility::Public).generate(&def);
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
    let tokens = structs::StructGenerator::new(&BTreeMap::new(), Visibility::Public).generate(&def);
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
  let def = make_response_parser_struct(ResponseVariant {
    status_code: StatusCodeToken::Ok200,
    variant_name: EnumVariantToken::new("Ok"),
    description: None,
    schema_type: None,
    content_category: ContentCategory::Json,
  });
  let tokens = structs::StructGenerator::new(&BTreeMap::new(), Visibility::Public).generate(&def);
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
  for (schema_type, expected_code, desc) in cases {
    let def = make_response_parser_struct(ResponseVariant {
      status_code: StatusCodeToken::Ok200,
      variant_name: EnumVariantToken::new("Ok"),
      description: None,
      schema_type: Some(schema_type),
      content_category: ContentCategory::Text,
    });
    let tokens = structs::StructGenerator::new(&BTreeMap::new(), Visibility::Public).generate(&def);
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
  let def = make_response_parser_struct(ResponseVariant {
    status_code: StatusCodeToken::Ok200,
    variant_name: EnumVariantToken::new("Ok"),
    description: None,
    schema_type: Some(TypeRef::new("MyStruct")),
    content_category: ContentCategory::Json,
  });
  let tokens = structs::StructGenerator::new(&BTreeMap::new(), Visibility::Public).generate(&def);
  let code = tokens.to_string();
  assert!(
    code.contains("json_with_diagnostics"),
    "missing json_with_diagnostics call"
  );
  assert!(code.contains("MyStruct"), "missing MyStruct type");
}

#[test]
fn test_binary_response_parsing() {
  let def = make_response_parser_struct(ResponseVariant {
    status_code: StatusCodeToken::Ok200,
    variant_name: EnumVariantToken::new("Ok"),
    description: None,
    schema_type: Some(TypeRef::new("Vec<u8>")),
    content_category: ContentCategory::Binary,
  });
  let tokens = structs::StructGenerator::new(&BTreeMap::new(), Visibility::Public).generate(&def);
  let code = tokens.to_string();
  assert!(
    code.contains("req . bytes () . await ? . to_vec ()"),
    "missing bytes conversion for binary content"
  );
}

#[test]
fn test_serde_import_generation() {
  let def = base_struct(StructKind::Schema);
  let errors = HashSet::new();
  let tokens = codegen::generate(&[RustType::Struct(def)], &errors, Visibility::Public);
  let code = tokens.to_string();
  assert!(code.contains("Debug"), "missing Debug derive");
  assert!(code.contains("Clone"), "missing Clone derive");
}

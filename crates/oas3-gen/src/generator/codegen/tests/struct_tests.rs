use std::collections::{BTreeMap, BTreeSet, HashSet};

use crate::generator::{
  ast::{
    DeriveTrait, FieldDef, PathSegment, QueryParameter, ResponseVariant, RustType, StructDef, StructKind, StructMethod,
    StructMethodKind, TypeRef, ValidationAttribute,
  },
  codegen::{self, Visibility, structs},
};

fn base_struct(kind: StructKind) -> StructDef {
  StructDef {
    name: "Sample".to_string(),
    docs: vec!["/// Sample struct".to_string()],
    fields: vec![FieldDef {
      name: "field".to_string(),
      rust_type: TypeRef::new("String"),
      serde_attrs: vec![],
      extra_attrs: vec![],
      validation_attrs: vec![ValidationAttribute::Length {
        min: Some(1),
        max: None,
      }],
      default_value: None,
      ..Default::default()
    }],
    derives: BTreeSet::from([DeriveTrait::Debug, DeriveTrait::Clone]),
    serde_attrs: vec![],
    outer_attrs: vec![],
    methods: vec![],
    kind,
  }
}

fn make_response_parser_struct(variant: ResponseVariant) -> StructDef {
  let mut def = base_struct(StructKind::OperationRequest);
  def.methods.push(StructMethod {
    name: "parse_response".to_string(),
    docs: vec!["/// Parse response".to_string()],
    kind: StructMethodKind::ParseResponse {
      response_enum: "ResponseEnum".to_string(),
      variants: vec![variant],
    },
    attrs: vec![],
  });
  def
}

fn make_path_struct(field_name: &str, rust_type: &str, path_literal: &str) -> StructDef {
  let mut def = base_struct(StructKind::OperationRequest);
  def.fields = vec![FieldDef {
    name: field_name.to_string(),
    rust_type: TypeRef::new(rust_type),
    serde_attrs: vec![],
    extra_attrs: vec![],
    validation_attrs: vec![],
    default_value: None,
    ..Default::default()
  }];
  def.methods.push(StructMethod {
    name: "render_path".to_string(),
    docs: vec![format!("/// Render path with {} parameter", rust_type)],
    kind: StructMethodKind::RenderPath {
      segments: vec![
        PathSegment::Literal(path_literal.to_string()),
        PathSegment::Parameter {
          field: field_name.to_string(),
        },
      ],
      query_params: vec![],
    },
    attrs: vec![],
  });
  def
}

#[test]
fn generates_struct_with_supplied_derives() {
  let def = StructDef {
    derives: BTreeSet::from([DeriveTrait::Debug, DeriveTrait::Clone, DeriveTrait::Serialize]),
    ..base_struct(StructKind::Schema)
  };
  let tokens = structs::generate_struct(&def, &BTreeMap::new(), Visibility::Public);
  let code = tokens.to_string();
  assert!(code.contains("derive"), "missing derive attribute");
  assert!(code.contains("Debug"), "missing Debug derive");
  assert!(code.contains("Clone"), "missing Clone derive");
  assert!(code.contains("Serialize"), "missing Serialize derive");
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
    let tokens = structs::generate_struct(&def, &BTreeMap::new(), Visibility::Public);
    let code = tokens.to_string();
    assert_eq!(
      code.contains("validate"),
      should_contain_validate,
      "validation attribute mismatch for case: {desc}"
    );
  }
}

#[test]
fn renders_struct_methods() {
  let mut def = base_struct(StructKind::OperationRequest);
  def.methods.push(StructMethod {
    name: "render_path".to_string(),
    docs: vec!["/// Render path".to_string()],
    kind: StructMethodKind::RenderPath {
      segments: vec![
        PathSegment::Literal("/users/".to_string()),
        PathSegment::Parameter {
          field: "field".to_string(),
        },
      ],
      query_params: vec![QueryParameter {
        field: "field".to_string(),
        encoded_name: "field".to_string(),
        explode: false,
        optional: false,
        is_array: false,
        style: None,
      }],
    },
    attrs: vec![],
  });
  let tokens = structs::generate_struct(&def, &BTreeMap::new(), Visibility::Public);
  let code = tokens.to_string();
  assert!(code.contains("impl Sample"), "missing impl block");
  assert!(code.contains("fn render_path"), "missing render_path method");
}

#[test]
fn renders_response_parser_method() {
  let def = make_response_parser_struct(ResponseVariant {
    status_code: "200".to_string(),
    variant_name: "Ok".to_string(),
    description: None,
    schema_type: None,
    content_type: None,
  });
  let tokens = structs::generate_struct(&def, &BTreeMap::new(), Visibility::Public);
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
      status_code: "200".to_string(),
      variant_name: "Ok".to_string(),
      description: None,
      schema_type: Some(schema_type),
      content_type: Some("text/plain".to_string()),
    });
    let tokens = structs::generate_struct(&def, &BTreeMap::new(), Visibility::Public);
    let code = tokens.to_string();
    assert!(code.contains(expected_code), "missing expected code for {desc}");
    assert!(
      code.contains("Ok (ResponseEnum :: Ok (data))"),
      "missing success return for {desc}"
    );
  }
}

#[test]
fn renders_default_json_parser_for_unknown_content_type() {
  let def = make_response_parser_struct(ResponseVariant {
    status_code: "200".to_string(),
    variant_name: "Ok".to_string(),
    description: None,
    schema_type: Some(TypeRef::new("MyStruct")),
    content_type: Some("application/octet-stream".to_string()),
  });
  let tokens = structs::generate_struct(&def, &BTreeMap::new(), Visibility::Public);
  let code = tokens.to_string();
  assert!(
    code.contains("json_with_diagnostics"),
    "missing json_with_diagnostics call"
  );
  assert!(code.contains("MyStruct"), "missing MyStruct type");
}

#[test]
fn test_binary_response_parsing() {
  let cases = [("image/png", "image"), ("application/pdf", "pdf")];
  for (content_type, desc) in cases {
    let def = make_response_parser_struct(ResponseVariant {
      status_code: "200".to_string(),
      variant_name: "Ok".to_string(),
      description: None,
      schema_type: Some(TypeRef::new("Vec<u8>")),
      content_type: Some(content_type.to_string()),
    });
    let tokens = structs::generate_struct(&def, &BTreeMap::new(), Visibility::Public);
    let code = tokens.to_string();
    assert!(
      code.contains("req . bytes () . await ? . to_vec ()"),
      "missing bytes conversion for {desc}"
    );
  }
}

#[test]
fn test_serde_import_generation() {
  let cases = [
    (
      BTreeSet::from([DeriveTrait::Debug, DeriveTrait::Deserialize]),
      "use serde :: Deserialize",
      false,
      "Deserialize only",
    ),
    (
      BTreeSet::from([DeriveTrait::Debug, DeriveTrait::Serialize, DeriveTrait::Deserialize]),
      "use serde :: { Deserialize , Serialize }",
      true,
      "both Serialize and Deserialize",
    ),
  ];
  for (derives, expected_import, should_have_serialize, desc) in cases {
    let def = StructDef {
      derives,
      ..base_struct(StructKind::Schema)
    };
    let errors = HashSet::new();
    let tokens = codegen::generate(&[RustType::Struct(def)], &errors, Visibility::Public);
    let code = tokens.to_string();
    assert!(code.contains(expected_import), "missing import for {desc}");
    if !should_have_serialize {
      assert!(!code.contains("Serialize"), "should not contain Serialize for {desc}");
    }
  }
}

#[test]
fn test_path_parameter_types() {
  let cases = [
    ("id", "i64", "/users/", "integer"),
    ("active", "bool", "/items/", "boolean"),
    ("amount", "f64", "/prices/", "float"),
    ("uuid", "uuid::Uuid", "/entities/", "UUID"),
  ];
  for (field_name, rust_type, path_literal, desc) in cases {
    let def = make_path_struct(field_name, rust_type, path_literal);
    let tokens = structs::generate_struct(&def, &BTreeMap::new(), Visibility::Public);
    let code = tokens.to_string();
    assert!(code.contains("fn render_path"), "missing render_path for {desc}");
    assert!(
      code.contains(&format!("serialize_query_param (& self . {field_name})")),
      "missing serialize_query_param for {desc}"
    );
    assert!(
      code.contains("percent_encode_path_segment"),
      "missing percent_encode_path_segment for {desc}"
    );
  }
}

#[test]
fn renders_path_with_mixed_parameters() {
  let mut def = base_struct(StructKind::OperationRequest);
  def.fields = vec![
    FieldDef {
      name: "user_id".to_string(),
      rust_type: TypeRef::new("i64"),
      serde_attrs: vec![],
      extra_attrs: vec![],
      validation_attrs: vec![],
      default_value: None,
      ..Default::default()
    },
    FieldDef {
      name: "post_slug".to_string(),
      rust_type: TypeRef::new("String"),
      serde_attrs: vec![],
      extra_attrs: vec![],
      validation_attrs: vec![],
      default_value: None,
      ..Default::default()
    },
  ];
  def.methods.push(StructMethod {
    name: "render_path".to_string(),
    docs: vec!["/// Render path with mixed parameters".to_string()],
    kind: StructMethodKind::RenderPath {
      segments: vec![
        PathSegment::Literal("/users/".to_string()),
        PathSegment::Parameter {
          field: "user_id".to_string(),
        },
        PathSegment::Literal("/posts/".to_string()),
        PathSegment::Parameter {
          field: "post_slug".to_string(),
        },
      ],
      query_params: vec![],
    },
    attrs: vec![],
  });
  let tokens = structs::generate_struct(&def, &BTreeMap::new(), Visibility::Public);
  let code = tokens.to_string();
  assert!(
    code.contains("serialize_query_param (& self . user_id)"),
    "missing user_id serialization"
  );
  assert!(
    code.contains("serialize_query_param (& self . post_slug)"),
    "missing post_slug serialization"
  );
  assert_eq!(
    code.matches("percent_encode_path_segment").count(),
    2,
    "expected 2 percent_encode_path_segment calls"
  );
}

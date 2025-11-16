use std::collections::BTreeMap;

use crate::generator::{
  ast::{
    FieldDef, PathSegment, QueryParameter, ResponseVariant, RustType, StructDef, StructKind, StructMethod,
    StructMethodKind, TypeRef,
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
      validation_attrs: vec!["length(min = 1)".to_string()],
      regex_validation: None,
      default_value: None,
      ..Default::default()
    }],
    derives: vec!["Debug".to_string(), "Clone".to_string()],
    serde_attrs: vec![],
    outer_attrs: vec![],
    methods: vec![],
    kind,
  }
}

#[test]
fn generates_struct_with_supplied_derives() {
  let def = StructDef {
    derives: vec!["Debug".into(), "Clone".into(), "Serialize".into()],
    ..base_struct(StructKind::Schema)
  };
  let tokens = structs::generate_struct(&def, &BTreeMap::new(), Visibility::Public);
  let code = tokens.to_string();
  assert!(code.contains("derive"));
  assert!(code.contains("Debug"));
  assert!(code.contains("Clone"));
  assert!(code.contains("Serialize"));
  assert!(code.contains("pub struct Sample"));
}

#[test]
fn emits_validation_attributes_when_present() {
  let def = base_struct(StructKind::Schema);
  let tokens = structs::generate_struct(&def, &BTreeMap::new(), Visibility::Public);
  let code = tokens.to_string();
  assert!(code.contains("# [validate"));
}

#[test]
fn skips_validation_attributes_when_absent() {
  let mut def = base_struct(StructKind::Schema);
  def.fields[0].validation_attrs.clear();
  let tokens = structs::generate_struct(&def, &BTreeMap::new(), Visibility::Public);
  let code = tokens.to_string();
  assert!(!code.contains("validate"));
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
      }],
    },
    attrs: vec![],
  });
  let tokens = structs::generate_struct(&def, &BTreeMap::new(), Visibility::Public);
  let code = tokens.to_string();
  assert!(code.contains("impl Sample"));
  assert!(code.contains("fn render_path"));
}

#[test]
fn renders_response_parser_method() {
  let mut def = base_struct(StructKind::OperationRequest);
  def.methods.push(StructMethod {
    name: "parse_response".to_string(),
    docs: vec!["/// Parse response".to_string()],
    kind: StructMethodKind::ParseResponse {
      response_enum: "ResponseEnum".to_string(),
      variants: vec![ResponseVariant {
        status_code: "200".to_string(),
        variant_name: "Ok".to_string(),
        description: None,
        schema_type: None,
      }],
    },
    attrs: vec![],
  });
  let tokens = structs::generate_struct(&def, &BTreeMap::new(), Visibility::Public);
  let code = tokens.to_string();
  assert!(code.contains("fn parse_response"));
  assert!(code.contains("ResponseEnum"));
}

#[test]
fn codegen_emits_only_deserialize_when_needed() {
  let def = StructDef {
    derives: vec!["Debug".into(), "Deserialize".into()],
    ..base_struct(StructKind::Schema)
  };
  let errors = std::collections::HashSet::new();
  let tokens = codegen::generate(&[RustType::Struct(def)], &errors, Visibility::Public);
  let code = tokens.to_string();
  assert!(code.contains("use serde :: Deserialize"));
  assert!(!code.contains("Serialize"));
}

#[test]
fn codegen_emits_both_serde_traits_when_required() {
  let def = StructDef {
    derives: vec!["Debug".into(), "Serialize".into(), "Deserialize".into()],
    ..base_struct(StructKind::Schema)
  };
  let errors = std::collections::HashSet::new();
  let tokens = codegen::generate(&[RustType::Struct(def)], &errors, Visibility::Public);
  let code = tokens.to_string();
  assert!(code.contains("use serde :: { Deserialize , Serialize }"));
}

#[test]
fn renders_path_with_integer_parameter() {
  let mut def = base_struct(StructKind::OperationRequest);
  def.fields = vec![FieldDef {
    name: "id".to_string(),
    rust_type: TypeRef::new("i64"),
    serde_attrs: vec![],
    extra_attrs: vec![],
    validation_attrs: vec![],
    regex_validation: None,
    default_value: None,
    ..Default::default()
  }];
  def.methods.push(StructMethod {
    name: "render_path".to_string(),
    docs: vec!["/// Render path with integer parameter".to_string()],
    kind: StructMethodKind::RenderPath {
      segments: vec![
        PathSegment::Literal("/users/".to_string()),
        PathSegment::Parameter { field: "id".to_string() },
      ],
      query_params: vec![],
    },
    attrs: vec![],
  });
  let tokens = structs::generate_struct(&def, &BTreeMap::new(), Visibility::Public);
  let code = tokens.to_string();
  assert!(code.contains("fn render_path"));
  assert!(code.contains("serialize_query_param"));
  assert!(code.contains("percent_encode_path_segment"));
}

#[test]
fn renders_path_with_boolean_parameter() {
  let mut def = base_struct(StructKind::OperationRequest);
  def.fields = vec![FieldDef {
    name: "active".to_string(),
    rust_type: TypeRef::new("bool"),
    serde_attrs: vec![],
    extra_attrs: vec![],
    validation_attrs: vec![],
    regex_validation: None,
    default_value: None,
    ..Default::default()
  }];
  def.methods.push(StructMethod {
    name: "render_path".to_string(),
    docs: vec!["/// Render path with boolean parameter".to_string()],
    kind: StructMethodKind::RenderPath {
      segments: vec![
        PathSegment::Literal("/items/".to_string()),
        PathSegment::Parameter {
          field: "active".to_string(),
        },
      ],
      query_params: vec![],
    },
    attrs: vec![],
  });
  let tokens = structs::generate_struct(&def, &BTreeMap::new(), Visibility::Public);
  let code = tokens.to_string();
  assert!(code.contains("serialize_query_param (& self . active)"));
  assert!(code.contains("percent_encode_path_segment"));
}

#[test]
fn renders_path_with_float_parameter() {
  let mut def = base_struct(StructKind::OperationRequest);
  def.fields = vec![FieldDef {
    name: "amount".to_string(),
    rust_type: TypeRef::new("f64"),
    serde_attrs: vec![],
    extra_attrs: vec![],
    validation_attrs: vec![],
    regex_validation: None,
    default_value: None,
    ..Default::default()
  }];
  def.methods.push(StructMethod {
    name: "render_path".to_string(),
    docs: vec!["/// Render path with float parameter".to_string()],
    kind: StructMethodKind::RenderPath {
      segments: vec![
        PathSegment::Literal("/prices/".to_string()),
        PathSegment::Parameter {
          field: "amount".to_string(),
        },
      ],
      query_params: vec![],
    },
    attrs: vec![],
  });
  let tokens = structs::generate_struct(&def, &BTreeMap::new(), Visibility::Public);
  let code = tokens.to_string();
  assert!(code.contains("serialize_query_param (& self . amount)"));
  assert!(code.contains("percent_encode_path_segment"));
}

#[test]
fn renders_path_with_uuid_parameter() {
  let mut def = base_struct(StructKind::OperationRequest);
  def.fields = vec![FieldDef {
    name: "uuid".to_string(),
    rust_type: TypeRef::new("uuid::Uuid"),
    serde_attrs: vec![],
    extra_attrs: vec![],
    validation_attrs: vec![],
    regex_validation: None,
    default_value: None,
    ..Default::default()
  }];
  def.methods.push(StructMethod {
    name: "render_path".to_string(),
    docs: vec!["/// Render path with UUID parameter".to_string()],
    kind: StructMethodKind::RenderPath {
      segments: vec![
        PathSegment::Literal("/entities/".to_string()),
        PathSegment::Parameter {
          field: "uuid".to_string(),
        },
      ],
      query_params: vec![],
    },
    attrs: vec![],
  });
  let tokens = structs::generate_struct(&def, &BTreeMap::new(), Visibility::Public);
  let code = tokens.to_string();
  assert!(code.contains("serialize_query_param (& self . uuid)"));
  assert!(code.contains("percent_encode_path_segment"));
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
      regex_validation: None,
      default_value: None,
      ..Default::default()
    },
    FieldDef {
      name: "post_slug".to_string(),
      rust_type: TypeRef::new("String"),
      serde_attrs: vec![],
      extra_attrs: vec![],
      validation_attrs: vec![],
      regex_validation: None,
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
  assert!(code.contains("serialize_query_param (& self . user_id)"));
  assert!(code.contains("serialize_query_param (& self . post_slug)"));
  assert_eq!(code.matches("percent_encode_path_segment").count(), 2);
}

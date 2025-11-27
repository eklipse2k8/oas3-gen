use std::collections::{BTreeMap, BTreeSet};

use crate::generator::{
  analyzer::{TypeUsage, build_type_usage_map, type_graph::TypeDependencyGraph},
  ast::{
    ContentCategory, DeriveTrait, EnumDef, EnumToken, EnumVariantToken, FieldDef, ResponseEnumDef, ResponseVariant,
    RustPrimitive, RustType, StatusCodeToken, StructDef, StructKind, StructToken, TypeAliasDef, TypeRef,
    VariantContent, VariantDef, tokens::FieldNameToken,
  },
};

fn seeds(entries: &[(&str, (bool, bool))]) -> BTreeMap<EnumToken, (bool, bool)> {
  entries
    .iter()
    .map(|(name, flags)| (EnumToken::new(*name), *flags))
    .collect()
}

#[test]
fn test_dependency_graph_simple_struct() {
  let user_struct = RustType::Struct(StructDef {
    name: StructToken::new("User"),
    docs: vec![],
    fields: vec![FieldDef {
      name: FieldNameToken::new("address"),
      rust_type: TypeRef::new(RustPrimitive::Custom("Address".into())),
      ..Default::default()
    }],
    derives: BTreeSet::from([DeriveTrait::Debug, DeriveTrait::Clone]),
    serde_attrs: vec![],
    outer_attrs: vec![],
    methods: vec![],
    kind: StructKind::Schema,
  });

  let address_struct = RustType::Struct(StructDef {
    name: StructToken::new("Address"),
    docs: vec![],
    fields: vec![FieldDef {
      name: FieldNameToken::new("street"),
      rust_type: TypeRef::new(RustPrimitive::String),
      ..Default::default()
    }],
    derives: BTreeSet::from([DeriveTrait::Debug, DeriveTrait::Clone]),
    serde_attrs: vec![],
    outer_attrs: vec![],
    methods: vec![],
    kind: StructKind::Schema,
  });

  let types = vec![user_struct, address_struct];

  let usage_map = build_type_usage_map(seeds(&[]), &types);

  assert_eq!(usage_map.len(), 2);
  assert_eq!(usage_map.get(&EnumToken::new("User")), Some(&TypeUsage::Bidirectional));
  assert_eq!(
    usage_map.get(&EnumToken::new("Address")),
    Some(&TypeUsage::Bidirectional)
  );
}

#[test]
fn test_propagation_request_to_nested() {
  let request_struct = RustType::Struct(StructDef {
    name: StructToken::new("CreateUserRequest"),
    docs: vec![],
    fields: vec![FieldDef {
      name: FieldNameToken::new("user"),
      rust_type: TypeRef::new(RustPrimitive::Custom("User".into())),
      ..Default::default()
    }],
    derives: BTreeSet::from([DeriveTrait::Debug, DeriveTrait::Clone]),
    serde_attrs: vec![],
    outer_attrs: vec![],
    methods: vec![],
    kind: StructKind::RequestBody,
  });

  let user_struct = RustType::Struct(StructDef {
    name: StructToken::new("User"),
    docs: vec![],
    fields: vec![FieldDef {
      name: FieldNameToken::new("name"),
      rust_type: TypeRef::new(RustPrimitive::String),
      ..Default::default()
    }],
    derives: BTreeSet::from([DeriveTrait::Debug, DeriveTrait::Clone]),
    serde_attrs: vec![],
    outer_attrs: vec![],
    methods: vec![],
    kind: StructKind::Schema,
  });

  let types = vec![request_struct, user_struct];
  let usage_map = build_type_usage_map(seeds(&[("CreateUserRequest", (true, false))]), &types);

  assert_eq!(
    usage_map.get(&EnumToken::new("CreateUserRequest")),
    Some(&TypeUsage::RequestOnly)
  );
  assert_eq!(usage_map.get(&EnumToken::new("User")), Some(&TypeUsage::RequestOnly));
}

#[test]
fn test_propagation_response_to_nested() {
  let response_struct = RustType::Struct(StructDef {
    name: StructToken::new("UserResponse"),
    docs: vec![],
    fields: vec![FieldDef {
      name: FieldNameToken::new("user"),
      rust_type: TypeRef::new(RustPrimitive::Custom("User".into())),
      ..Default::default()
    }],
    derives: BTreeSet::from([DeriveTrait::Debug, DeriveTrait::Clone]),
    serde_attrs: vec![],
    outer_attrs: vec![],
    methods: vec![],
    kind: StructKind::Schema,
  });

  let user_struct = RustType::Struct(StructDef {
    name: StructToken::new("User"),
    docs: vec![],
    fields: vec![FieldDef {
      name: FieldNameToken::new("name"),
      rust_type: TypeRef::new(RustPrimitive::String),
      ..Default::default()
    }],
    derives: BTreeSet::from([DeriveTrait::Debug, DeriveTrait::Clone]),
    serde_attrs: vec![],
    outer_attrs: vec![],
    methods: vec![],
    kind: StructKind::Schema,
  });

  let types = vec![response_struct, user_struct];
  let usage_map = build_type_usage_map(seeds(&[("UserResponse", (false, true))]), &types);

  assert_eq!(
    usage_map.get(&EnumToken::new("UserResponse")),
    Some(&TypeUsage::ResponseOnly)
  );
  assert_eq!(usage_map.get(&EnumToken::new("User")), Some(&TypeUsage::ResponseOnly));
}

#[test]
fn test_propagation_bidirectional() {
  let request_struct = RustType::Struct(StructDef {
    name: StructToken::new("UpdateUserRequest"),
    docs: vec![],
    fields: vec![FieldDef {
      name: FieldNameToken::new("user"),
      rust_type: TypeRef::new(RustPrimitive::Custom("User".into())),
      ..Default::default()
    }],
    derives: BTreeSet::from([DeriveTrait::Debug, DeriveTrait::Clone]),
    serde_attrs: vec![],
    outer_attrs: vec![],
    methods: vec![],
    kind: StructKind::RequestBody,
  });

  let response_struct = RustType::Struct(StructDef {
    name: StructToken::new("UserResponse"),
    docs: vec![],
    fields: vec![FieldDef {
      name: FieldNameToken::new("user"),
      rust_type: TypeRef::new(RustPrimitive::Custom("User".into())),
      ..Default::default()
    }],
    derives: BTreeSet::from([DeriveTrait::Debug, DeriveTrait::Clone]),
    serde_attrs: vec![],
    outer_attrs: vec![],
    methods: vec![],
    kind: StructKind::Schema,
  });

  let user_struct = RustType::Struct(StructDef {
    name: StructToken::new("User"),
    docs: vec![],
    fields: vec![FieldDef {
      name: FieldNameToken::new("name"),
      rust_type: TypeRef::new(RustPrimitive::String),
      ..Default::default()
    }],
    derives: BTreeSet::from([DeriveTrait::Debug, DeriveTrait::Clone]),
    serde_attrs: vec![],
    outer_attrs: vec![],
    methods: vec![],
    kind: StructKind::Schema,
  });

  let types = vec![request_struct, response_struct, user_struct];
  let usage_map = build_type_usage_map(
    seeds(&[("UpdateUserRequest", (true, false)), ("UserResponse", (false, true))]),
    &types,
  );

  assert_eq!(
    usage_map.get(&EnumToken::new("UpdateUserRequest")),
    Some(&TypeUsage::RequestOnly)
  );
  assert_eq!(
    usage_map.get(&EnumToken::new("UserResponse")),
    Some(&TypeUsage::ResponseOnly)
  );
  assert_eq!(usage_map.get(&EnumToken::new("User")), Some(&TypeUsage::Bidirectional));
}

#[test]
fn test_transitive_dependency_chain() {
  let a_struct = RustType::Struct(StructDef {
    name: StructToken::new("A"),
    docs: vec![],
    fields: vec![FieldDef {
      name: FieldNameToken::new("b"),
      rust_type: TypeRef::new(RustPrimitive::Custom("B".into())),
      ..Default::default()
    }],
    derives: BTreeSet::from([DeriveTrait::Debug]),
    serde_attrs: vec![],
    outer_attrs: vec![],
    methods: vec![],
    kind: StructKind::Schema,
  });

  let b_struct = RustType::Struct(StructDef {
    name: StructToken::new("B"),
    docs: vec![],
    fields: vec![FieldDef {
      name: FieldNameToken::new("c"),
      rust_type: TypeRef::new(RustPrimitive::Custom("C".into())),
      ..Default::default()
    }],
    derives: BTreeSet::from([DeriveTrait::Debug]),
    serde_attrs: vec![],
    outer_attrs: vec![],
    methods: vec![],
    kind: StructKind::Schema,
  });

  let c_struct = RustType::Struct(StructDef {
    name: StructToken::new("C"),
    docs: vec![],
    fields: vec![FieldDef {
      name: FieldNameToken::new("value"),
      rust_type: TypeRef::new(RustPrimitive::String),
      ..Default::default()
    }],
    derives: BTreeSet::from([DeriveTrait::Debug]),
    serde_attrs: vec![],
    outer_attrs: vec![],
    methods: vec![],
    kind: StructKind::Schema,
  });

  let types = vec![a_struct, b_struct, c_struct];
  let usage_map = build_type_usage_map(seeds(&[("A", (false, true))]), &types);

  assert_eq!(usage_map.get(&EnumToken::new("A")), Some(&TypeUsage::ResponseOnly));
  assert_eq!(usage_map.get(&EnumToken::new("B")), Some(&TypeUsage::ResponseOnly));
  assert_eq!(usage_map.get(&EnumToken::new("C")), Some(&TypeUsage::ResponseOnly));
}

#[test]
fn test_enum_with_tuple_variant() {
  let enum_def = RustType::Enum(EnumDef {
    name: EnumToken::new("Result"),
    docs: vec![],
    variants: vec![VariantDef {
      name: EnumVariantToken::new("Success"),
      docs: vec![],
      content: VariantContent::Tuple(vec![TypeRef::new(RustPrimitive::Custom("User".into()))]),
      serde_attrs: vec![],
      deprecated: false,
    }],
    discriminator: None,
    derives: BTreeSet::from([DeriveTrait::Debug]),
    serde_attrs: vec![],
    outer_attrs: vec![],
    case_insensitive: false,
    methods: vec![],
  });

  let user_struct = RustType::Struct(StructDef {
    name: StructToken::new("User"),
    docs: vec![],
    fields: vec![FieldDef {
      name: FieldNameToken::new("name"),
      rust_type: TypeRef::new(RustPrimitive::String),
      ..Default::default()
    }],
    derives: BTreeSet::from([DeriveTrait::Debug]),
    serde_attrs: vec![],
    outer_attrs: vec![],
    methods: vec![],
    kind: StructKind::Schema,
  });

  let types = vec![enum_def, user_struct];
  let usage_map = build_type_usage_map(seeds(&[("Result", (false, true))]), &types);

  assert_eq!(usage_map.get(&EnumToken::new("Result")), Some(&TypeUsage::ResponseOnly));
  assert_eq!(usage_map.get(&EnumToken::new("User")), Some(&TypeUsage::ResponseOnly));
}

#[test]
fn test_type_alias_dependency() {
  let alias = RustType::TypeAlias(TypeAliasDef {
    name: "UserId".to_string(),
    docs: vec![],
    target: TypeRef::new(RustPrimitive::Custom("User".into())),
  });

  let user_struct = RustType::Struct(StructDef {
    name: StructToken::new("User"),
    docs: vec![],
    fields: vec![FieldDef {
      name: FieldNameToken::new("id"),
      rust_type: TypeRef::new(RustPrimitive::I64),
      ..Default::default()
    }],
    derives: BTreeSet::from([DeriveTrait::Debug]),
    serde_attrs: vec![],
    outer_attrs: vec![],
    methods: vec![],
    kind: StructKind::Schema,
  });

  let types = vec![alias, user_struct];
  let usage_map = build_type_usage_map(seeds(&[("UserId", (false, true))]), &types);

  assert_eq!(usage_map.get(&EnumToken::new("UserId")), Some(&TypeUsage::ResponseOnly));
  assert_eq!(usage_map.get(&EnumToken::new("User")), Some(&TypeUsage::ResponseOnly));
}

#[test]
fn test_no_propagation_without_operations() {
  let user_struct = RustType::Struct(StructDef {
    name: StructToken::new("User"),
    docs: vec![],
    fields: vec![FieldDef {
      name: FieldNameToken::new("address"),
      rust_type: TypeRef::new(RustPrimitive::Custom("Address".into())),
      ..Default::default()
    }],
    derives: BTreeSet::from([DeriveTrait::Debug]),
    serde_attrs: vec![],
    outer_attrs: vec![],
    methods: vec![],
    kind: StructKind::Schema,
  });

  let address_struct = RustType::Struct(StructDef {
    name: StructToken::new("Address"),
    docs: vec![],
    fields: vec![],
    derives: BTreeSet::from([DeriveTrait::Debug]),
    serde_attrs: vec![],
    outer_attrs: vec![],
    methods: vec![],
    kind: StructKind::Schema,
  });

  let types = vec![user_struct, address_struct];
  let usage_map = build_type_usage_map(seeds(&[]), &types);

  assert_eq!(usage_map.len(), 2);
  assert_eq!(usage_map.get(&EnumToken::new("User")), Some(&TypeUsage::Bidirectional));
  assert_eq!(
    usage_map.get(&EnumToken::new("Address")),
    Some(&TypeUsage::Bidirectional)
  );
}

#[test]
fn test_cyclic_dependency_handling() {
  let a_struct = RustType::Struct(StructDef {
    name: StructToken::new("A"),
    docs: vec![],
    fields: vec![FieldDef {
      name: FieldNameToken::new("b"),
      rust_type: TypeRef::new(RustPrimitive::Custom("B".into())).with_boxed(),
      ..Default::default()
    }],
    derives: BTreeSet::from([DeriveTrait::Debug]),
    serde_attrs: vec![],
    outer_attrs: vec![],
    methods: vec![],
    kind: StructKind::Schema,
  });

  let b_struct = RustType::Struct(StructDef {
    name: StructToken::new("B"),
    docs: vec![],
    fields: vec![FieldDef {
      name: FieldNameToken::new("a"),
      rust_type: TypeRef::new(RustPrimitive::Custom("A".into())).with_boxed(),
      ..Default::default()
    }],
    derives: BTreeSet::from([DeriveTrait::Debug]),
    serde_attrs: vec![],
    outer_attrs: vec![],
    methods: vec![],
    kind: StructKind::Schema,
  });

  let types = vec![a_struct, b_struct];
  let usage_map = build_type_usage_map(seeds(&[("A", (false, true))]), &types);

  assert_eq!(usage_map.get(&EnumToken::new("A")), Some(&TypeUsage::ResponseOnly));
  assert_eq!(usage_map.get(&EnumToken::new("B")), Some(&TypeUsage::ResponseOnly));
}

#[test]
fn test_response_enum_does_not_propagate_to_request_type() {
  let request_struct = RustType::Struct(StructDef {
    name: StructToken::new("CreateUserRequestParams"),
    docs: vec![],
    fields: vec![FieldDef {
      name: FieldNameToken::new("name"),
      rust_type: TypeRef::new(RustPrimitive::String),
      ..Default::default()
    }],
    derives: BTreeSet::from([DeriveTrait::Debug]),
    serde_attrs: vec![],
    outer_attrs: vec![],
    methods: vec![],
    kind: StructKind::OperationRequest,
  });

  let response_struct = RustType::Struct(StructDef {
    name: StructToken::new("User"),
    docs: vec![],
    fields: vec![FieldDef {
      name: FieldNameToken::new("id"),
      rust_type: TypeRef::new(RustPrimitive::String),
      ..Default::default()
    }],
    derives: BTreeSet::from([DeriveTrait::Debug]),
    serde_attrs: vec![],
    outer_attrs: vec![],
    methods: vec![],
    kind: StructKind::Schema,
  });

  let response_enum = RustType::ResponseEnum(ResponseEnumDef {
    name: EnumToken::new("CreateUserResponseEnum"),
    docs: vec![],
    request_type: Some(StructToken::new("CreateUserRequestParams")),
    variants: vec![ResponseVariant {
      status_code: StatusCodeToken::Ok200,
      variant_name: EnumVariantToken::new("Ok"),
      description: None,
      schema_type: Some(TypeRef::new(RustPrimitive::Custom("User".into()))),
      content_category: ContentCategory::Json,
    }],
  });

  let types = vec![request_struct, response_struct, response_enum];
  let seed = seeds(&[
    ("CreateUserRequestParams", (true, false)),
    ("CreateUserResponseEnum", (false, true)),
  ]);
  let usage_map = build_type_usage_map(seed, &types);

  assert_eq!(
    usage_map.get(&EnumToken::new("CreateUserRequestParams")),
    Some(&TypeUsage::RequestOnly)
  );
  assert_eq!(usage_map.get(&EnumToken::new("User")), Some(&TypeUsage::ResponseOnly));
  assert_eq!(
    usage_map.get(&EnumToken::new("CreateUserResponseEnum")),
    Some(&TypeUsage::ResponseOnly)
  );
}

#[test]
fn test_response_enum_propagates_to_variant_types_only() {
  let response_a = RustType::Struct(StructDef {
    name: StructToken::new("ResponseA"),
    docs: vec![],
    fields: vec![],
    derives: BTreeSet::new(),
    serde_attrs: vec![],
    outer_attrs: vec![],
    methods: vec![],
    kind: StructKind::Schema,
  });

  let response_b = RustType::Struct(StructDef {
    name: StructToken::new("ResponseB"),
    docs: vec![],
    fields: vec![],
    derives: BTreeSet::new(),
    serde_attrs: vec![],
    outer_attrs: vec![],
    methods: vec![],
    kind: StructKind::Schema,
  });

  let request_struct = RustType::Struct(StructDef {
    name: StructToken::new("RequestParams"),
    docs: vec![],
    fields: vec![],
    derives: BTreeSet::new(),
    serde_attrs: vec![],
    outer_attrs: vec![],
    methods: vec![],
    kind: StructKind::OperationRequest,
  });

  let response_enum = RustType::ResponseEnum(ResponseEnumDef {
    name: EnumToken::new("MyResponseEnum"),
    docs: vec![],
    request_type: Some(StructToken::new("RequestParams")),
    variants: vec![
      ResponseVariant {
        status_code: StatusCodeToken::Ok200,
        variant_name: EnumVariantToken::new("Ok"),
        description: None,
        schema_type: Some(TypeRef::new(RustPrimitive::Custom("ResponseA".into()))),
        content_category: ContentCategory::Json,
      },
      ResponseVariant {
        status_code: StatusCodeToken::BadRequest400,
        variant_name: EnumVariantToken::new("BadRequest"),
        description: None,
        schema_type: Some(TypeRef::new(RustPrimitive::Custom("ResponseB".into()))),
        content_category: ContentCategory::Json,
      },
    ],
  });

  let types = vec![response_a, response_b, request_struct, response_enum];
  let seed = seeds(&[("RequestParams", (true, false)), ("MyResponseEnum", (false, true))]);
  let usage_map = build_type_usage_map(seed, &types);

  assert_eq!(
    usage_map.get(&EnumToken::new("RequestParams")),
    Some(&TypeUsage::RequestOnly)
  );
  assert_eq!(
    usage_map.get(&EnumToken::new("ResponseA")),
    Some(&TypeUsage::ResponseOnly)
  );
  assert_eq!(
    usage_map.get(&EnumToken::new("ResponseB")),
    Some(&TypeUsage::ResponseOnly)
  );
  assert_eq!(
    usage_map.get(&EnumToken::new("MyResponseEnum")),
    Some(&TypeUsage::ResponseOnly)
  );
}

#[test]
fn test_request_body_chain_with_response_enum() {
  let request_body_struct = RustType::Struct(StructDef {
    name: StructToken::new("CreateChatCompletionRequest"),
    docs: vec![],
    fields: vec![FieldDef {
      name: FieldNameToken::new("model"),
      rust_type: TypeRef::new(RustPrimitive::Custom("ModelIds".into())),
      ..Default::default()
    }],
    derives: BTreeSet::new(),
    serde_attrs: vec![],
    outer_attrs: vec![],
    methods: vec![],
    kind: StructKind::Schema,
  });

  let model_enum = RustType::Enum(EnumDef {
    name: EnumToken::new("ModelIds"),
    docs: vec![],
    variants: vec![VariantDef {
      name: EnumVariantToken::new("Gpt4"),
      docs: vec![],
      content: VariantContent::Unit,
      serde_attrs: vec![],
      deprecated: false,
    }],
    discriminator: None,
    derives: BTreeSet::new(),
    serde_attrs: vec![],
    outer_attrs: vec![],
    case_insensitive: false,
    methods: vec![],
  });

  let request_body_alias = RustType::TypeAlias(TypeAliasDef {
    name: "CreateChatCompletionRequestBody".to_string(),
    docs: vec![],
    target: TypeRef::new(RustPrimitive::Custom("CreateChatCompletionRequest".into())),
  });

  let request_params = RustType::Struct(StructDef {
    name: StructToken::new("CreateChatCompletionRequestParams"),
    docs: vec![],
    fields: vec![FieldDef {
      name: FieldNameToken::new("body"),
      rust_type: TypeRef::new(RustPrimitive::Custom("CreateChatCompletionRequestBody".into())),
      ..Default::default()
    }],
    derives: BTreeSet::new(),
    serde_attrs: vec![],
    outer_attrs: vec![],
    methods: vec![],
    kind: StructKind::OperationRequest,
  });

  let response_struct = RustType::Struct(StructDef {
    name: StructToken::new("CreateChatCompletionResponse"),
    docs: vec![],
    fields: vec![],
    derives: BTreeSet::new(),
    serde_attrs: vec![],
    outer_attrs: vec![],
    methods: vec![],
    kind: StructKind::Schema,
  });

  let response_enum = RustType::ResponseEnum(ResponseEnumDef {
    name: EnumToken::new("CreateChatCompletionResponseEnum"),
    docs: vec![],
    request_type: Some(StructToken::new("CreateChatCompletionRequestParams")),
    variants: vec![ResponseVariant {
      status_code: StatusCodeToken::Ok200,
      variant_name: EnumVariantToken::new("Ok"),
      description: None,
      schema_type: Some(TypeRef::new(RustPrimitive::Custom(
        "CreateChatCompletionResponse".into(),
      ))),
      content_category: ContentCategory::Json,
    }],
  });

  let types = vec![
    request_body_struct,
    model_enum,
    request_body_alias,
    request_params,
    response_struct,
    response_enum,
  ];

  let seed = seeds(&[
    ("CreateChatCompletionRequest", (true, false)),
    ("CreateChatCompletionRequestBody", (true, false)),
    ("CreateChatCompletionRequestParams", (true, false)),
    ("CreateChatCompletionResponseEnum", (false, true)),
  ]);

  let usage_map = build_type_usage_map(seed, &types);

  assert_eq!(
    usage_map.get(&EnumToken::new("CreateChatCompletionRequest")),
    Some(&TypeUsage::RequestOnly),
    "Request body schema should remain request-only"
  );
  assert_eq!(
    usage_map.get(&EnumToken::new("ModelIds")),
    Some(&TypeUsage::RequestOnly),
    "Model enum should remain request-only"
  );
  assert_eq!(
    usage_map.get(&EnumToken::new("CreateChatCompletionRequestBody")),
    Some(&TypeUsage::RequestOnly),
    "Request body alias should remain request-only"
  );
  assert_eq!(
    usage_map.get(&EnumToken::new("CreateChatCompletionRequestParams")),
    Some(&TypeUsage::RequestOnly),
    "Request params should remain request-only despite ResponseEnum reference"
  );
  assert_eq!(
    usage_map.get(&EnumToken::new("CreateChatCompletionResponse")),
    Some(&TypeUsage::ResponseOnly),
    "Response should be response-only"
  );
  assert_eq!(
    usage_map.get(&EnumToken::new("CreateChatCompletionResponseEnum")),
    Some(&TypeUsage::ResponseOnly),
    "Response enum should be response-only"
  );
}

#[test]
fn test_response_enum_dependency_extraction() {
  let request_struct = RustType::Struct(StructDef {
    name: StructToken::new("RequestParams"),
    docs: vec![],
    fields: vec![],
    derives: BTreeSet::new(),
    serde_attrs: vec![],
    outer_attrs: vec![],
    methods: vec![],
    kind: StructKind::OperationRequest,
  });

  let response_a = RustType::Struct(StructDef {
    name: StructToken::new("ResponseA"),
    docs: vec![],
    fields: vec![],
    derives: BTreeSet::new(),
    serde_attrs: vec![],
    outer_attrs: vec![],
    methods: vec![],
    kind: StructKind::Schema,
  });

  let response_b = RustType::Struct(StructDef {
    name: StructToken::new("ResponseB"),
    docs: vec![],
    fields: vec![],
    derives: BTreeSet::new(),
    serde_attrs: vec![],
    outer_attrs: vec![],
    methods: vec![],
    kind: StructKind::Schema,
  });

  let response_enum = RustType::ResponseEnum(ResponseEnumDef {
    name: EnumToken::new("MyResponseEnum"),
    docs: vec![],
    request_type: Some(StructToken::new("RequestParams")),
    variants: vec![
      ResponseVariant {
        status_code: StatusCodeToken::Ok200,
        variant_name: EnumVariantToken::new("Ok"),
        description: None,
        schema_type: Some(TypeRef::new(RustPrimitive::Custom("ResponseA".into()))),
        content_category: ContentCategory::Json,
      },
      ResponseVariant {
        status_code: StatusCodeToken::BadRequest400,
        variant_name: EnumVariantToken::new("BadRequest"),
        description: None,
        schema_type: Some(TypeRef::new(RustPrimitive::Custom("ResponseB".into()))),
        content_category: ContentCategory::Json,
      },
    ],
  });

  let types = vec![request_struct, response_a, response_b, response_enum];
  let dep_graph = TypeDependencyGraph::build(&types);

  let response_enum_deps = dep_graph.get_dependencies("MyResponseEnum");
  assert!(response_enum_deps.is_some(), "ResponseEnum should have dependencies");

  let deps = response_enum_deps.unwrap();
  assert!(deps.contains("ResponseA"), "Should depend on ResponseA variant");
  assert!(deps.contains("ResponseB"), "Should depend on ResponseB variant");
  assert!(
    !deps.contains("RequestParams"),
    "Should NOT depend on request_type field - this was the bug!"
  );
}

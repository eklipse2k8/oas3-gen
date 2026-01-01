use std::collections::BTreeMap;

use super::build_type_usage_map;
use crate::generator::{
  analyzer::{DependencyGraph, TypeUsage},
  ast::{
    EnumDef, EnumToken, EnumVariantToken, FieldDef, ResponseEnumDef, ResponseMediaType, ResponseVariant, RustPrimitive,
    RustType, StatusCodeToken, StructDef, StructKind, StructToken, TypeAliasDef, TypeAliasToken, TypeRef,
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
    fields: vec![
      FieldDef::builder()
        .name(FieldNameToken::new("address"))
        .rust_type(TypeRef::new(RustPrimitive::Custom("Address".into())))
        .build(),
    ],
    kind: StructKind::Schema,
    ..Default::default()
  });

  let address_struct = RustType::Struct(StructDef {
    name: StructToken::new("Address"),
    fields: vec![
      FieldDef::builder()
        .name(FieldNameToken::new("street"))
        .rust_type(TypeRef::new(RustPrimitive::String))
        .build(),
    ],
    kind: StructKind::Schema,
    ..Default::default()
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
    fields: vec![
      FieldDef::builder()
        .name(FieldNameToken::new("user"))
        .rust_type(TypeRef::new(RustPrimitive::Custom("User".into())))
        .build(),
    ],
    kind: StructKind::Schema,
    ..Default::default()
  });

  let user_struct = RustType::Struct(StructDef {
    name: StructToken::new("User"),
    fields: vec![
      FieldDef::builder()
        .name(FieldNameToken::new("name"))
        .rust_type(TypeRef::new(RustPrimitive::String))
        .build(),
    ],
    kind: StructKind::Schema,
    ..Default::default()
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
    fields: vec![
      FieldDef::builder()
        .name(FieldNameToken::new("user"))
        .rust_type(TypeRef::new(RustPrimitive::Custom("User".into())))
        .build(),
    ],
    kind: StructKind::Schema,
    ..Default::default()
  });

  let user_struct = RustType::Struct(StructDef {
    name: StructToken::new("User"),
    fields: vec![
      FieldDef::builder()
        .name(FieldNameToken::new("name"))
        .rust_type(TypeRef::new(RustPrimitive::String))
        .build(),
    ],
    kind: StructKind::Schema,
    ..Default::default()
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
    fields: vec![
      FieldDef::builder()
        .name(FieldNameToken::new("user"))
        .rust_type(TypeRef::new(RustPrimitive::Custom("User".into())))
        .build(),
    ],
    kind: StructKind::Schema,
    ..Default::default()
  });

  let response_struct = RustType::Struct(StructDef {
    name: StructToken::new("UserResponse"),
    fields: vec![
      FieldDef::builder()
        .name(FieldNameToken::new("user"))
        .rust_type(TypeRef::new(RustPrimitive::Custom("User".into())))
        .build(),
    ],
    kind: StructKind::Schema,
    ..Default::default()
  });

  let user_struct = RustType::Struct(StructDef {
    name: StructToken::new("User"),
    fields: vec![
      FieldDef::builder()
        .name(FieldNameToken::new("name"))
        .rust_type(TypeRef::new(RustPrimitive::String))
        .build(),
    ],
    kind: StructKind::Schema,
    ..Default::default()
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
    fields: vec![
      FieldDef::builder()
        .name(FieldNameToken::new("b"))
        .rust_type(TypeRef::new(RustPrimitive::Custom("B".into())))
        .build(),
    ],
    kind: StructKind::Schema,
    ..Default::default()
  });

  let b_struct = RustType::Struct(StructDef {
    name: StructToken::new("B"),
    fields: vec![
      FieldDef::builder()
        .name(FieldNameToken::new("c"))
        .rust_type(TypeRef::new(RustPrimitive::Custom("C".into())))
        .build(),
    ],
    serde_attrs: vec![],
    outer_attrs: vec![],
    methods: vec![],
    kind: StructKind::Schema,
    ..Default::default()
  });

  let c_struct = RustType::Struct(StructDef {
    name: StructToken::new("C"),
    fields: vec![
      FieldDef::builder()
        .name(FieldNameToken::new("value"))
        .rust_type(TypeRef::new(RustPrimitive::String))
        .build(),
    ],
    serde_attrs: vec![],
    outer_attrs: vec![],
    methods: vec![],
    kind: StructKind::Schema,
    ..Default::default()
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
    variants: vec![
      VariantDef::builder()
        .name(EnumVariantToken::new("Success"))
        .content(VariantContent::Tuple(vec![TypeRef::new(RustPrimitive::Custom(
          "User".into(),
        ))]))
        .build(),
    ],
    ..Default::default()
  });

  let user_struct = RustType::Struct(StructDef {
    name: StructToken::new("User"),
    fields: vec![
      FieldDef::builder()
        .name(FieldNameToken::new("name"))
        .rust_type(TypeRef::new(RustPrimitive::String))
        .build(),
    ],
    kind: StructKind::Schema,
    ..Default::default()
  });

  let types = vec![enum_def, user_struct];
  let usage_map = build_type_usage_map(seeds(&[("Result", (false, true))]), &types);

  assert_eq!(usage_map.get(&EnumToken::new("Result")), Some(&TypeUsage::ResponseOnly));
  assert_eq!(usage_map.get(&EnumToken::new("User")), Some(&TypeUsage::ResponseOnly));
}

#[test]
fn test_type_alias_dependency() {
  let alias = RustType::TypeAlias(TypeAliasDef {
    name: TypeAliasToken::new("UserId"),
    target: TypeRef::new(RustPrimitive::Custom("User".into())),
    ..Default::default()
  });

  let user_struct = RustType::Struct(StructDef {
    name: StructToken::new("User"),
    fields: vec![
      FieldDef::builder()
        .name(FieldNameToken::new("address"))
        .rust_type(TypeRef::new(RustPrimitive::Custom("Address".into())))
        .build(),
    ],
    kind: StructKind::Schema,
    ..Default::default()
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
    fields: vec![
      FieldDef::builder()
        .name(FieldNameToken::new("address"))
        .rust_type(TypeRef::new(RustPrimitive::Custom("Address".into())))
        .build(),
    ],
    kind: StructKind::Schema,
    ..Default::default()
  });

  let address_struct = RustType::Struct(StructDef {
    name: StructToken::new("Address"),
    kind: StructKind::Schema,
    ..Default::default()
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
    fields: vec![
      FieldDef::builder()
        .name(FieldNameToken::new("b"))
        .rust_type(TypeRef::new(RustPrimitive::Custom("B".into())).with_boxed())
        .build(),
    ],
    kind: StructKind::Schema,
    ..Default::default()
  });

  let b_struct = RustType::Struct(StructDef {
    name: StructToken::new("B"),
    fields: vec![
      FieldDef::builder()
        .name(FieldNameToken::new("a"))
        .rust_type(TypeRef::new(RustPrimitive::Custom("A".into())).with_boxed())
        .build(),
    ],
    kind: StructKind::Schema,
    ..Default::default()
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
    fields: vec![
      FieldDef::builder()
        .name(FieldNameToken::new("name"))
        .rust_type(TypeRef::new(RustPrimitive::String))
        .build(),
    ],
    kind: StructKind::OperationRequest,
    ..Default::default()
  });

  let user_struct = RustType::Struct(StructDef {
    name: StructToken::new("User"),
    fields: vec![
      FieldDef::builder()
        .name(FieldNameToken::new("id"))
        .rust_type(TypeRef::new(RustPrimitive::String))
        .build(),
    ],
    kind: StructKind::Schema,
    ..Default::default()
  });

  let response_enum = RustType::ResponseEnum(ResponseEnumDef {
    name: EnumToken::new("CreateUserResponseEnum"),
    request_type: Some(StructToken::new("CreateUserRequestParams")),
    variants: vec![ResponseVariant {
      status_code: StatusCodeToken::Ok200,
      variant_name: EnumVariantToken::new("Ok"),
      description: None,
      media_types: vec![ResponseMediaType::with_schema(
        "application/json",
        Some(TypeRef::new(RustPrimitive::Custom("User".into()))),
      )],
      schema_type: Some(TypeRef::new(RustPrimitive::Custom("User".into()))),
    }],
    ..Default::default()
  });

  let types = vec![request_struct, user_struct, response_enum];
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
    kind: StructKind::Schema,
    ..Default::default()
  });

  let response_b = RustType::Struct(StructDef {
    name: StructToken::new("ResponseB"),
    kind: StructKind::Schema,
    ..Default::default()
  });

  let request_struct = RustType::Struct(StructDef {
    name: StructToken::new("RequestParams"),
    kind: StructKind::OperationRequest,
    ..Default::default()
  });

  let response_enum = RustType::ResponseEnum(ResponseEnumDef {
    name: EnumToken::new("MyResponseEnum"),
    request_type: Some(StructToken::new("RequestParams")),
    variants: vec![
      ResponseVariant {
        status_code: StatusCodeToken::Ok200,
        variant_name: EnumVariantToken::new("Ok"),
        description: None,
        media_types: vec![ResponseMediaType::with_schema(
          "application/json",
          Some(TypeRef::new(RustPrimitive::Custom("ResponseA".into()))),
        )],
        schema_type: Some(TypeRef::new(RustPrimitive::Custom("ResponseA".into()))),
      },
      ResponseVariant {
        status_code: StatusCodeToken::BadRequest400,
        variant_name: EnumVariantToken::new("BadRequest"),
        description: None,
        media_types: vec![ResponseMediaType::with_schema(
          "application/json",
          Some(TypeRef::new(RustPrimitive::Custom("ResponseB".into()))),
        )],
        schema_type: Some(TypeRef::new(RustPrimitive::Custom("ResponseB".into()))),
      },
    ],
    ..Default::default()
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
    fields: vec![
      FieldDef::builder()
        .name(FieldNameToken::new("model"))
        .rust_type(TypeRef::new(RustPrimitive::Custom("ModelIds".into())))
        .build(),
    ],
    kind: StructKind::Schema,
    ..Default::default()
  });

  let model_enum = RustType::Enum(EnumDef {
    name: EnumToken::new("ModelIds"),
    variants: vec![
      VariantDef::builder()
        .name(EnumVariantToken::new("Gpt4"))
        .content(VariantContent::Unit)
        .build(),
    ],
    ..Default::default()
  });

  let request_body_alias = RustType::TypeAlias(TypeAliasDef {
    name: TypeAliasToken::new("CreateChatCompletionRequestBody"),
    target: TypeRef::new(RustPrimitive::Custom("CreateChatCompletionRequest".into())),
    ..Default::default()
  });

  let request_params = RustType::Struct(StructDef {
    name: StructToken::new("CreateChatCompletionRequestParams"),
    fields: vec![
      FieldDef::builder()
        .name(FieldNameToken::new("body"))
        .rust_type(TypeRef::new(RustPrimitive::Custom(
          "CreateChatCompletionRequestBody".into(),
        )))
        .build(),
    ],
    kind: StructKind::OperationRequest,
    ..Default::default()
  });

  let response_struct = RustType::Struct(StructDef {
    name: StructToken::new("CreateChatCompletionResponse"),
    kind: StructKind::Schema,
    ..Default::default()
  });

  let response_enum = RustType::ResponseEnum(ResponseEnumDef {
    name: EnumToken::new("CreateChatCompletionResponseEnum"),
    request_type: Some(StructToken::new("CreateChatCompletionRequestParams")),
    variants: vec![ResponseVariant {
      status_code: StatusCodeToken::Ok200,
      variant_name: EnumVariantToken::new("Ok"),
      description: None,
      media_types: vec![ResponseMediaType::with_schema(
        "application/json",
        Some(TypeRef::new(RustPrimitive::Custom(
          "CreateChatCompletionResponse".into(),
        ))),
      )],
      schema_type: Some(TypeRef::new(RustPrimitive::Custom(
        "CreateChatCompletionResponse".into(),
      ))),
    }],
    ..Default::default()
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
    kind: StructKind::OperationRequest,
    ..Default::default()
  });

  let response_a = RustType::Struct(StructDef {
    name: StructToken::new("ResponseA"),
    kind: StructKind::Schema,
    ..Default::default()
  });

  let response_b = RustType::Struct(StructDef {
    name: StructToken::new("ResponseB"),
    kind: StructKind::Schema,
    ..Default::default()
  });

  let response_enum = RustType::ResponseEnum(ResponseEnumDef {
    name: EnumToken::new("MyResponseEnum"),
    request_type: Some(StructToken::new("RequestParams")),
    variants: vec![
      ResponseVariant {
        status_code: StatusCodeToken::Ok200,
        variant_name: EnumVariantToken::new("Ok"),
        description: None,
        media_types: vec![ResponseMediaType::with_schema(
          "application/json",
          Some(TypeRef::new(RustPrimitive::Custom("ResponseA".into()))),
        )],
        schema_type: Some(TypeRef::new(RustPrimitive::Custom("ResponseA".into()))),
      },
      ResponseVariant {
        status_code: StatusCodeToken::BadRequest400,
        variant_name: EnumVariantToken::new("BadRequest"),
        description: None,
        media_types: vec![ResponseMediaType::with_schema(
          "application/json",
          Some(TypeRef::new(RustPrimitive::Custom("ResponseB".into()))),
        )],
        schema_type: Some(TypeRef::new(RustPrimitive::Custom("ResponseB".into()))),
      },
    ],
    ..Default::default()
  });

  let types = vec![request_struct, response_a, response_b, response_enum];
  let dep_graph = DependencyGraph::build(&types);

  let response_enum_deps = dep_graph.dependencies_of("MyResponseEnum");
  assert!(response_enum_deps.is_some(), "ResponseEnum should have dependencies");

  let deps = response_enum_deps.unwrap();
  assert!(deps.contains("ResponseA"), "Should depend on ResponseA variant");
  assert!(deps.contains("ResponseB"), "Should depend on ResponseB variant");
  assert!(
    !deps.contains("RequestParams"),
    "Should NOT depend on request_type field - this was the bug!"
  );
}

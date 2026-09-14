use std::collections::BTreeMap;

use super::build_type_usage_map;
use crate::generator::{
  ast::{
    EnumDef, EnumToken, EnumVariantToken, FieldDef, ResponseEnumDef, ResponseEnumVariant, ResponsePayload,
    ResponseUnionDef, ResponseUnionVariant, RustPrimitive, RustType, StatusCodeToken, StructDef, StructKind,
    StructToken, TypeAliasDef, TypeAliasToken, TypeRef, VariantContent, VariantDef, tokens::FieldNameToken,
  },
  postprocess::serde_usage::TypeUsage,
};

fn payload_union(name: &str, members: &[&str]) -> RustType {
  RustType::ResponseUnion(
    ResponseUnionDef::builder()
      .name(EnumToken::new(name))
      .variants(
        members
          .iter()
          .map(|member| {
            ResponseUnionVariant::builder()
              .name(EnumVariantToken::new(*member))
              .rust_type(TypeRef::new(RustPrimitive::Custom((*member).into())))
              .build()
          })
          .collect(),
      )
      .build(),
  )
}

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
fn test_payload_union_propagates_to_member_types_only() {
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

  let types = vec![
    request_struct,
    user_struct,
    payload_union("UserOrError", &["User", "Error"]),
  ];
  let seed = seeds(&[
    ("CreateUserRequestParams", (true, false)),
    ("UserOrError", (false, true)),
  ]);
  let usage_map = build_type_usage_map(seed, &types);

  assert_eq!(
    usage_map.get(&EnumToken::new("CreateUserRequestParams")),
    Some(&TypeUsage::RequestOnly)
  );
  assert_eq!(usage_map.get(&EnumToken::new("User")), Some(&TypeUsage::ResponseOnly));
  assert_eq!(
    usage_map.get(&EnumToken::new("UserOrError")),
    Some(&TypeUsage::ResponseOnly)
  );
}

#[test]
fn test_response_response_enum_propagates_nothing() {
  let response_a = RustType::Struct(StructDef {
    name: StructToken::new("ResponseA"),
    kind: StructKind::Schema,
    ..Default::default()
  });

  let request_struct = RustType::Struct(StructDef {
    name: StructToken::new("RequestParams"),
    kind: StructKind::OperationRequest,
    ..Default::default()
  });

  let response_enum = RustType::ResponseEnum(
    ResponseEnumDef::builder()
      .name(EnumToken::new("ApiResponse"))
      .variants(vec![
        ResponseEnumVariant::builder()
          .name(EnumVariantToken::new("Ok"))
          .status_code(StatusCodeToken::Ok200)
          .payload(ResponsePayload::Value)
          .build(),
      ])
      .build(),
  );

  let types = vec![response_a, request_struct, response_enum];
  let seed = seeds(&[("RequestParams", (true, false)), ("ApiResponse", (false, true))]);
  let usage_map = build_type_usage_map(seed, &types);

  assert_eq!(
    usage_map.get(&EnumToken::new("RequestParams")),
    Some(&TypeUsage::RequestOnly)
  );
  assert_eq!(
    usage_map.get(&EnumToken::new("ResponseA")),
    Some(&TypeUsage::Bidirectional),
    "the response enum references no body type, so ResponseA stays an orphan"
  );
  assert_eq!(
    usage_map.get(&EnumToken::new("ApiResponse")),
    Some(&TypeUsage::ResponseOnly)
  );
}

#[test]
fn test_request_body_chain_with_payload_union() {
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

  let response_union = payload_union("CreateChatCompletionResponseOrError", &["CreateChatCompletionResponse"]);

  let types = vec![
    request_body_struct,
    model_enum,
    request_body_alias,
    request_params,
    response_struct,
    response_union,
  ];

  let seed = seeds(&[
    ("CreateChatCompletionRequest", (true, false)),
    ("CreateChatCompletionRequestBody", (true, false)),
    ("CreateChatCompletionRequestParams", (true, false)),
    ("CreateChatCompletionResponseOrError", (false, true)),
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
    "Request params should remain request-only"
  );
  assert_eq!(
    usage_map.get(&EnumToken::new("CreateChatCompletionResponse")),
    Some(&TypeUsage::ResponseOnly),
    "Response should be response-only"
  );
  assert_eq!(
    usage_map.get(&EnumToken::new("CreateChatCompletionResponseOrError")),
    Some(&TypeUsage::ResponseOnly),
    "Payload union should be response-only"
  );
}

#[test]
fn test_payload_union_does_not_reach_request_type() {
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

  let types = vec![
    request_struct,
    response_a,
    response_b,
    payload_union("ResponseAOrResponseB", &["ResponseA", "ResponseB"]),
  ];
  let seed = seeds(&[
    ("RequestParams", (true, false)),
    ("ResponseAOrResponseB", (false, true)),
  ]);
  let usage_map = build_type_usage_map(seed, &types);

  assert_eq!(
    usage_map.get(&EnumToken::new("ResponseA")),
    Some(&TypeUsage::ResponseOnly),
    "ResponseA should be response-only (propagated from the union)"
  );
  assert_eq!(
    usage_map.get(&EnumToken::new("ResponseB")),
    Some(&TypeUsage::ResponseOnly),
    "ResponseB should be response-only (propagated from the union)"
  );
  assert_eq!(
    usage_map.get(&EnumToken::new("RequestParams")),
    Some(&TypeUsage::RequestOnly),
    "RequestParams should stay request-only"
  );
}

#[test]
fn test_propagation_through_composite_map_type() {
  let result_struct = RustType::Struct(StructDef {
    name: StructToken::new("ResultBody"),
    fields: vec![
      FieldDef::builder()
        .name(FieldNameToken::new("category_applied_input_types"))
        .rust_type(TypeRef::new(RustPrimitive::Custom(
          "indexmap::IndexMap<String, Vec<InputType>>".into(),
        )))
        .build(),
    ],
    kind: StructKind::Schema,
    ..Default::default()
  });

  let input_type_enum = RustType::Enum(EnumDef {
    name: EnumToken::new("InputType"),
    variants: vec![
      VariantDef::builder()
        .name(EnumVariantToken::new("Text"))
        .content(VariantContent::Unit)
        .build(),
    ],
    ..Default::default()
  });

  let types = vec![result_struct, input_type_enum];
  let usage_map = build_type_usage_map(seeds(&[("ResultBody", (false, true))]), &types);

  assert_eq!(
    usage_map.get(&EnumToken::new("ResultBody")),
    Some(&TypeUsage::ResponseOnly)
  );
  assert_eq!(
    usage_map.get(&EnumToken::new("InputType")),
    Some(&TypeUsage::ResponseOnly),
    "nested type inside a composite map must inherit its parent's direction"
  );
}

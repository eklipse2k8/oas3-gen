use std::collections::BTreeMap;

use crate::generator::{
  analyzer::{TypeUsage, build_type_usage_map},
  ast::{
    EnumDef, FieldDef, RustPrimitive, RustType, StructDef, StructKind, TypeAliasDef, TypeRef, VariantContent,
    VariantDef,
  },
};

fn seeds(entries: &[(&str, (bool, bool))]) -> BTreeMap<String, (bool, bool)> {
  entries
    .iter()
    .map(|(name, flags)| ((*name).to_string(), *flags))
    .collect()
}

#[test]
fn test_dependency_graph_simple_struct() {
  let user_struct = RustType::Struct(StructDef {
    name: "User".to_string(),
    docs: vec![],
    fields: vec![FieldDef {
      name: "address".to_string(),
      rust_type: TypeRef::new(RustPrimitive::Custom("Address".to_string())),
      ..Default::default()
    }],
    derives: vec!["Debug".to_string(), "Clone".to_string()],
    serde_attrs: vec![],
    outer_attrs: vec![],
    methods: vec![],
    kind: StructKind::Schema,
  });

  let address_struct = RustType::Struct(StructDef {
    name: "Address".to_string(),
    docs: vec![],
    fields: vec![FieldDef {
      name: "street".to_string(),
      rust_type: TypeRef::new(RustPrimitive::String),
      ..Default::default()
    }],
    derives: vec!["Debug".to_string(), "Clone".to_string()],
    serde_attrs: vec![],
    outer_attrs: vec![],
    methods: vec![],
    kind: StructKind::Schema,
  });

  let types = vec![user_struct, address_struct];

  let usage_map = build_type_usage_map(seeds(&[]), &types);

  assert_eq!(usage_map.len(), 2);
  assert_eq!(usage_map.get("User"), Some(&TypeUsage::Bidirectional));
  assert_eq!(usage_map.get("Address"), Some(&TypeUsage::Bidirectional));
}

#[test]
fn test_propagation_request_to_nested() {
  let request_struct = RustType::Struct(StructDef {
    name: "CreateUserRequest".to_string(),
    docs: vec![],
    fields: vec![FieldDef {
      name: "user".to_string(),
      rust_type: TypeRef::new(RustPrimitive::Custom("User".to_string())),
      ..Default::default()
    }],
    derives: vec!["Debug".to_string(), "Clone".to_string()],
    serde_attrs: vec![],
    outer_attrs: vec![],
    methods: vec![],
    kind: StructKind::RequestBody,
  });

  let user_struct = RustType::Struct(StructDef {
    name: "User".to_string(),
    docs: vec![],
    fields: vec![FieldDef {
      name: "name".to_string(),
      rust_type: TypeRef::new(RustPrimitive::String),
      ..Default::default()
    }],
    derives: vec!["Debug".to_string(), "Clone".to_string()],
    serde_attrs: vec![],
    outer_attrs: vec![],
    methods: vec![],
    kind: StructKind::Schema,
  });

  let types = vec![request_struct, user_struct];
  let usage_map = build_type_usage_map(seeds(&[("CreateUserRequest", (true, false))]), &types);

  assert_eq!(usage_map.get("CreateUserRequest"), Some(&TypeUsage::RequestOnly));
  assert_eq!(usage_map.get("User"), Some(&TypeUsage::RequestOnly));
}

#[test]
fn test_propagation_response_to_nested() {
  let response_struct = RustType::Struct(StructDef {
    name: "UserResponse".to_string(),
    docs: vec![],
    fields: vec![FieldDef {
      name: "user".to_string(),
      rust_type: TypeRef::new(RustPrimitive::Custom("User".to_string())),
      ..Default::default()
    }],
    derives: vec!["Debug".to_string(), "Clone".to_string()],
    serde_attrs: vec![],
    outer_attrs: vec![],
    methods: vec![],
    kind: StructKind::Schema,
  });

  let user_struct = RustType::Struct(StructDef {
    name: "User".to_string(),
    docs: vec![],
    fields: vec![FieldDef {
      name: "name".to_string(),
      rust_type: TypeRef::new(RustPrimitive::String),
      ..Default::default()
    }],
    derives: vec!["Debug".to_string(), "Clone".to_string()],
    serde_attrs: vec![],
    outer_attrs: vec![],
    methods: vec![],
    kind: StructKind::Schema,
  });

  let types = vec![response_struct, user_struct];
  let usage_map = build_type_usage_map(seeds(&[("UserResponse", (false, true))]), &types);

  assert_eq!(usage_map.get("UserResponse"), Some(&TypeUsage::ResponseOnly));
  assert_eq!(usage_map.get("User"), Some(&TypeUsage::ResponseOnly));
}

#[test]
fn test_propagation_bidirectional() {
  let request_struct = RustType::Struct(StructDef {
    name: "UpdateUserRequest".to_string(),
    docs: vec![],
    fields: vec![FieldDef {
      name: "user".to_string(),
      rust_type: TypeRef::new(RustPrimitive::Custom("User".to_string())),
      ..Default::default()
    }],
    derives: vec!["Debug".to_string(), "Clone".to_string()],
    serde_attrs: vec![],
    outer_attrs: vec![],
    methods: vec![],
    kind: StructKind::RequestBody,
  });

  let response_struct = RustType::Struct(StructDef {
    name: "UserResponse".to_string(),
    docs: vec![],
    fields: vec![FieldDef {
      name: "user".to_string(),
      rust_type: TypeRef::new(RustPrimitive::Custom("User".to_string())),
      ..Default::default()
    }],
    derives: vec!["Debug".to_string(), "Clone".to_string()],
    serde_attrs: vec![],
    outer_attrs: vec![],
    methods: vec![],
    kind: StructKind::Schema,
  });

  let user_struct = RustType::Struct(StructDef {
    name: "User".to_string(),
    docs: vec![],
    fields: vec![FieldDef {
      name: "name".to_string(),
      rust_type: TypeRef::new(RustPrimitive::String),
      ..Default::default()
    }],
    derives: vec!["Debug".to_string(), "Clone".to_string()],
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

  assert_eq!(usage_map.get("UpdateUserRequest"), Some(&TypeUsage::RequestOnly));
  assert_eq!(usage_map.get("UserResponse"), Some(&TypeUsage::ResponseOnly));
  assert_eq!(usage_map.get("User"), Some(&TypeUsage::Bidirectional));
}

#[test]
fn test_transitive_dependency_chain() {
  let a_struct = RustType::Struct(StructDef {
    name: "A".to_string(),
    docs: vec![],
    fields: vec![FieldDef {
      name: "b".to_string(),
      rust_type: TypeRef::new(RustPrimitive::Custom("B".to_string())),
      ..Default::default()
    }],
    derives: vec!["Debug".to_string()],
    serde_attrs: vec![],
    outer_attrs: vec![],
    methods: vec![],
    kind: StructKind::Schema,
  });

  let b_struct = RustType::Struct(StructDef {
    name: "B".to_string(),
    docs: vec![],
    fields: vec![FieldDef {
      name: "c".to_string(),
      rust_type: TypeRef::new(RustPrimitive::Custom("C".to_string())),
      ..Default::default()
    }],
    derives: vec!["Debug".to_string()],
    serde_attrs: vec![],
    outer_attrs: vec![],
    methods: vec![],
    kind: StructKind::Schema,
  });

  let c_struct = RustType::Struct(StructDef {
    name: "C".to_string(),
    docs: vec![],
    fields: vec![FieldDef {
      name: "value".to_string(),
      rust_type: TypeRef::new(RustPrimitive::String),
      ..Default::default()
    }],
    derives: vec!["Debug".to_string()],
    serde_attrs: vec![],
    outer_attrs: vec![],
    methods: vec![],
    kind: StructKind::Schema,
  });

  let types = vec![a_struct, b_struct, c_struct];
  let usage_map = build_type_usage_map(seeds(&[("A", (false, true))]), &types);

  assert_eq!(usage_map.get("A"), Some(&TypeUsage::ResponseOnly));
  assert_eq!(usage_map.get("B"), Some(&TypeUsage::ResponseOnly));
  assert_eq!(usage_map.get("C"), Some(&TypeUsage::ResponseOnly));
}

#[test]
fn test_enum_with_tuple_variant() {
  let enum_def = RustType::Enum(EnumDef {
    name: "Result".to_string(),
    docs: vec![],
    variants: vec![VariantDef {
      name: "Success".to_string(),
      docs: vec![],
      content: VariantContent::Tuple(vec![TypeRef::new(RustPrimitive::Custom("User".to_string()))]),
      serde_attrs: vec![],
      deprecated: false,
    }],
    discriminator: None,
    derives: vec!["Debug".to_string()],
    serde_attrs: vec![],
    outer_attrs: vec![],
  });

  let user_struct = RustType::Struct(StructDef {
    name: "User".to_string(),
    docs: vec![],
    fields: vec![FieldDef {
      name: "name".to_string(),
      rust_type: TypeRef::new(RustPrimitive::String),
      ..Default::default()
    }],
    derives: vec!["Debug".to_string()],
    serde_attrs: vec![],
    outer_attrs: vec![],
    methods: vec![],
    kind: StructKind::Schema,
  });

  let types = vec![enum_def, user_struct];
  let usage_map = build_type_usage_map(seeds(&[("Result", (false, true))]), &types);

  assert_eq!(usage_map.get("Result"), Some(&TypeUsage::ResponseOnly));
  assert_eq!(usage_map.get("User"), Some(&TypeUsage::ResponseOnly));
}

#[test]
fn test_type_alias_dependency() {
  let alias = RustType::TypeAlias(TypeAliasDef {
    name: "UserId".to_string(),
    docs: vec![],
    target: TypeRef::new(RustPrimitive::Custom("User".to_string())),
  });

  let user_struct = RustType::Struct(StructDef {
    name: "User".to_string(),
    docs: vec![],
    fields: vec![FieldDef {
      name: "id".to_string(),
      rust_type: TypeRef::new(RustPrimitive::I64),
      ..Default::default()
    }],
    derives: vec!["Debug".to_string()],
    serde_attrs: vec![],
    outer_attrs: vec![],
    methods: vec![],
    kind: StructKind::Schema,
  });

  let types = vec![alias, user_struct];
  let usage_map = build_type_usage_map(seeds(&[("UserId", (false, true))]), &types);

  assert_eq!(usage_map.get("UserId"), Some(&TypeUsage::ResponseOnly));
  assert_eq!(usage_map.get("User"), Some(&TypeUsage::ResponseOnly));
}

#[test]
fn test_no_propagation_without_operations() {
  let user_struct = RustType::Struct(StructDef {
    name: "User".to_string(),
    docs: vec![],
    fields: vec![FieldDef {
      name: "address".to_string(),
      rust_type: TypeRef::new(RustPrimitive::Custom("Address".to_string())),
      ..Default::default()
    }],
    derives: vec!["Debug".to_string()],
    serde_attrs: vec![],
    outer_attrs: vec![],
    methods: vec![],
    kind: StructKind::Schema,
  });

  let address_struct = RustType::Struct(StructDef {
    name: "Address".to_string(),
    docs: vec![],
    fields: vec![],
    derives: vec!["Debug".to_string()],
    serde_attrs: vec![],
    outer_attrs: vec![],
    methods: vec![],
    kind: StructKind::Schema,
  });

  let types = vec![user_struct, address_struct];
  let usage_map = build_type_usage_map(seeds(&[]), &types);

  assert_eq!(usage_map.len(), 2);
  assert_eq!(usage_map.get("User"), Some(&TypeUsage::Bidirectional));
  assert_eq!(usage_map.get("Address"), Some(&TypeUsage::Bidirectional));
}

#[test]
fn test_cyclic_dependency_handling() {
  let a_struct = RustType::Struct(StructDef {
    name: "A".to_string(),
    docs: vec![],
    fields: vec![FieldDef {
      name: "b".to_string(),
      rust_type: TypeRef::new(RustPrimitive::Custom("B".to_string())).with_boxed(),
      ..Default::default()
    }],
    derives: vec!["Debug".to_string()],
    serde_attrs: vec![],
    outer_attrs: vec![],
    methods: vec![],
    kind: StructKind::Schema,
  });

  let b_struct = RustType::Struct(StructDef {
    name: "B".to_string(),
    docs: vec![],
    fields: vec![FieldDef {
      name: "a".to_string(),
      rust_type: TypeRef::new(RustPrimitive::Custom("A".to_string())).with_boxed(),
      ..Default::default()
    }],
    derives: vec!["Debug".to_string()],
    serde_attrs: vec![],
    outer_attrs: vec![],
    methods: vec![],
    kind: StructKind::Schema,
  });

  let types = vec![a_struct, b_struct];
  let usage_map = build_type_usage_map(seeds(&[("A", (false, true))]), &types);

  assert_eq!(usage_map.get("A"), Some(&TypeUsage::ResponseOnly));
  assert_eq!(usage_map.get("B"), Some(&TypeUsage::ResponseOnly));
}

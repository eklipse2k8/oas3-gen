use crate::generator::{
  ast::{RustPrimitive, TypeAliasDef, TypeAliasToken, TypeRef},
  codegen::{Visibility, type_aliases::generate_type_alias},
};

fn format_tokens(tokens: proc_macro2::TokenStream) -> String {
  prettyplease::unparse(&syn::parse2(tokens).unwrap())
}

#[test]
fn test_basic_type_aliases() {
  let cases = [
    (
      TypeAliasDef {
        name: TypeAliasToken::new("Identifier"),
        docs: vec![],
        target: TypeRef::new(RustPrimitive::String),
      },
      "type Identifier = String;",
    ),
    (
      TypeAliasDef {
        name: TypeAliasToken::new("Count"),
        docs: vec![],
        target: TypeRef::new(RustPrimitive::I64),
      },
      "type Count = i64;",
    ),
    (
      TypeAliasDef {
        name: TypeAliasToken::new("Timestamp"),
        docs: vec![],
        target: TypeRef::new(RustPrimitive::DateTime),
      },
      "type Timestamp = chrono::DateTime<chrono::Utc>;",
    ),
  ];

  for (def, expected_suffix) in cases {
    let name = def.name.clone();
    let tokens = generate_type_alias(&def, Visibility::Public);
    let code = format_tokens(tokens);
    assert!(code.contains("pub"), "missing pub visibility for {name}");
    assert!(
      code.contains(expected_suffix),
      "expected '{expected_suffix}' in output for {name}:\n{code}"
    );
  }
}

#[test]
fn test_type_alias_with_docs() {
  let def = TypeAliasDef {
    name: TypeAliasToken::new("UserId"),
    docs: vec![
      "Unique identifier for a user.".to_string(),
      "Format: UUID string.".to_string(),
    ],
    target: TypeRef::new(RustPrimitive::String),
  };

  let tokens = generate_type_alias(&def, Visibility::Public);
  let code = format_tokens(tokens);

  assert!(
    code.contains("Unique identifier for a user."),
    "missing first doc line:\n{code}"
  );
  assert!(
    code.contains("Format: UUID string."),
    "missing second doc line:\n{code}"
  );
  assert!(
    code.contains("pub type UserId = String;"),
    "missing type alias declaration:\n{code}"
  );
}

#[test]
fn test_type_alias_visibility_levels() {
  let def = TypeAliasDef {
    name: TypeAliasToken::new("TestAlias"),
    docs: vec![],
    target: TypeRef::new(RustPrimitive::Bool),
  };

  let cases = [
    (Visibility::Public, "pub type TestAlias"),
    (Visibility::Crate, "pub(crate) type TestAlias"),
    (Visibility::File, "type TestAlias"),
  ];

  for (visibility, expected_prefix) in cases {
    let tokens = generate_type_alias(&def, visibility);
    let code = format_tokens(tokens);
    assert!(
      code.contains(expected_prefix),
      "expected '{expected_prefix}' for visibility {visibility:?}:\n{code}"
    );
  }
}

#[test]
fn test_type_alias_with_wrapper_types() {
  let cases = [
    (
      TypeRef::new(RustPrimitive::String).with_vec(),
      "type Strings = Vec<String>;",
    ),
    (
      TypeRef::new(RustPrimitive::I32).with_option(),
      "type OptionalInt = Option<i32>;",
    ),
    (
      TypeRef::new(RustPrimitive::Custom("Pet".into())).with_vec(),
      "type Pets = Vec<Pet>;",
    ),
    (
      TypeRef::new(RustPrimitive::Custom("LargeStruct".into())).with_boxed(),
      "type BoxedStruct = Box<LargeStruct>;",
    ),
    (
      TypeRef::new(RustPrimitive::String).with_vec().with_option(),
      "type OptionalStrings = Option<Vec<String>>;",
    ),
  ];

  let names = ["Strings", "OptionalInt", "Pets", "BoxedStruct", "OptionalStrings"];

  for ((target, expected_suffix), name) in cases.into_iter().zip(names) {
    let def = TypeAliasDef {
      name: TypeAliasToken::new(name),
      docs: vec![],
      target,
    };
    let tokens = generate_type_alias(&def, Visibility::Public);
    let code = format_tokens(tokens);
    assert!(
      code.contains(expected_suffix),
      "expected '{expected_suffix}' for {name}:\n{code}"
    );
  }
}

#[test]
fn test_type_alias_custom_types() {
  let def = TypeAliasDef {
    name: TypeAliasToken::new("PetList"),
    docs: vec!["List of pets from the API.".to_string()],
    target: TypeRef::new(RustPrimitive::Custom("Vec<Pet>".into())),
  };

  let tokens = generate_type_alias(&def, Visibility::Crate);
  let code = format_tokens(tokens);

  assert!(code.contains("pub(crate) type PetList = Vec<Pet>;"));
}

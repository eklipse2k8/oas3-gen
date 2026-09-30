use serde_json::json;

use crate::generator::{
  ast::{
    EnumDef, EnumDefault, EnumDefaultValue, EnumToken, EnumVariantToken, FieldDef, FieldNameToken, RustType, StructDef,
    StructToken, TypeRef, VariantContent, VariantDef,
  },
  naming::inference::NormalizedVariant,
  postprocess::defaults::FieldDefaultProcessor,
};

fn value_variant(name: &str, rename: &str, aliases: &[&str]) -> VariantDef {
  let mut variant = VariantDef::builder()
    .normalized(NormalizedVariant {
      name: name.to_string(),
      rename_value: rename.to_string(),
    })
    .content(VariantContent::Unit)
    .build();
  for alias in aliases {
    variant.add_alias(*alias);
  }
  variant
}

fn tuple_variant(name: &str, wrapped: TypeRef) -> VariantDef {
  VariantDef {
    name: EnumVariantToken::new(name),
    content: VariantContent::Tuple(vec![wrapped]),
    ..Default::default()
  }
}

fn enum_type(name: &str, variants: Vec<VariantDef>) -> RustType {
  RustType::Enum(EnumDef {
    name: EnumToken::new(name),
    variants,
    ..Default::default()
  })
}

#[test]
fn test_field_default_resolves_against_generated_enum() {
  let cases = [
    (
      TypeRef::new("Status").with_option(),
      json!("inactive"),
      Some(("Inactive", None)),
    ),
    (TypeRef::new("Status"), json!("disabled"), Some(("Inactive", None))),
    (TypeRef::new("Status").with_option(), json!("unknown"), None),
    (TypeRef::new("Status").with_option(), json!(null), None),
    (TypeRef::new("Status").with_vec(), json!("active"), None),
    (
      TypeRef::new("Model").with_option(),
      json!("model-1.0"),
      Some(("Enum", Some("Stable"))),
    ),
    (
      TypeRef::new("Model").with_option(),
      json!("Legacy"),
      Some(("Enum2", Some("Legacy"))),
    ),
    (TypeRef::new("Mixed").with_option(), json!(5), Some(("Integer", None))),
    (TypeRef::new("Mixed").with_option(), json!("5"), Some(("Text", None))),
    (TypeRef::new("Shape").with_option(), json!({ "side": 1 }), None),
    (TypeRef::new("String").with_option(), json!("inactive"), None),
  ];

  let fields = cases
    .iter()
    .map(|(rust_type, value, _)| {
      FieldDef::builder()
        .name(FieldNameToken::new("field"))
        .rust_type(rust_type.clone())
        .default_value(value.clone())
        .build()
    })
    .collect::<Vec<_>>();

  let mut types = vec![
    RustType::Struct(StructDef {
      name: StructToken::new("Request"),
      fields,
      ..Default::default()
    }),
    enum_type(
      "Status",
      vec![
        value_variant("Active", "active", &[]),
        value_variant("Inactive", "inactive", &["disabled"]),
      ],
    ),
    enum_type(
      "Model",
      vec![
        tuple_variant("Enum", TypeRef::new("ModelEnum")),
        tuple_variant("Enum2", TypeRef::new("ModelEnum2")),
      ],
    ),
    enum_type(
      "ModelEnum",
      vec![
        value_variant("Draft", "model-1.0-draft", &[]),
        value_variant("Stable", "model-1.0", &[]),
      ],
    ),
    enum_type("ModelEnum2", vec![value_variant("Legacy", "Legacy", &[])]),
    enum_type(
      "Mixed",
      vec![
        tuple_variant("Integer", TypeRef::new("i64")),
        tuple_variant("Text", TypeRef::new("String")),
      ],
    ),
  ];

  FieldDefaultProcessor::process(&mut types);

  let RustType::Struct(def) = &types[0] else {
    panic!("expected the request struct first");
  };
  for ((rust_type, value, expected), field) in cases.iter().zip(&def.fields) {
    let actual = field.default_variant.as_ref().map(|default| {
      (
        default.variant.as_str(),
        default.inner_variant.as_ref().map(EnumVariantToken::as_str),
      )
    });
    assert_eq!(actual, *expected, "default {value} on {}", rust_type.to_rust_type());
  }
}

#[test]
fn test_enum_without_default_derives_one_when_a_derived_default_needs_it() {
  let union_type = |name: &str, wrapped: &str, default_mode: EnumDefault| {
    RustType::Enum(EnumDef {
      name: EnumToken::new(name),
      variants: vec![
        tuple_variant("Wrapped", TypeRef::new(wrapped)),
        value_variant("Unit", "unit", &[]),
      ],
      default_mode,
      ..Default::default()
    })
  };
  let field = |rust_type: TypeRef| {
    FieldDef::builder()
      .name(FieldNameToken::new("field"))
      .rust_type(rust_type)
  };
  let explicit_default = EnumDefault::Value(EnumDefaultValue {
    value: json!("unit"),
    inner_variant: None,
  });

  let mut types = vec![
    RustType::Struct(StructDef {
      name: StructToken::new("Request"),
      fields: vec![
        field(TypeRef::new("Required")).build(),
        field(TypeRef::new("Optional").with_option()).build(),
        field(TypeRef::new("Listed").with_vec()).build(),
        field(TypeRef::new("Explicit")).default_value(json!("unit")).build(),
        field(TypeRef::new("Valued")).build(),
      ],
      ..Default::default()
    }),
    union_type("Required", "RequiredInner", EnumDefault::None),
    union_type("RequiredInner", "String", EnumDefault::None),
    union_type("Derived", "DerivedInner", EnumDefault::Derive),
    union_type("DerivedInner", "String", EnumDefault::None),
    union_type("Optional", "String", EnumDefault::None),
    union_type("Listed", "String", EnumDefault::None),
    union_type("Explicit", "String", EnumDefault::None),
    union_type("Valued", "String", explicit_default),
  ];

  FieldDefaultProcessor::process(&mut types);

  let expected = [
    ("Required", "derive"),
    ("RequiredInner", "derive"),
    ("Derived", "derive"),
    ("DerivedInner", "derive"),
    ("Optional", "none"),
    ("Listed", "none"),
    ("Explicit", "none"),
    ("Valued", "value"),
  ];
  let actual = types
    .iter()
    .filter_map(|rust_type| match rust_type {
      RustType::Enum(def) => Some((
        def.name.as_str(),
        match def.default_mode {
          EnumDefault::Derive => "derive",
          EnumDefault::None => "none",
          EnumDefault::Value(_) => "value",
        },
      )),
      _ => None,
    })
    .collect::<Vec<_>>();
  assert_eq!(actual, expected);
}

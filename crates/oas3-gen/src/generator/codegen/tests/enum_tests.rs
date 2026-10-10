use quote::ToTokens;
use serde_json::json;

use crate::generator::{
  ast::{
    DiscriminatedEnumDef, DiscriminatedVariant, Documentation, EnumDef, EnumDefault, EnumDefaultValue, EnumMethod,
    EnumMethodKind, EnumToken, EnumVariantToken, OuterAttr, ResponseEnumDef, ResponseEnumVariant, ResponsePayload,
    ResponseUnionDef, ResponseUnionVariant, RustPrimitive, SerdeAttribute, SerdeMode, StatusCodeToken, StructToken,
    TypeRef, VariantContent, VariantDef,
  },
  codegen::{
    Visibility,
    enums::{DiscriminatedEnumFragment, EnumFragment, ResponseEnumFragment, ResponseUnionFragment},
  },
  converter::GenerationTarget,
  naming::constants::OTHER_ENUM_VARIANT,
};

fn make_unit_variant(name: &str) -> VariantDef {
  VariantDef::builder()
    .name(EnumVariantToken::from(name))
    .content(VariantContent::Unit)
    .build()
}

fn make_simple_enum(name: &str, variants: Vec<VariantDef>) -> EnumDef {
  EnumDef {
    name: EnumToken::new(name),
    variants,
    serde_attrs: vec![],
    outer_attrs: vec![],
    case_insensitive: false,
    methods: vec![],
    generate_display: true,
    ..Default::default()
  }
}

#[test]
fn test_basic_enum_generation() {
  let def = make_simple_enum(
    "Color",
    vec![
      make_unit_variant("Red"),
      make_unit_variant("Green"),
      make_unit_variant("Blue"),
    ],
  );

  let code = EnumFragment::new(&def, Visibility::Public)
    .into_token_stream()
    .to_string();

  let assertions = [
    ("pub enum Color", "should have pub enum declaration"),
    ("Red", "should have Red variant"),
    ("Green", "should have Green variant"),
    ("Blue", "should have Blue variant"),
    ("# [default]", "first variant should have #[default] attribute"),
    ("Debug", "should derive Debug"),
    ("Clone", "should derive Clone"),
    ("Serialize", "should derive Serialize"),
    ("Deserialize", "should derive Deserialize"),
  ];
  for (expected, msg) in assertions {
    assert!(code.contains(expected), "{msg}");
  }
}

#[test]
fn test_simple_enum_display_impl() {
  let simple_def = make_simple_enum(
    "Color",
    vec![
      make_unit_variant("Red"),
      make_unit_variant("Green"),
      make_unit_variant("Blue"),
    ],
  );

  let code = EnumFragment::new(&simple_def, Visibility::Public)
    .into_token_stream()
    .to_string();

  assert!(
    code.contains("impl core :: fmt :: Display for Color"),
    "should have Display impl for simple enum"
  );
  assert!(
    code.contains("Self :: Red => write ! (f , \"Red\")"),
    "should output variant name for Red"
  );
  assert!(
    code.contains("Self :: Green => write ! (f , \"Green\")"),
    "should output variant name for Green"
  );
  assert!(
    code.contains("Self :: Blue => write ! (f , \"Blue\")"),
    "should output variant name for Blue"
  );
}

#[test]
fn test_enum_default_variant_placement() {
  let cases = [(None, "Red"), (Some(2), "Blue")];

  for (marked_idx, expected_variant) in cases {
    let variants = ["Red", "Green", "Blue"]
      .into_iter()
      .enumerate()
      .map(|(idx, name)| {
        let mut variant = make_unit_variant(name);
        variant.default = Some(idx) == marked_idx;
        variant
      })
      .collect::<Vec<_>>();
    let def = make_simple_enum("Color", variants);

    let code = EnumFragment::new(&def, Visibility::Public)
      .into_token_stream()
      .to_string();

    assert_eq!(
      code.matches("# [default]").count(),
      1,
      "exactly one #[default] expected when marked_idx is {marked_idx:?}"
    );
    assert!(
      code.contains(&format!("# [default] {expected_variant}")),
      "#[default] should precede {expected_variant} when marked_idx is {marked_idx:?}"
    );
  }
}

#[test]
fn test_enum_default_value_impl() {
  let tuple_variant = |name: &str, primitive: RustPrimitive, default: bool| VariantDef {
    name: EnumVariantToken::new(name),
    content: VariantContent::Tuple(vec![TypeRef::new(primitive)]),
    default,
    ..Default::default()
  };

  let cases = [
    (
      0,
      serde_json::json!("cheesecake"),
      "Self :: String (\"cheesecake\" . to_string ())",
    ),
    (1, serde_json::json!(3), "Self :: Integer (3i64)"),
  ];

  for (default_idx, value, expected_construction) in cases {
    let mut def = make_simple_enum(
      "Value",
      vec![
        tuple_variant("String", RustPrimitive::String, default_idx == 0),
        tuple_variant("Integer", RustPrimitive::I64, default_idx == 1),
      ],
    );
    def.default_mode = EnumDefault::Value(EnumDefaultValue {
      value,
      inner_variant: None,
    });

    let code = EnumFragment::new(&def, Visibility::Public)
      .into_token_stream()
      .to_string();

    assert!(
      code.contains("impl Default for Value"),
      "manual Default impl expected for {expected_construction}"
    );
    assert!(
      code.contains(expected_construction),
      "construction mismatch, expected {expected_construction} in {code}"
    );
    assert!(
      !code.contains("oas3_gen_support"),
      "derive Default should not be emitted for {expected_construction}"
    );
    assert!(
      !code.contains("# [default]"),
      "no #[default] marker expected for {expected_construction}"
    );
  }
}

#[test]
fn test_enum_default_value_impl_with_wrapped_enum() {
  let mut def = make_simple_enum(
    "Size",
    vec![
      VariantDef {
        name: EnumVariantToken::new("Custom"),
        content: VariantContent::Tuple(vec![TypeRef::new("Dimensions")]),
        ..Default::default()
      },
      VariantDef {
        name: EnumVariantToken::new("Preset"),
        content: VariantContent::Tuple(vec![TypeRef::new("SizePreset")]),
        default: true,
        ..Default::default()
      },
    ],
  );
  def.default_mode = EnumDefault::Value(EnumDefaultValue {
    value: serde_json::json!("auto_2K"),
    inner_variant: Some(EnumVariantToken::new("Auto2k")),
  });

  let code = EnumFragment::new(&def, Visibility::Public)
    .into_token_stream()
    .to_string();

  assert!(
    code.contains("Self :: Preset (SizePreset :: Auto2k)"),
    "wrapped enum construction expected, got {code}"
  );
  assert!(!code.contains("# [default]"), "no #[default] marker expected");
}

#[test]
fn test_enum_default_mode_none_emits_no_default() {
  let mut def = make_simple_enum("Value", vec![make_unit_variant("Red"), make_unit_variant("Blue")]);
  def.default_mode = EnumDefault::None;

  let code = EnumFragment::new(&def, Visibility::Public)
    .into_token_stream()
    .to_string();

  assert!(
    !code.contains("oas3_gen_support"),
    "derive Default should not be emitted: {code}"
  );
  assert!(!code.contains("# [default]"), "no #[default] marker expected: {code}");
  assert!(!code.contains("impl Default for"), "no Default impl expected: {code}");
}

#[test]
fn test_simple_enum_display_impl_with_serde_rename() {
  let renamed_def = EnumDef {
    name: EnumToken::new("Status"),
    variants: vec![
      VariantDef::builder()
        .name(EnumVariantToken::new("InProgress"))
        .content(VariantContent::Unit)
        .serde_attrs(vec![SerdeAttribute::Rename("in_progress".to_string())])
        .build(),
      VariantDef::builder()
        .name(EnumVariantToken::new("Completed"))
        .content(VariantContent::Unit)
        .serde_attrs(vec![SerdeAttribute::Rename("completed".to_string())])
        .build(),
    ],
    serde_attrs: vec![],
    outer_attrs: vec![],
    case_insensitive: false,
    methods: vec![],
    generate_display: true,
    ..Default::default()
  };

  let code = EnumFragment::new(&renamed_def, Visibility::Public)
    .into_token_stream()
    .to_string();

  assert!(
    code.contains("impl core :: fmt :: Display for Status"),
    "should have Display impl"
  );
  assert!(
    code.contains("Self :: InProgress => write ! (f , \"in_progress\")"),
    "should output serde rename value for InProgress"
  );
  assert!(
    code.contains("Self :: Completed => write ! (f , \"completed\")"),
    "should output serde rename value for Completed"
  );
}

#[test]
fn test_tuple_enum_no_display_impl() {
  let tuple_def = EnumDef {
    name: EnumToken::new("Value"),
    variants: vec![
      VariantDef::builder()
        .name(EnumVariantToken::new("Text"))
        .content(VariantContent::Tuple(vec![TypeRef::new(RustPrimitive::String)]))
        .build(),
      VariantDef::builder()
        .name(EnumVariantToken::new("Number"))
        .content(VariantContent::Tuple(vec![TypeRef::new(RustPrimitive::I64)]))
        .build(),
    ],
    serde_attrs: vec![],
    outer_attrs: vec![],
    case_insensitive: false,
    methods: vec![],
    ..Default::default()
  };

  let code = EnumFragment::new(&tuple_def, Visibility::Public)
    .into_token_stream()
    .to_string();

  assert!(
    !code.contains("impl core :: fmt :: Display for Value"),
    "tuple enum should NOT have Display impl"
  );
}

#[test]
fn test_enum_with_docs() {
  let def = EnumDef {
    name: EnumToken::new("Status"),
    docs: Documentation::from_lines(["Represents the status of an item.", "Can be active or inactive."]),
    variants: vec![make_unit_variant("Active"), make_unit_variant("Inactive")],
    serde_attrs: vec![],
    outer_attrs: vec![],
    case_insensitive: false,
    methods: vec![],
    ..Default::default()
  };

  let code = EnumFragment::new(&def, Visibility::Public)
    .into_token_stream()
    .to_string();

  assert!(
    code.contains("# [doc = \" Represents the status of an item.\"]"),
    "should have first doc line"
  );
  assert!(
    code.contains("# [doc = \" Can be active or inactive.\"]"),
    "should have second doc line"
  );
}

#[test]
fn test_enum_tuple_variants() {
  let cases = [
    (
      "single type tuple",
      vec![
        VariantDef::builder()
          .name(EnumVariantToken::new("Text"))
          .content(VariantContent::Tuple(vec![TypeRef::new(RustPrimitive::String)]))
          .build(),
        VariantDef::builder()
          .name(EnumVariantToken::new("Number"))
          .content(VariantContent::Tuple(vec![TypeRef::new(RustPrimitive::I64)]))
          .build(),
        make_unit_variant("Null"),
      ],
      vec![
        ("Text (String)", "Text(String) variant"),
        ("Number (i64)", "Number(i64) variant"),
        ("Null", "Null unit variant"),
      ],
    ),
    (
      "multiple type tuple",
      vec![
        VariantDef::builder()
          .name(EnumVariantToken::new("KeyValue"))
          .content(VariantContent::Tuple(vec![
            TypeRef::new(RustPrimitive::String),
            TypeRef::new(RustPrimitive::I32),
          ]))
          .build(),
      ],
      vec![("KeyValue (String , i32)", "multi-type tuple variant")],
    ),
  ];

  for (case_name, variants, expected_content) in cases {
    let def = make_simple_enum("Value", variants);
    let code = EnumFragment::new(&def, Visibility::Public)
      .into_token_stream()
      .to_string();

    for (expected, msg) in expected_content {
      assert!(code.contains(expected), "{case_name}: should have {msg}");
    }
  }
}

#[test]
fn test_enum_variant_attributes() {
  let deprecated_def = EnumDef {
    name: EnumToken::new("ApiVersion"),
    variants: vec![
      VariantDef::builder()
        .name(EnumVariantToken::new("V1"))
        .content(VariantContent::Unit)
        .deprecated(true)
        .build(),
      make_unit_variant("V2"),
    ],
    serde_attrs: vec![],
    outer_attrs: vec![],
    case_insensitive: false,
    methods: vec![],
    ..Default::default()
  };

  let deprecated_code = EnumFragment::new(&deprecated_def, Visibility::Public)
    .into_token_stream()
    .to_string();
  assert!(
    deprecated_code.contains("# [deprecated]"),
    "should have deprecated attribute"
  );

  let outer_attrs_def = EnumDef {
    name: EnumToken::new("Flagged"),
    variants: vec![make_unit_variant("Yes"), make_unit_variant("No")],
    serde_attrs: vec![],
    outer_attrs: vec![OuterAttr::SkipSerializingNone],
    case_insensitive: false,
    methods: vec![],
    ..Default::default()
  };

  let outer_attrs_code = EnumFragment::new(&outer_attrs_def, Visibility::Public)
    .into_token_stream()
    .to_string();
  assert!(
    outer_attrs_code.contains("# [serde_with :: skip_serializing_none]"),
    "should have skip_serializing_none attribute"
  );
}

#[test]
fn test_enum_serde_attributes() {
  let cases = [
    (
      "rename",
      EnumDef {
        name: EnumToken::new("Status"),
        variants: vec![
          VariantDef::builder()
            .name(EnumVariantToken::new("InProgress"))
            .content(VariantContent::Unit)
            .serde_attrs(vec![SerdeAttribute::Rename("in_progress".to_string())])
            .build(),
          VariantDef::builder()
            .name(EnumVariantToken::new("Completed"))
            .content(VariantContent::Unit)
            .serde_attrs(vec![SerdeAttribute::Rename("completed".to_string())])
            .build(),
        ],
        serde_attrs: vec![],
        outer_attrs: vec![],
        case_insensitive: false,
        methods: vec![],
        ..Default::default()
      },
      vec![
        ("# [serde (rename = \"in_progress\")]", "serde rename for InProgress"),
        ("# [serde (rename = \"completed\")]", "serde rename for Completed"),
      ],
    ),
    (
      "untagged",
      EnumDef {
        name: EnumToken::new("AnyValue"),
        variants: vec![make_unit_variant("StringVal"), make_unit_variant("NumberVal")],
        serde_attrs: vec![SerdeAttribute::Untagged],
        outer_attrs: vec![],
        case_insensitive: false,
        methods: vec![],
        ..Default::default()
      },
      vec![("# [serde (untagged)]", "untagged serde attribute")],
    ),
  ];

  for (case_name, def, expected_attrs) in cases {
    let code = EnumFragment::new(&def, Visibility::Public)
      .into_token_stream()
      .to_string()
      .clone();
    for (expected, msg) in expected_attrs {
      assert!(code.contains(expected), "{case_name}: should have {msg}");
    }
  }
}

#[test]
fn test_case_insensitive_enum() {
  let base_def = EnumDef {
    name: EnumToken::new("Status"),
    variants: vec![
      VariantDef::builder()
        .name(EnumVariantToken::new("Active"))
        .content(VariantContent::Unit)
        .serde_attrs(vec![
          SerdeAttribute::Rename("active".to_string()),
          SerdeAttribute::Alias("Active".to_string()),
          SerdeAttribute::Alias("on".to_string()),
        ])
        .build(),
      VariantDef::builder()
        .name(EnumVariantToken::new("InProgress"))
        .content(VariantContent::Unit)
        .serde_attrs(vec![SerdeAttribute::Rename("in-progress".to_string())])
        .build(),
    ],
    serde_attrs: vec![],
    outer_attrs: vec![],
    case_insensitive: true,
    methods: vec![],
    ..Default::default()
  };

  let code = EnumFragment::new(&base_def, Visibility::Public)
    .into_token_stream()
    .to_string();

  let parts: Vec<&str> = code.split("enum Status").collect();
  assert_eq!(parts.len(), 2, "should split into derive and impl parts");
  let derive_part = parts[0];
  let impl_part = parts[1];

  assert!(
    !derive_part.contains("Deserialize"),
    "Deserialize should not be in derive attribute"
  );
  assert!(
    impl_part.contains("impl < 'de > serde :: Deserialize < 'de > for Status"),
    "Should implement Deserialize manually"
  );
  assert!(
    impl_part.contains("\"active\" | \"on\" => Ok (Self :: Active)"),
    "should match active and its alias once each, lowercased"
  );
  assert!(
    impl_part.contains("\"in-progress\" => Ok (Self :: InProgress)"),
    "should match in-progress"
  );

  let fallback_def = EnumDef {
    name: EnumToken::new("Priority"),
    variants: vec![
      make_unit_variant("High"),
      make_unit_variant("Medium"),
      make_unit_variant("Low"),
      make_unit_variant("Unknown"),
    ],
    serde_attrs: vec![],
    outer_attrs: vec![],
    case_insensitive: true,
    methods: vec![],
    ..Default::default()
  };

  let fallback_code = EnumFragment::new(&fallback_def, Visibility::Public)
    .into_token_stream()
    .to_string();
  assert!(
    fallback_code.contains("_ => Ok (Self :: Unknown)"),
    "should fallback to Unknown variant for unrecognized values"
  );
}

#[test]
fn test_case_insensitive_enum_deserialize_only() {
  let def = EnumDef {
    name: EnumToken::new("Status"),
    variants: vec![
      VariantDef::builder()
        .name(EnumVariantToken::new("Active"))
        .content(VariantContent::Unit)
        .serde_attrs(vec![SerdeAttribute::Rename("active".to_string())])
        .build(),
      VariantDef::builder()
        .name(EnumVariantToken::new("Inactive"))
        .content(VariantContent::Unit)
        .serde_attrs(vec![SerdeAttribute::Rename("inactive".to_string())])
        .build(),
    ],
    case_insensitive: true,
    serde_mode: SerdeMode::DeserializeOnly,
    ..Default::default()
  };

  let code = EnumFragment::new(&def, Visibility::Public)
    .into_token_stream()
    .to_string();

  let parts: Vec<&str> = code.split("pub enum Status").collect();
  assert_eq!(parts.len(), 2, "should split into derives and enum parts");
  let derive_part = parts[0];

  assert!(
    !derive_part.contains("Serialize"),
    "should NOT derive Serialize when DeserializeOnly"
  );
  assert!(
    !derive_part.contains("Deserialize"),
    "should NOT derive Deserialize (custom impl used for case-insensitive)"
  );
  assert!(
    !code.contains("# [serde (rename"),
    "should NOT have serde rename attrs when no serde derives"
  );
  assert!(
    code.contains("impl < 'de > serde :: Deserialize < 'de > for Status"),
    "should have custom Deserialize impl"
  );
  assert!(
    code.contains("\"active\" => Ok (Self :: Active)"),
    "custom deserialize should match 'active'"
  );
}

#[test]
fn test_enum_visibility() {
  let cases = [
    (
      Visibility::Crate,
      "pub (crate) enum Internal",
      true,
      "pub(crate) visibility",
    ),
    (
      Visibility::File,
      "pub (super) enum Private",
      true,
      "pub(super) visibility for file-level",
    ),
    (
      Visibility::File,
      "pub (crate)",
      false,
      "no pub(crate) visibility for file-level",
    ),
  ];

  for (visibility, pattern, should_contain, msg) in cases {
    let name = match visibility {
      Visibility::Crate => "Internal",
      Visibility::File => "Private",
      Visibility::Public => "Public",
    };
    let def = make_simple_enum(name, vec![make_unit_variant("A"), make_unit_variant("B")]);
    let code = EnumFragment::new(&def, visibility).into_token_stream().to_string();

    if should_contain {
      assert!(code.contains(pattern), "should have {msg}");
    } else {
      assert!(!code.contains(pattern), "should not have {msg}");
    }
  }
}

#[test]
fn test_enum_constructor_methods() {
  let simple_def = EnumDef {
    name: EnumToken::new("RequestBody"),
    variants: vec![
      VariantDef::builder()
        .name(EnumVariantToken::new("Json"))
        .content(VariantContent::Tuple(vec![TypeRef::new(RustPrimitive::Custom(
          "JsonPayload".into(),
        ))]))
        .build(),
    ],
    serde_attrs: vec![],
    outer_attrs: vec![],
    case_insensitive: false,
    methods: vec![EnumMethod::new(
      "json",
      EnumMethodKind::SimpleConstructor {
        variant_name: "Json".into(),
        wrapped_type: TypeRef::new("JsonPayload"),
      },
      Documentation::from_lines(["Creates an empty JSON body."]),
    )],
    ..Default::default()
  };

  let simple_code = EnumFragment::new(&simple_def, Visibility::Public)
    .into_token_stream()
    .to_string();
  assert!(simple_code.contains("impl RequestBody"), "should have impl block");
  assert!(
    simple_code.contains("pub fn json () -> Self"),
    "should have json constructor"
  );
  assert!(
    simple_code.contains("Self :: Json (JsonPayload :: default ())"),
    "should construct with default"
  );

  let param_def = EnumDef {
    name: EnumToken::new("Request"),
    variants: vec![
      VariantDef::builder()
        .name(EnumVariantToken::new("Create"))
        .content(VariantContent::Tuple(vec![TypeRef::new(RustPrimitive::Custom(
          "CreateParams".into(),
        ))]))
        .build(),
    ],
    serde_attrs: vec![],
    outer_attrs: vec![],
    case_insensitive: false,
    methods: vec![EnumMethod::new(
      "with_name",
      EnumMethodKind::ParameterizedConstructor {
        variant_name: "Create".into(),
        wrapped_type: TypeRef::new("CreateParams"),
        param_name: "name".to_string(),
        param_type: TypeRef::new("String"),
      },
      Documentation::from_lines(["Creates a request with the given name."]),
    )],
    ..Default::default()
  };

  let param_code = EnumFragment::new(&param_def, Visibility::Public)
    .into_token_stream()
    .to_string();
  assert!(
    param_code.contains("pub fn with_name (name : String) -> Self"),
    "should have parameterized constructor"
  );
  assert!(
    param_code.contains("Self :: Create (CreateParams { name , .. Default :: default () })"),
    "should construct with parameter"
  );
}

#[test]
fn test_enum_constructor_methods_without_docs() {
  let def = EnumDef {
    name: EnumToken::new("RequestBody"),
    variants: vec![
      VariantDef::builder()
        .name(EnumVariantToken::new("Json"))
        .content(VariantContent::Tuple(vec![TypeRef::new(RustPrimitive::Custom(
          "JsonPayload".into(),
        ))]))
        .build(),
    ],
    serde_attrs: vec![],
    outer_attrs: vec![],
    case_insensitive: false,
    methods: vec![EnumMethod::new(
      "json",
      EnumMethodKind::SimpleConstructor {
        variant_name: "Json".into(),
        wrapped_type: TypeRef::new("JsonPayload"),
      },
      Documentation::default(),
    )],
    ..Default::default()
  };

  let code = EnumFragment::new(&def, Visibility::Public)
    .into_token_stream()
    .to_string();
  assert!(code.contains("pub fn json () -> Self"), "should have json constructor");
  assert!(
    !code.contains("Creates a `"),
    "should not emit generalized constructor docs"
  );
  assert!(
    !code.contains("Convenience helper"),
    "should not emit generalized helper docs"
  );
}

#[test]
fn test_known_value_constructor_methods() {
  let def = EnumDef {
    name: EnumToken::new("ModelOption"),
    variants: vec![
      VariantDef::builder()
        .name(EnumVariantToken::new("Preset"))
        .content(VariantContent::Tuple(vec![TypeRef::new("ModelOptionPreset")]))
        .build(),
      VariantDef::builder()
        .name(EnumVariantToken::new("Custom"))
        .content(VariantContent::Tuple(vec![TypeRef::new("String")]))
        .build(),
    ],
    serde_attrs: vec![SerdeAttribute::Untagged],
    outer_attrs: vec![],
    case_insensitive: false,
    methods: vec![
      EnumMethod::new(
        "gemini_25_pro",
        EnumMethodKind::KnownValueConstructor {
          wrapper_variant: EnumVariantToken::new("Preset"),
          known_type: EnumToken::new("ModelOptionPreset"),
          known_variant: EnumVariantToken::new("Gemini25Pro"),
        },
        Documentation::from_lines(["Our best model."]),
      ),
      EnumMethod::new(
        "gemini_25_flash",
        EnumMethodKind::KnownValueConstructor {
          wrapper_variant: EnumVariantToken::new("Preset"),
          known_type: EnumToken::new("ModelOptionPreset"),
          known_variant: EnumVariantToken::new("Gemini25Flash"),
        },
        Documentation::default(),
      ),
    ],
    ..Default::default()
  };

  let code = EnumFragment::new(&def, Visibility::Public)
    .into_token_stream()
    .to_string();
  assert!(
    code.contains("pub fn gemini_25_pro () -> Self"),
    "should have gemini_25_pro constructor"
  );
  assert!(
    code.contains("Self :: Preset (ModelOptionPreset :: Gemini25Pro)"),
    "should construct the wrapper variant with the inner enum value"
  );
  assert!(
    code.contains("pub fn gemini_25_flash () -> Self"),
    "should have gemini_25_flash constructor"
  );
  assert!(code.contains("Our best model"), "should include docs on method");
}

#[test]
fn test_discriminated_enum() {
  let without_fallback = DiscriminatedEnumDef::builder()
    .name("Pet".into())
    .docs(Documentation::from_lines(["A pet can be a dog or cat."]))
    .discriminator_field("petType".to_string())
    .variants(vec![
      DiscriminatedVariant::builder()
        .discriminator_values(vec!["dog".to_string()])
        .variant_name(EnumVariantToken::new("Dog"))
        .type_name(TypeRef::new("DogData"))
        .build(),
      DiscriminatedVariant::builder()
        .discriminator_values(vec!["cat".to_string()])
        .variant_name(EnumVariantToken::new("Cat"))
        .type_name(TypeRef::new("CatData"))
        .build(),
    ])
    .serde_mode(SerdeMode::Both)
    .build();

  let code = DiscriminatedEnumFragment::new(&without_fallback, Visibility::Public)
    .into_token_stream()
    .to_string();
  let assertions_without = [
    (
      "# [derive (Debug , Clone , PartialEq)]",
      "should derive Debug, Clone, PartialEq",
    ),
    ("pub enum Pet", "should have pub enum declaration"),
    ("Dog (DogData)", "should have dog variant"),
    ("Cat (CatData)", "should have cat variant"),
    (
      "value . get (\"petType\")",
      "should read the discriminator field by name",
    ),
    ("impl Default for Pet", "should have Default impl"),
    ("impl serde :: Serialize for Pet", "should have Serialize impl"),
    (
      "impl < 'de > serde :: Deserialize < 'de > for Pet",
      "should have Deserialize impl",
    ),
    ("Some (\"dog\")", "should have dog discriminator match"),
    ("Some (\"cat\")", "should have cat discriminator match"),
    ("missing_field (\"petType\")", "should error on missing discriminator"),
  ];
  for (expected, msg) in assertions_without {
    assert!(code.contains(expected), "{msg}:\n{code}");
  }

  let with_fallback = DiscriminatedEnumDef::builder()
    .name("Message".into())
    .discriminator_field("type".to_string())
    .variants(vec![
      DiscriminatedVariant::builder()
        .discriminator_values(vec!["text".to_string()])
        .variant_name(EnumVariantToken::new("Text"))
        .type_name(TypeRef::new("TextMessage"))
        .build(),
    ])
    .fallback(
      DiscriminatedVariant::builder()
        .discriminator_values(vec![])
        .variant_name(EnumVariantToken::new("Unknown"))
        .type_name(TypeRef::new("serde_json::Value"))
        .build(),
    )
    .serde_mode(SerdeMode::Both)
    .build();

  let code_with = DiscriminatedEnumFragment::new(&with_fallback, Visibility::Public)
    .into_token_stream()
    .to_string();
  let fallback_assertions = [
    ("Unknown (serde_json :: Value)", "should have fallback variant in enum"),
    (
      "Self :: Unknown (< serde_json :: Value >",
      "should use fallback in Default impl",
    ),
    (
      "None => serde_json :: from_value (value) . map (Self :: Unknown)",
      "should use fallback when discriminator missing",
    ),
  ];
  for (expected, msg) in fallback_assertions {
    assert!(code_with.contains(expected), "{msg}:\n{code_with}");
  }
  assert!(
    !code_with.contains("missing_field"),
    "should not error on missing field when fallback exists"
  );
}

#[test]
fn test_discriminated_enum_serialize_only() {
  let def = DiscriminatedEnumDef::builder()
    .name("RequestType".into())
    .discriminator_field("kind".to_string())
    .variants(vec![
      DiscriminatedVariant::builder()
        .discriminator_values(vec!["create".to_string()])
        .variant_name(EnumVariantToken::new("Create"))
        .type_name(TypeRef::new("CreateRequest"))
        .build(),
    ])
    .serde_mode(SerdeMode::SerializeOnly)
    .build();

  let code = DiscriminatedEnumFragment::new(&def, Visibility::Public)
    .into_token_stream()
    .to_string();
  assert!(
    code.contains("impl serde :: Serialize for RequestType"),
    "should have Serialize impl"
  );
  assert!(
    !code.contains("impl < 'de > serde :: Deserialize"),
    "should NOT have Deserialize impl"
  );
}

#[test]
fn test_discriminated_enum_deserialize_only() {
  let def = DiscriminatedEnumDef::builder()
    .name("ResponseType".into())
    .discriminator_field("kind".to_string())
    .variants(vec![
      DiscriminatedVariant::builder()
        .discriminator_values(vec!["success".to_string()])
        .variant_name(EnumVariantToken::new("Success"))
        .type_name(TypeRef::new("SuccessResponse"))
        .build(),
    ])
    .maybe_fallback(None)
    .serde_mode(SerdeMode::DeserializeOnly)
    .build();

  let code = DiscriminatedEnumFragment::new(&def, Visibility::Public)
    .into_token_stream()
    .to_string();
  assert!(
    !code.contains("impl serde :: Serialize"),
    "should NOT have Serialize impl"
  );
  assert!(
    code.contains("impl < 'de > serde :: Deserialize < 'de > for ResponseType"),
    "should have Deserialize impl"
  );
}

#[test]
fn test_response_response_enum_generation() {
  let def = ResponseEnumDef::builder()
    .name(EnumToken::new("ApiResponse"))
    .docs(Documentation::from_lines(["Shared response enum"]))
    .variants(vec![
      ResponseEnumVariant::builder()
        .name(EnumVariantToken::new("Ok"))
        .status_code(StatusCodeToken::Ok200)
        .payload(ResponsePayload::Value)
        .build(),
      ResponseEnumVariant::builder()
        .name(EnumVariantToken::new("NotFound"))
        .status_code(StatusCodeToken::NotFound404)
        .build(),
      ResponseEnumVariant::builder()
        .name(EnumVariantToken::new("ClientError"))
        .status_code(StatusCodeToken::ClientError4XX)
        .payload(ResponsePayload::Failure)
        .build(),
      ResponseEnumVariant::builder()
        .name(EnumVariantToken::new("Other"))
        .status_code(StatusCodeToken::Default)
        .payload(ResponsePayload::Raw)
        .build(),
    ])
    .build();

  let code = ResponseEnumFragment::new(Visibility::Public, &def)
    .into_token_stream()
    .to_string();

  let assertions = [
    (
      "pub enum ApiResponse < Value , Failure >",
      "should declare both type parameters",
    ),
    ("# [derive (Debug , Clone)]", "should derive Debug and Clone"),
    ("Ok (Value)", "success variant should carry Value"),
    ("NotFound ,", "bodiless status should be a unit variant"),
    (
      "ClientError (http :: StatusCode , Failure)",
      "range variant should carry the status and Failure",
    ),
    (
      "Other (http :: StatusCode , Vec < u8 >)",
      "raw variant should carry the status and bytes",
    ),
    ("# [doc = \"200\"]", "should document the status code"),
    ("# [doc = \"4XX\"]", "should document the range"),
  ];
  for (expected, msg) in assertions {
    assert!(code.contains(expected), "{msg}: {code}");
  }

  let success_only = ResponseEnumDef::builder()
    .name(EnumToken::new("ApiResponse"))
    .variants(vec![
      ResponseEnumVariant::builder()
        .name(EnumVariantToken::new("Ok"))
        .status_code(StatusCodeToken::Ok200)
        .payload(ResponsePayload::Value)
        .build(),
    ])
    .build();
  let code = ResponseEnumFragment::new(Visibility::Public, &success_only)
    .into_token_stream()
    .to_string();
  assert!(
    code.contains("pub enum ApiResponse < Value >"),
    "unused error parameter should be omitted: {code}"
  );

  let client = ResponseEnumDef::builder()
    .name(EnumToken::new("ApiResponse"))
    .variants(vec![
      ResponseEnumVariant::builder()
        .name(EnumVariantToken::new("Other"))
        .status_code(StatusCodeToken::Default)
        .payload(ResponsePayload::Unread)
        .build(),
    ])
    .with_headers(StructToken::new("WithHeaders"))
    .build();
  let code = ResponseEnumFragment::new(Visibility::Public, &client)
    .into_token_stream()
    .to_string();
  let assertions = [
    (
      "# [derive (Debug)] pub enum ApiResponse",
      "an unread response is not Clone",
    ),
    (
      "Other (Box < reqwest :: Response >)",
      "the unread variant should carry only the boxed response",
    ),
    (
      "# [derive (Debug , Clone)] pub struct WithHeaders",
      "WithHeaders stays Clone",
    ),
  ];
  for (expected, msg) in assertions {
    assert!(code.contains(expected), "{msg}: {code}");
  }
}

#[test]
fn test_payload_union_generation() {
  let variants = vec![
    ResponseUnionVariant::builder()
      .name(EnumVariantToken::new("BasicError"))
      .rust_type(TypeRef::new("BasicError"))
      .build(),
    ResponseUnionVariant::builder()
      .name(EnumVariantToken::new("ValidationError"))
      .rust_type(TypeRef::new("ValidationError"))
      .build(),
  ];
  let cases = [
    (
      None,
      vec!["# [derive (Debug , Clone)]", "pub enum BasicErrorOrValidationError"],
      vec!["serde", "Serialize"],
    ),
    (
      Some(GenerationTarget::Client),
      vec!["# [derive (Debug)]"],
      vec!["Clone", "serde"],
    ),
    (Some(GenerationTarget::Server), vec![], vec!["derive", "serde"]),
  ];

  for (streaming, present, absent) in cases {
    let def = ResponseUnionDef::builder()
      .name(EnumToken::new("BasicErrorOrValidationError"))
      .variants(variants.clone())
      .maybe_streaming(streaming)
      .build();
    let code = ResponseUnionFragment::new(Visibility::Public, &def)
      .into_token_stream()
      .to_string();

    assert!(
      code.contains("BasicError (BasicError) , ValidationError (ValidationError)"),
      "members should be tuple variants for streaming={streaming:?}: {code}"
    );
    for expected in present {
      assert!(
        code.contains(expected),
        "expected `{expected}` for streaming={streaming:?}: {code}"
      );
    }
    for unexpected in absent {
      assert!(
        !code.contains(unexpected),
        "did not expect `{unexpected}` for streaming={streaming:?}: {code}"
      );
    }
  }
}

#[test]
fn test_non_exhaustive_enum_generation() {
  let def = EnumDef {
    non_exhaustive: true,
    ..make_simple_enum(
      "CuisineType",
      vec![
        VariantDef::builder()
          .name(EnumVariantToken::new("Italian"))
          .content(VariantContent::Unit)
          .serde_attrs(vec![SerdeAttribute::Rename("italian".to_string())])
          .build(),
        VariantDef::builder()
          .name(EnumVariantToken::new("Thai"))
          .content(VariantContent::Unit)
          .serde_attrs(vec![
            SerdeAttribute::Rename("thai".to_string()),
            SerdeAttribute::Alias("Thai-Food".to_string()),
          ])
          .build(),
        VariantDef::builder()
          .name(EnumVariantToken::new(OTHER_ENUM_VARIANT))
          .content(VariantContent::Tuple(vec![TypeRef::new(RustPrimitive::String)]))
          .serde_attrs(vec![SerdeAttribute::Untagged])
          .build(),
      ],
    )
  };

  let shared_expected = [
    "# [non_exhaustive] pub enum CuisineType",
    "# [serde (untagged)] Other (String)",
    "Self :: Italian => write ! (f , \"italian\")",
    "Self :: Other (v) => write ! (f , \"{v}\")",
    "impl From < String > for CuisineType { fn from (value : String) -> Self {",
    "_ => Self :: Other (value) ,",
  ];
  let shared_unexpected = ["\"other\""];
  let cases = [
    (
      false,
      vec![
        "Eq , Hash , Serialize , Deserialize",
        "match value . as_str () { \"italian\" => Self :: Italian , \"thai\" | \"Thai-Food\" => Self :: Thai ,",
        "impl core :: str :: FromStr for CuisineType",
        "Ok (Self :: from (s . to_string ()))",
      ],
      vec!["unknown variant"],
    ),
    (
      true,
      vec![
        "Ok (Self :: from (s . to_string ()))",
        "String :: deserialize (deserializer) . map (Self :: from)",
        "match value . to_ascii_lowercase () . as_str () { \"italian\" => Self :: Italian ,",
        "\"thai\" | \"thai-food\" => Self :: Thai ,",
      ],
      vec!["unknown variant", "unknown_variant"],
    ),
    (
      true,
      vec![
        "impl < 'de > serde :: Deserialize < 'de > for CuisineType",
        "String :: deserialize (deserializer) . map (Self :: from)",
        "\"thai\" | \"thai-food\" => Self :: Thai ,",
        "impl core :: str :: FromStr for CuisineType",
      ],
      vec!["unknown_variant"],
    ),
  ];

  for (case_insensitive, expected, unexpected) in cases {
    let def = EnumDef {
      case_insensitive,
      ..def.clone()
    };
    let code = EnumFragment::new(&def, Visibility::Public)
      .into_token_stream()
      .to_string();

    for pattern in shared_expected.iter().chain(&expected) {
      assert!(
        code.contains(pattern),
        "case_insensitive={case_insensitive}: expected `{pattern}`\n{code}"
      );
    }
    for pattern in shared_unexpected.iter().chain(&unexpected) {
      assert!(
        !code.contains(pattern),
        "case_insensitive={case_insensitive}: did not expect `{pattern}`\n{code}"
      );
    }
  }
}

#[test]
fn test_bool_and_mixed_value_enum_serde() {
  let value_variant = |value: serde_json::Value| {
    VariantDef::builder()
      .value(&value)
      .expect("supported enum value")
      .content(VariantContent::Unit)
      .build()
  };
  let cases = [
    (
      vec![json!(true), json!(false)],
      RustPrimitive::Bool,
      vec![
        "Self :: True => true , Self :: False => false ,",
        "serializer . serialize_bool (value)",
        "let value = bool :: deserialize (deserializer) ? ; if value { Ok (Self :: True) } else { Ok (Self :: False) }",
      ],
      vec!["# [serde (rename", "match value {"],
    ),
    (
      vec![json!(true)],
      RustPrimitive::Bool,
      vec!["if value { Ok (Self :: True) } else { Err (serde :: de :: Error :: custom"],
      vec!["Self :: False"],
    ),
    (
      vec![json!("auto"), json!(1), json!(0.5), json!(true)],
      RustPrimitive::Value,
      vec![
        "Self :: Auto => serializer . serialize_str (\"auto\") ,",
        "Self :: Value1 => serializer . serialize_i64 (1i64) ,",
        "Self :: Value0_5 => serializer . serialize_f64 (0.5f64) ,",
        "Self :: True => serializer . serialize_bool (true) ,",
        "let value = serde_json :: Value :: deserialize (deserializer) ? ; match & value {",
        "serde_json :: Value :: String (s) if matches ! (s . as_str () , \"auto\") => Ok (Self :: Auto) ,",
        "serde_json :: Value :: Number (n) if n . as_i64 () == Some (1i64) => Ok (Self :: Value1) ,",
        "serde_json :: Value :: Number (n) if n . as_f64 () . map (f64 :: to_bits) == Some ((0.5f64) . to_bits ()) => Ok (Self :: Value0_5) ,",
        "serde_json :: Value :: Bool (true) => Ok (Self :: True) ,",
        "expected one of: {}\" , value , \"auto, 1, 0.5, true\"",
      ],
      vec![
        "# [serde (rename",
        "# [derive (Debug , Clone , PartialEq , Eq , Hash , Serialize",
      ],
    ),
  ];

  for (values, primitive, expected, unexpected) in cases {
    let def = EnumDef {
      scalar_repr: Some(primitive.clone()),
      ..make_simple_enum("Choice", values.into_iter().map(value_variant).collect())
    };
    let code = EnumFragment::new(&def, Visibility::Public)
      .into_token_stream()
      .to_string();

    for pattern in &expected {
      assert!(code.contains(pattern), "{primitive:?}: expected `{pattern}`\n{code}");
    }
    for pattern in &unexpected {
      assert!(
        !code.contains(pattern),
        "{primitive:?}: did not expect `{pattern}`\n{code}"
      );
    }
  }
}

#[test]
fn test_mixed_value_enum_text_arms_match_parsed_values() {
  let variants = [
    json!("auto"),
    json!(1),
    json!(-1),
    json!(0.5),
    json!(0.000_001),
    json!(true),
  ]
  .map(|value| {
    VariantDef::builder()
      .value(&value)
      .expect("supported enum value")
      .content(VariantContent::Unit)
      .build()
  });
  let cases = [
    (
      false,
      vec![
        "serde_json :: Value :: String (s) if s == \"true\" => Ok (Self :: True) ,",
        "serde_json :: Value :: String (s) if s . parse :: < i64 > () . ok () == Some (1i64) => Ok (Self :: Value1) ,",
        "serde_json :: Value :: String (s) if s . parse :: < i64 > () . ok () == Some (- 1i64) => Ok (Self :: Value_1) ,",
        "serde_json :: Value :: String (s) if s . parse :: < f64 > () . ok () . map (f64 :: to_bits) == Some ((0.5f64) . to_bits ()) => Ok (Self :: Value0_5) ,",
        "serde_json :: Value :: String (s) if s . parse :: < f64 > () . ok () . map (f64 :: to_bits) == Some ((0.000_001f64) . to_bits ()) => Ok (Self :: Value0_000001) ,",
      ],
    ),
    (
      true,
      vec!["serde_json :: Value :: String (s) if s . eq_ignore_ascii_case (\"true\") => Ok (Self :: True) ,"],
    ),
  ];

  for (case_insensitive, expected) in cases {
    let def = EnumDef {
      scalar_repr: Some(RustPrimitive::Value),
      case_insensitive,
      ..make_simple_enum("Choice", variants.to_vec())
    };
    let code = EnumFragment::new(&def, Visibility::Public)
      .into_token_stream()
      .to_string();

    for pattern in &expected {
      assert!(
        code.contains(pattern),
        "case_insensitive={case_insensitive}: expected `{pattern}`\n{code}"
      );
    }
    for canonical in ["\"1\"", "\"0.5\"", "\"true\""] {
      let pattern = format!("matches ! (s . as_str () , {canonical})");
      assert!(
        !code.contains(&pattern),
        "case_insensitive={case_insensitive}: text arms must compare parsed values, not `{pattern}`\n{code}"
      );
    }
  }
}

#[test]
fn test_non_simple_enum_without_generate_display_has_no_display() {
  let def = EnumDef {
    name: EnumToken::new("StringOrNumber"),
    variants: vec![
      VariantDef::builder()
        .name(EnumVariantToken::new("StringVal"))
        .content(VariantContent::Tuple(vec![TypeRef::new("String")]))
        .build(),
      VariantDef::builder()
        .name(EnumVariantToken::new("NumberVal"))
        .content(VariantContent::Tuple(vec![TypeRef::new("f64")]))
        .build(),
    ],
    generate_display: false,
    ..Default::default()
  };

  let code = EnumFragment::new(&def, Visibility::Public)
    .into_token_stream()
    .to_string();

  assert!(
    !code.contains("impl core :: fmt :: Display"),
    "should NOT generate Display impl for non-simple enum without generate_display flag"
  );
}

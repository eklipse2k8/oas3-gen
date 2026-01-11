use std::rc::Rc;

use crate::generator::{
  ast::{
    DiscriminatedEnumDef, DiscriminatedVariant, Documentation, EnumDef, EnumMethod, EnumMethodKind, EnumToken,
    EnumVariantToken, OuterAttr, ResponseEnumDef, ResponseMediaType, ResponseVariant, RustPrimitive, SerdeAttribute,
    SerdeMode, StatusCodeToken, StructToken, TypeRef, VariantContent, VariantDef,
  },
  codegen::{
    CodeGenerationContext, Visibility,
    enums::{DiscriminatedEnumGenerator, EnumGenerator, ResponseEnumGenerator},
  },
  naming::constants::{KNOWN_ENUM_VARIANT, OTHER_ENUM_VARIANT},
};

fn default_context() -> Rc<CodeGenerationContext> {
  Rc::new(CodeGenerationContext::default())
}

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
    discriminator: None,
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

  let code = EnumGenerator::new(&default_context(), &def, Visibility::Public)
    .generate()
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
  // Simple enum without renames - Display uses variant name
  let simple_def = make_simple_enum(
    "Color",
    vec![
      make_unit_variant("Red"),
      make_unit_variant("Green"),
      make_unit_variant("Blue"),
    ],
  );

  let code = EnumGenerator::new(&default_context(), &simple_def, Visibility::Public)
    .generate()
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
fn test_simple_enum_display_impl_with_serde_rename() {
  // Enum with serde renames - Display uses the serde rename value
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
    discriminator: None,
    serde_attrs: vec![],
    outer_attrs: vec![],
    case_insensitive: false,
    methods: vec![],
    generate_display: true,
    ..Default::default()
  };

  let code = EnumGenerator::new(&default_context(), &renamed_def, Visibility::Public)
    .generate()
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
  // Enums with tuple variants should NOT get a Display impl
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
    discriminator: None,
    serde_attrs: vec![],
    outer_attrs: vec![],
    case_insensitive: false,
    methods: vec![],
    ..Default::default()
  };

  let code = EnumGenerator::new(&default_context(), &tuple_def, Visibility::Public)
    .generate()
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
    discriminator: None,
    serde_attrs: vec![],
    outer_attrs: vec![],
    case_insensitive: false,
    methods: vec![],
    ..Default::default()
  };

  let code = EnumGenerator::new(&default_context(), &def, Visibility::Public)
    .generate()
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
    let code = EnumGenerator::new(&default_context(), &def, Visibility::Public)
      .generate()
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
    discriminator: None,
    serde_attrs: vec![],
    outer_attrs: vec![],
    case_insensitive: false,
    methods: vec![],
    ..Default::default()
  };

  let deprecated_code = EnumGenerator::new(&default_context(), &deprecated_def, Visibility::Public)
    .generate()
    .to_string();
  assert!(
    deprecated_code.contains("# [deprecated]"),
    "should have deprecated attribute"
  );

  let outer_attrs_def = EnumDef {
    name: EnumToken::new("Flagged"),
    variants: vec![make_unit_variant("Yes"), make_unit_variant("No")],
    discriminator: None,
    serde_attrs: vec![],
    outer_attrs: vec![OuterAttr::SkipSerializingNone],
    case_insensitive: false,
    methods: vec![],
    ..Default::default()
  };

  let outer_attrs_code = EnumGenerator::new(&default_context(), &outer_attrs_def, Visibility::Public)
    .generate()
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
        discriminator: None,
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
      "discriminator tag",
      EnumDef {
        name: EnumToken::new("Event"),
        variants: vec![
          make_unit_variant("Created"),
          make_unit_variant("Updated"),
          make_unit_variant("Deleted"),
        ],
        discriminator: Some("eventType".to_string()),
        serde_attrs: vec![],
        outer_attrs: vec![],
        case_insensitive: false,
        methods: vec![],
        ..Default::default()
      },
      vec![("# [serde (tag = \"eventType\")]", "serde tag attribute")],
    ),
    (
      "untagged",
      EnumDef {
        name: EnumToken::new("AnyValue"),
        variants: vec![make_unit_variant("StringVal"), make_unit_variant("NumberVal")],
        discriminator: None,
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
    let code = EnumGenerator::new(&default_context(), &def, Visibility::Public)
      .generate()
      .to_string();
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
        .serde_attrs(vec![SerdeAttribute::Rename("active".to_string())])
        .build(),
      VariantDef::builder()
        .name(EnumVariantToken::new("InProgress"))
        .content(VariantContent::Unit)
        .serde_attrs(vec![SerdeAttribute::Rename("in-progress".to_string())])
        .build(),
    ],
    discriminator: None,
    serde_attrs: vec![],
    outer_attrs: vec![],
    case_insensitive: true,
    methods: vec![],
    ..Default::default()
  };

  let tokens = EnumGenerator::new(&default_context(), &base_def, Visibility::Public).generate();
  let code = tokens.to_string();

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
    impl_part.contains("\"active\" => Ok (Status :: Active)"),
    "should match active"
  );
  assert!(
    impl_part.contains("\"in-progress\" => Ok (Status :: InProgress)"),
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
    discriminator: None,
    serde_attrs: vec![],
    outer_attrs: vec![],
    case_insensitive: true,
    methods: vec![],
    ..Default::default()
  };

  let fallback_code = EnumGenerator::new(&default_context(), &fallback_def, Visibility::Public)
    .generate()
    .to_string();
  assert!(
    fallback_code.contains("_ => Ok (Priority :: Unknown)"),
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

  let code = EnumGenerator::new(&default_context(), &def, Visibility::Public)
    .generate()
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
    code.contains("\"active\" => Ok (Status :: Active)"),
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
      "pub enum",
      false,
      "no pub visibility for file-private",
    ),
    (
      Visibility::File,
      "pub (crate)",
      false,
      "no pub(crate) visibility for file-private",
    ),
    (Visibility::File, "enum Private", true, "private visibility"),
  ];

  for (visibility, pattern, should_contain, msg) in cases {
    let name = match visibility {
      Visibility::Crate => "Internal",
      Visibility::File => "Private",
      Visibility::Public => "Public",
    };
    let def = make_simple_enum(name, vec![make_unit_variant("A"), make_unit_variant("B")]);
    let code = EnumGenerator::new(&default_context(), &def, visibility)
      .generate()
      .to_string();

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
    discriminator: None,
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

  let simple_code = EnumGenerator::new(&default_context(), &simple_def, Visibility::Public)
    .generate()
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
    discriminator: None,
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

  let param_code = EnumGenerator::new(&default_context(), &param_def, Visibility::Public)
    .generate()
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
    discriminator: None,
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

  let code = EnumGenerator::new(&default_context(), &def, Visibility::Public)
    .generate()
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
        .name(EnumVariantToken::new(KNOWN_ENUM_VARIANT))
        .content(VariantContent::Tuple(vec![TypeRef::new("ModelOptionKnown")]))
        .build(),
      VariantDef::builder()
        .name(EnumVariantToken::new(OTHER_ENUM_VARIANT))
        .content(VariantContent::Tuple(vec![TypeRef::new("String")]))
        .build(),
    ],
    discriminator: None,
    serde_attrs: vec![],
    outer_attrs: vec![],
    case_insensitive: false,
    methods: vec![
      EnumMethod::new(
        "gemini_25_pro",
        EnumMethodKind::KnownValueConstructor {
          known_type: EnumToken::new("ModelOptionKnown"),
          known_variant: EnumVariantToken::new("Gemini25Pro"),
        },
        Documentation::from_lines(["Our best model."]),
      ),
      EnumMethod::new(
        "gemini_25_flash",
        EnumMethodKind::KnownValueConstructor {
          known_type: EnumToken::new("ModelOptionKnown"),
          known_variant: EnumVariantToken::new("Gemini25Flash"),
        },
        Documentation::default(),
      ),
    ],
    ..Default::default()
  };

  let code = EnumGenerator::new(&default_context(), &def, Visibility::Public)
    .generate()
    .to_string();
  assert!(
    code.contains("pub fn gemini_25_pro () -> Self"),
    "should have gemini_25_pro constructor"
  );
  assert!(
    code.contains("Self :: Known (ModelOptionKnown :: Gemini25Pro)"),
    "should construct Known variant with inner enum value"
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

  let code = DiscriminatedEnumGenerator::new(&default_context(), &without_fallback, Visibility::Public)
    .generate()
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
      "pub const DISCRIMINATOR_FIELD",
      "should have discriminator field constant",
    ),
    ("\"petType\"", "should have discriminator field value"),
    ("impl Default for Pet", "should have Default impl"),
    ("impl serde :: Serialize for Pet", "should have Serialize impl"),
    (
      "impl < 'de > serde :: Deserialize < 'de > for Pet",
      "should have Deserialize impl",
    ),
    ("Some (\"dog\")", "should have dog discriminator match"),
    ("Some (\"cat\")", "should have cat discriminator match"),
    (
      "missing_field (Self :: DISCRIMINATOR_FIELD)",
      "should error on missing discriminator",
    ),
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

  let code_with = DiscriminatedEnumGenerator::new(&default_context(), &with_fallback, Visibility::Public)
    .generate()
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

  let code = DiscriminatedEnumGenerator::new(&default_context(), &def, Visibility::Public)
    .generate()
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

  let code = DiscriminatedEnumGenerator::new(&default_context(), &def, Visibility::Public)
    .generate()
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
fn test_response_enum_generation() {
  let def = ResponseEnumDef {
    name: EnumToken::new("GetUserResponse"),
    docs: Documentation::from_lines(["Response for GET /users/{id}"]),
    variants: vec![
      ResponseVariant::builder()
        .status_code(StatusCodeToken::Ok200)
        .variant_name(EnumVariantToken::new("Ok"))
        .description("User found".to_string())
        .media_types(vec![ResponseMediaType::with_schema(
          "application/json",
          Some(TypeRef::new(RustPrimitive::Custom("User".into()))),
        )])
        .schema_type(TypeRef::new(RustPrimitive::Custom("User".into())))
        .build(),
      ResponseVariant::builder()
        .status_code(StatusCodeToken::NotFound404)
        .variant_name(EnumVariantToken::new("NotFound"))
        .description("User not found".to_string())
        .media_types(vec![ResponseMediaType::new("application/json")])
        .build(),
      ResponseVariant::builder()
        .status_code(StatusCodeToken::InternalServerError500)
        .variant_name(EnumVariantToken::new("InternalServerError"))
        .media_types(vec![ResponseMediaType::with_schema(
          "application/json",
          Some(TypeRef::new(RustPrimitive::Custom("ErrorResponse".into()))),
        )])
        .schema_type(TypeRef::new(RustPrimitive::Custom("ErrorResponse".into())))
        .build(),
    ],
    request_type: Some(StructToken::new("GetUserRequest")),
    try_from: vec![],
  };

  let code = ResponseEnumGenerator::new(&default_context(), &def, Visibility::Public)
    .generate()
    .to_string();

  let assertions = [
    ("pub enum GetUserResponse", "should have pub enum declaration"),
    ("# [derive (Debug , Clone)]", "should derive Debug and Clone"),
    ("Ok (User)", "should have Ok variant with User type"),
    ("NotFound", "should have NotFound unit variant"),
    (
      "InternalServerError (ErrorResponse)",
      "should have error variant with type",
    ),
    (
      "# [doc = \"200: User found\"]",
      "should have doc with status and description",
    ),
    ("# [doc = \"404: User not found\"]", "should have doc for 404"),
    (
      "# [doc = \"500\"]",
      "should have doc with just status when no description",
    ),
  ];
  for (expected, msg) in assertions {
    assert!(code.contains(expected), "{msg}");
  }
}

#[test]
fn test_relaxed_wrapper_enum_generates_display() {
  let def = EnumDef {
    name: EnumToken::new("CuisineType"),
    variants: vec![
      VariantDef::builder()
        .name(EnumVariantToken::new(KNOWN_ENUM_VARIANT))
        .content(VariantContent::Tuple(vec![TypeRef::new("CuisineTypeKnown")]))
        .build(),
      VariantDef::builder()
        .name(EnumVariantToken::new(OTHER_ENUM_VARIANT))
        .content(VariantContent::Tuple(vec![TypeRef::new("String")]))
        .build(),
    ],
    generate_display: true,
    ..Default::default()
  };

  let code = EnumGenerator::new(&default_context(), &def, Visibility::Public)
    .generate()
    .to_string();

  let assertions = [
    (
      "impl core :: fmt :: Display for CuisineType",
      "should generate Display impl for relaxed wrapper enum",
    ),
    (
      "Self :: Known (v) => write ! (f , \"{v}\")",
      "should delegate Known variant to inner Display",
    ),
    (
      "Self :: Other (v) => write ! (f , \"{v}\")",
      "should delegate Other variant to inner Display",
    ),
  ];

  for (expected, msg) in assertions {
    assert!(code.contains(expected), "{msg}\nGenerated code:\n{code}");
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

  let code = EnumGenerator::new(&default_context(), &def, Visibility::Public)
    .generate()
    .to_string();

  assert!(
    !code.contains("impl core :: fmt :: Display"),
    "should NOT generate Display impl for non-simple enum without generate_display flag"
  );
}

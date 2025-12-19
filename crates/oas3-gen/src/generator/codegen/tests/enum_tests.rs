use crate::generator::{
  ast::{
    ContentCategory, DiscriminatedEnumDef, DiscriminatedVariant, EnumDef, EnumMethod, EnumMethodKind, EnumToken,
    EnumVariantToken, OuterAttr, ResponseEnumDef, ResponseVariant, RustPrimitive, SerdeAttribute, SerdeMode,
    StatusCodeToken, StructToken, TypeRef, VariantContent, VariantDef,
  },
  codegen::{
    Visibility,
    enums::{DiscriminatedEnumGenerator, EnumGenerator, ResponseEnumGenerator},
  },
};

fn make_unit_variant(name: &str) -> VariantDef {
  VariantDef {
    name: EnumVariantToken::from(name),
    docs: vec![],
    content: VariantContent::Unit,
    serde_attrs: vec![],
    deprecated: false,
  }
}

fn make_simple_enum(name: &str, variants: Vec<VariantDef>) -> EnumDef {
  EnumDef {
    name: EnumToken::new(name),
    docs: vec![],
    variants,
    discriminator: None,
    serde_attrs: vec![],
    outer_attrs: vec![],
    case_insensitive: false,
    methods: vec![],
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

  let code = EnumGenerator::new(&def, Visibility::Public).generate().to_string();

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
fn test_enum_with_docs() {
  let def = EnumDef {
    name: EnumToken::new("Status"),
    docs: vec![
      "Represents the status of an item.".to_string(),
      "Can be active or inactive.".to_string(),
    ],
    variants: vec![make_unit_variant("Active"), make_unit_variant("Inactive")],
    discriminator: None,
    serde_attrs: vec![],
    outer_attrs: vec![],
    case_insensitive: false,
    methods: vec![],
    ..Default::default()
  };

  let code = EnumGenerator::new(&def, Visibility::Public).generate().to_string();

  assert!(
    code.contains("# [doc = \"Represents the status of an item.\"]"),
    "should have first doc line"
  );
  assert!(
    code.contains("# [doc = \"Can be active or inactive.\"]"),
    "should have second doc line"
  );
}

#[test]
fn test_enum_tuple_variants() {
  let cases = [
    (
      "single type tuple",
      vec![
        VariantDef {
          name: EnumVariantToken::new("Text"),
          docs: vec![],
          content: VariantContent::Tuple(vec![TypeRef::new(RustPrimitive::String)]),
          serde_attrs: vec![],
          deprecated: false,
        },
        VariantDef {
          name: EnumVariantToken::new("Number"),
          docs: vec![],
          content: VariantContent::Tuple(vec![TypeRef::new(RustPrimitive::I64)]),
          serde_attrs: vec![],
          deprecated: false,
        },
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
      vec![VariantDef {
        name: EnumVariantToken::new("KeyValue"),
        docs: vec![],
        content: VariantContent::Tuple(vec![
          TypeRef::new(RustPrimitive::String),
          TypeRef::new(RustPrimitive::I32),
        ]),
        serde_attrs: vec![],
        deprecated: false,
      }],
      vec![("KeyValue (String , i32)", "multi-type tuple variant")],
    ),
  ];

  for (case_name, variants, expected_content) in cases {
    let def = make_simple_enum("Value", variants);
    let code = EnumGenerator::new(&def, Visibility::Public).generate().to_string();

    for (expected, msg) in expected_content {
      assert!(code.contains(expected), "{case_name}: should have {msg}");
    }
  }
}

#[test]
fn test_enum_variant_attributes() {
  let deprecated_def = EnumDef {
    name: EnumToken::new("ApiVersion"),
    docs: vec![],
    variants: vec![
      VariantDef {
        name: EnumVariantToken::new("V1"),
        docs: vec![],
        content: VariantContent::Unit,
        serde_attrs: vec![],
        deprecated: true,
      },
      make_unit_variant("V2"),
    ],
    discriminator: None,
    serde_attrs: vec![],
    outer_attrs: vec![],
    case_insensitive: false,
    methods: vec![],
    ..Default::default()
  };

  let deprecated_code = EnumGenerator::new(&deprecated_def, Visibility::Public)
    .generate()
    .to_string();
  assert!(
    deprecated_code.contains("# [deprecated]"),
    "should have deprecated attribute"
  );

  let outer_attrs_def = EnumDef {
    name: EnumToken::new("Flagged"),
    docs: vec![],
    variants: vec![make_unit_variant("Yes"), make_unit_variant("No")],
    discriminator: None,
    serde_attrs: vec![],
    outer_attrs: vec![OuterAttr::NonExhaustive],
    case_insensitive: false,
    methods: vec![],
    ..Default::default()
  };

  let outer_attrs_code = EnumGenerator::new(&outer_attrs_def, Visibility::Public)
    .generate()
    .to_string();
  assert!(
    outer_attrs_code.contains("# [non_exhaustive]"),
    "should have non_exhaustive attribute"
  );
}

#[test]
fn test_enum_serde_attributes() {
  let cases = [
    (
      "rename",
      EnumDef {
        name: EnumToken::new("Status"),
        docs: vec![],
        variants: vec![
          VariantDef {
            name: EnumVariantToken::new("InProgress"),
            docs: vec![],
            content: VariantContent::Unit,
            serde_attrs: vec![SerdeAttribute::Rename("in_progress".to_string())],
            deprecated: false,
          },
          VariantDef {
            name: EnumVariantToken::new("Completed"),
            docs: vec![],
            content: VariantContent::Unit,
            serde_attrs: vec![SerdeAttribute::Rename("completed".to_string())],
            deprecated: false,
          },
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
        docs: vec![],
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
        docs: vec![],
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
    let code = EnumGenerator::new(&def, Visibility::Public).generate().to_string();
    for (expected, msg) in expected_attrs {
      assert!(code.contains(expected), "{case_name}: should have {msg}");
    }
  }
}

#[test]
fn test_case_insensitive_enum() {
  let base_def = EnumDef {
    name: EnumToken::new("Status"),
    docs: vec![],
    variants: vec![
      VariantDef {
        name: EnumVariantToken::new("Active"),
        docs: vec![],
        content: VariantContent::Unit,
        serde_attrs: vec![SerdeAttribute::Rename("active".to_string())],
        deprecated: false,
      },
      VariantDef {
        name: EnumVariantToken::new("InProgress"),
        docs: vec![],
        content: VariantContent::Unit,
        serde_attrs: vec![SerdeAttribute::Rename("in-progress".to_string())],
        deprecated: false,
      },
    ],
    discriminator: None,
    serde_attrs: vec![],
    outer_attrs: vec![],
    case_insensitive: true,
    methods: vec![],
    ..Default::default()
  };

  let tokens = EnumGenerator::new(&base_def, Visibility::Public).generate();
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
    docs: vec![],
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

  let fallback_code = EnumGenerator::new(&fallback_def, Visibility::Public)
    .generate()
    .to_string();
  assert!(
    fallback_code.contains("_ => Ok (Priority :: Unknown)"),
    "should fallback to Unknown variant for unrecognized values"
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
    let code = EnumGenerator::new(&def, visibility).generate().to_string();

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
    docs: vec![],
    variants: vec![VariantDef {
      name: EnumVariantToken::new("Json"),
      docs: vec![],
      content: VariantContent::Tuple(vec![TypeRef::new(RustPrimitive::Custom("JsonPayload".into()))]),
      serde_attrs: vec![],
      deprecated: false,
    }],
    discriminator: None,
    serde_attrs: vec![],
    outer_attrs: vec![],
    case_insensitive: false,
    methods: vec![EnumMethod {
      name: "json".into(),
      docs: vec!["Creates an empty JSON body.".to_string()],
      kind: EnumMethodKind::SimpleConstructor {
        variant_name: "Json".into(),
        wrapped_type: TypeRef::new("JsonPayload"),
      },
    }],
    ..Default::default()
  };

  let simple_code = EnumGenerator::new(&simple_def, Visibility::Public)
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
    docs: vec![],
    variants: vec![VariantDef {
      name: EnumVariantToken::new("Create"),
      docs: vec![],
      content: VariantContent::Tuple(vec![TypeRef::new(RustPrimitive::Custom("CreateParams".into()))]),
      serde_attrs: vec![],
      deprecated: false,
    }],
    discriminator: None,
    serde_attrs: vec![],
    outer_attrs: vec![],
    case_insensitive: false,
    methods: vec![EnumMethod {
      name: "with_name".into(),
      docs: vec!["Creates a request with the given name.".to_string()],
      kind: EnumMethodKind::ParameterizedConstructor {
        variant_name: "Create".into(),
        wrapped_type: TypeRef::new("CreateParams"),
        param_name: "name".to_string(),
        param_type: TypeRef::new("String"),
      },
    }],
    ..Default::default()
  };

  let param_code = EnumGenerator::new(&param_def, Visibility::Public)
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
fn test_discriminated_enum() {
  let without_fallback = DiscriminatedEnumDef {
    name: EnumToken::new("Pet"),
    docs: vec!["A pet can be a dog or cat.".to_string()],
    discriminator_field: "petType".to_string(),
    variants: vec![
      DiscriminatedVariant {
        discriminator_value: "dog".to_string(),
        variant_name: "Dog".to_string(),
        type_name: TypeRef::new("DogData"),
      },
      DiscriminatedVariant {
        discriminator_value: "cat".to_string(),
        variant_name: "Cat".to_string(),
        type_name: TypeRef::new("CatData"),
      },
    ],
    fallback: None,
    serde_mode: SerdeMode::Both,
    methods: vec![],
  };

  let code_without = DiscriminatedEnumGenerator::new(&without_fallback, Visibility::Public)
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
    assert!(code_without.contains(expected), "{msg}:\n{code_without}");
  }

  let with_fallback = DiscriminatedEnumDef {
    name: EnumToken::new("Message"),
    docs: vec![],
    discriminator_field: "type".to_string(),
    variants: vec![DiscriminatedVariant {
      discriminator_value: "text".to_string(),
      variant_name: "Text".to_string(),
      type_name: TypeRef::new("TextMessage"),
    }],
    fallback: Some(DiscriminatedVariant {
      discriminator_value: String::new(),
      variant_name: "Unknown".to_string(),
      type_name: TypeRef::new("serde_json::Value"),
    }),
    serde_mode: SerdeMode::Both,
    methods: vec![],
  };

  let code_with = DiscriminatedEnumGenerator::new(&with_fallback, Visibility::Public)
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
  let def = DiscriminatedEnumDef {
    name: EnumToken::new("RequestType"),
    docs: vec![],
    discriminator_field: "kind".to_string(),
    variants: vec![DiscriminatedVariant {
      discriminator_value: "create".to_string(),
      variant_name: "Create".to_string(),
      type_name: TypeRef::new("CreateRequest"),
    }],
    fallback: None,
    serde_mode: SerdeMode::SerializeOnly,
    methods: vec![],
  };

  let code = DiscriminatedEnumGenerator::new(&def, Visibility::Public)
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
  let def = DiscriminatedEnumDef {
    name: EnumToken::new("ResponseType"),
    docs: vec![],
    discriminator_field: "kind".to_string(),
    variants: vec![DiscriminatedVariant {
      discriminator_value: "success".to_string(),
      variant_name: "Success".to_string(),
      type_name: TypeRef::new("SuccessResponse"),
    }],
    fallback: None,
    serde_mode: SerdeMode::DeserializeOnly,
    methods: vec![],
  };

  let code = DiscriminatedEnumGenerator::new(&def, Visibility::Public)
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
    docs: vec!["Response for GET /users/{id}".to_string()],
    variants: vec![
      ResponseVariant {
        status_code: StatusCodeToken::Ok200,
        variant_name: EnumVariantToken::new("Ok"),
        description: Some("User found".to_string()),
        schema_type: Some(TypeRef::new(RustPrimitive::Custom("User".into()))),
        content_category: ContentCategory::Json,
      },
      ResponseVariant {
        status_code: StatusCodeToken::NotFound404,
        variant_name: EnumVariantToken::new("NotFound"),
        description: Some("User not found".to_string()),
        schema_type: None,
        content_category: ContentCategory::Json,
      },
      ResponseVariant {
        status_code: StatusCodeToken::InternalServerError500,
        variant_name: EnumVariantToken::new("InternalServerError"),
        description: None,
        schema_type: Some(TypeRef::new(RustPrimitive::Custom("ErrorResponse".into()))),
        content_category: ContentCategory::Json,
      },
    ],
    request_type: Some(StructToken::new("GetUserRequest")),
  };

  let code = ResponseEnumGenerator::new(&def, Visibility::Public)
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

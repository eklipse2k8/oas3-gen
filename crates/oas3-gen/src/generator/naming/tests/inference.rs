use std::collections::{BTreeMap, BTreeSet};

use oas3::spec::{ObjectOrReference, ObjectSchema, Schema, SchemaType, SchemaTypeSet};
use serde_json::{Value, json};

use crate::{
  generator::{
    ast::{EnumVariantToken, TypeRef, VariantContent, VariantDef},
    naming::{
      constants::{KNOWN_ENUM_VARIANT, REQUEST_BODY_SUFFIX},
      inference::{NormalizedVariant, derive_method_names, strip_common_affixes},
      name_index::{TypeNameIndex, compute_best_name, is_valid_common_name, longest_common_suffix},
    },
  },
  tests::common::create_test_spec,
  utils::SchemaExt,
};

fn make_string_schema() -> ObjectSchema {
  ObjectSchema {
    schema_type: Some(SchemaTypeSet::Single(SchemaType::String)),
    ..Default::default()
  }
}

fn make_string_enum_schema(values: Vec<Value>) -> ObjectSchema {
  ObjectSchema {
    schema_type: Some(SchemaTypeSet::Single(SchemaType::String)),
    enum_values: values,
    ..Default::default()
  }
}

fn make_const_schema(value: Value) -> ObjectSchema {
  ObjectSchema {
    const_value: Some(value),
    ..Default::default()
  }
}

#[test]
fn test_derive_method_names() {
  let cases = vec![
    ("RequestType", vec!["GetRequest", "PostRequest"], vec!["get", "post"]),
    (
      "Status",
      vec!["StatusActive", "Active", "StatusInactive"],
      vec!["status_active", "active", "inactive"],
    ),
    (
      "ResponseFormatOption",
      vec!["ResponseFormatText", "ResponseFormatJson", "ResponseFormatXml"],
      vec!["text", "json", "xml"],
    ),
    ("Status", vec![], vec![]),
  ];

  for (enum_name, variants, expected) in cases {
    let variants_vec = variants.into_iter().map(String::from).collect::<Vec<String>>();
    assert_eq!(
      derive_method_names(enum_name, &variants_vec),
      expected,
      "Failed for enum '{enum_name}'"
    );
  }
}

#[test]
fn test_longest_common_suffix() {
  let cases = vec![
    (vec!["CreateUser", "UpdateUser"], "ateUser"),
    (vec!["Status", "Status"], "Status"),
    (vec!["Short", "Long"], ""),
    (vec!["ABStatus", "CDStatus"], "Status"),
    (vec!["PrefixOnly", "PrefixOther"], ""),
    (vec![], ""),
  ];

  for (inputs, expected) in cases {
    let input_strings = inputs.into_iter().map(String::from).collect::<Vec<String>>();
    let refs: Vec<&String> = input_strings.iter().collect();
    assert_eq!(
      longest_common_suffix(&refs),
      expected,
      "Failed for inputs {input_strings:?}"
    );
  }
}

#[test]
fn test_is_valid_common_name() {
  let cases = vec![
    ("User", true),
    ("UserResponse", true),
    ("Enum", false),
    ("Struct", false),
    ("Type", false),
    ("Typ", false),
    ("abc", false),
    ("123Type", false),
  ];

  for (input, expected) in cases {
    assert_eq!(is_valid_common_name(input), expected, "Failed for input '{input}'");
  }
}

#[test]
#[allow(clippy::type_complexity)]
fn test_compute_best_name() {
  let cases: Vec<(Vec<(&str, bool)>, Vec<&str>, &str)> = vec![
    (vec![("SingleCandidate", false)], vec![], "SingleCandidate"),
    (vec![("Collision", false)], vec!["Collision"], "Collision2"),
    (
      vec![("NewName", false), ("ExistingName", true)],
      vec!["ExistingName"],
      "ExistingName",
    ),
    (vec![("NetUser", false), ("WebUser", false)], vec![], "User"),
    (vec![("Cat", false), ("Bat", false)], vec![], "Bat"),
    (vec![("MyGroup", false), ("YourGroup", false)], vec!["Group"], "Group2"),
  ];

  for (candidate_pairs, used_list, expected) in cases {
    let candidates = candidate_pairs
      .iter()
      .map(|(s, b)| ((*s).to_string(), *b))
      .collect::<BTreeSet<(String, bool)>>();
    let used = used_list.into_iter().map(String::from).collect::<BTreeSet<String>>();
    assert_eq!(
      compute_best_name(&candidates, &used),
      expected,
      "Failed for candidates {candidates:?} with used {used:?}"
    );
  }
}

#[test]
fn test_inline_type_scanner_known_suffix_patterns() {
  let voice_ids_shared = ObjectSchema {
    any_of: vec![
      ObjectOrReference::Object(make_string_schema()),
      ObjectOrReference::Object(make_string_enum_schema(vec![
        json!("alloy"),
        json!("ash"),
        json!("ballad"),
      ])),
    ],
    ..Default::default()
  };

  let format_type = ObjectSchema {
    any_of: vec![
      ObjectOrReference::Object(make_string_schema()),
      ObjectOrReference::Object(make_const_schema(json!("json"))),
      ObjectOrReference::Object(make_const_schema(json!("text"))),
      ObjectOrReference::Object(make_const_schema(json!("xml"))),
    ],
    ..Default::default()
  };

  let schemas = BTreeMap::from([
    ("VoiceIdsShared".to_string(), voice_ids_shared),
    ("FormatType".to_string(), format_type),
  ]);

  let spec = create_test_spec(BTreeMap::new());
  let index = TypeNameIndex::new(&schemas, &spec);
  let result = index.scan_and_compute_names().expect("Should scan successfully");

  let cases = [
    (
      vec!["alloy".to_string(), "ash".to_string(), "ballad".to_string()],
      "VoiceIdsSharedKnown",
      "StringEnumOptimizer pattern with enum_values",
    ),
    (
      vec!["json".to_string(), "text".to_string(), "xml".to_string()],
      "FormatTypeKnown",
      "anyOf with const values",
    ),
  ];

  for (enum_values, expected_name, description) in cases {
    let name = result.enum_names.get(&enum_values);
    assert_eq!(
      name,
      Some(&expected_name.to_string()),
      "{description} should be named with 'Known' suffix"
    );
  }
}

#[test]
fn test_inline_type_scanner_enum_naming_without_known_suffix() {
  let status = make_string_enum_schema(vec![json!("active"), json!("inactive"), json!("pending")]);

  let chat_model = make_string_enum_schema(vec![json!("gpt-4"), json!("gpt-3.5-turbo")]);

  let model_ids_shared = ObjectSchema {
    any_of: vec![
      ObjectOrReference::Object(make_string_schema()),
      ObjectOrReference::Ref {
        ref_path: "#/components/schemas/ChatModel".to_string(),
        summary: None,
        description: None,
      },
    ],
    ..Default::default()
  };

  let schemas = BTreeMap::from([
    ("Status".to_string(), status),
    ("ChatModel".to_string(), chat_model),
    ("ModelIdsShared".to_string(), model_ids_shared),
  ]);

  let spec = create_test_spec(BTreeMap::new());
  let index = TypeNameIndex::new(&schemas, &spec);
  let result = index.scan_and_compute_names().expect("Should scan successfully");

  let status_values = vec!["active".to_string(), "inactive".to_string(), "pending".to_string()];
  let status_name = result.enum_names.get(&status_values);
  assert!(status_name.is_some(), "Regular enum should have a precomputed name");
  assert!(
    !status_name.unwrap().ends_with(KNOWN_ENUM_VARIANT),
    "Regular enum should not have 'Known' suffix"
  );

  let chat_model_values = vec!["gpt-3.5-turbo".to_string(), "gpt-4".to_string()];
  let chat_model_name = result.enum_names.get(&chat_model_values);
  assert!(
    chat_model_name.is_some(),
    "Top-level enum referenced by anyOf should have a precomputed name"
  );
  assert!(
    chat_model_name.unwrap().starts_with("ChatModel"),
    "Precomputed name should be based on ChatModel"
  );
}

#[test]
fn test_infer_name_from_context() {
  let cases = [
    (
      ObjectSchema::default(),
      "/api/check-access-by-email",
      "200",
      "check_access_by_email200Response",
      "should sanitize hyphens",
    ),
    (
      ObjectSchema::default(),
      "/api/foo-bar.baz_qux",
      "201",
      "foo_bar_baz_qux201Response",
      "should sanitize multiple separators",
    ),
    (
      ObjectSchema::default(),
      "/api/create-user",
      REQUEST_BODY_SUFFIX,
      "create_userRequestBody",
      "should handle request body suffix",
    ),
  ];

  for (schema, path, status, expected, description) in cases {
    let result = schema.infer_name_from_context(path, status);
    assert_eq!(result, expected, "Failed: {description}");
    assert!(
      !result.contains('-') && !result.contains('.'),
      "Result should not contain hyphens or dots: {result}"
    );
  }

  let mut schema_with_property = ObjectSchema::default();
  schema_with_property
    .properties
    .insert("user".to_string(), ObjectOrReference::Object(ObjectSchema::default()));
  let result = schema_with_property.infer_name_from_context("/api/check-access", "200");
  assert_eq!(
    result, "userResponse",
    "Single property response should use property name"
  );
}

#[test]
fn test_normalize_strings() {
  let cases = [
    (json!("active"), "Active", "active"),
    (json!("pending_approval"), "PendingApproval", "pending_approval"),
    (json!("pending-approval"), "PendingApproval", "pending-approval"),
  ];

  for (val, expected_name, expected_rename) in cases {
    let res = NormalizedVariant::try_from(&val).unwrap();
    assert_eq!(res.name, expected_name, "name mismatch for {val:?}");
    assert_eq!(res.rename_value, expected_rename, "rename mismatch for {val:?}");
  }
}

#[test]
#[allow(clippy::approx_constant)]
fn test_normalize_numbers() {
  let cases = [
    (json!(404), "Value404", "404"),
    (json!(-42), "Value_42", "-42"),
    (json!(0), "Value0", "0"),
    (json!(3.14), "Value3_14", "3.14"),
    (json!(-2.5), "Value_2_5", "-2.5"),
  ];

  for (val, expected_name, expected_rename) in cases {
    let res = NormalizedVariant::try_from(&val).unwrap();
    assert_eq!(res.name, expected_name, "name mismatch for {val:?}");
    assert_eq!(res.rename_value, expected_rename, "rename mismatch for {val:?}");
  }
}

#[test]
fn test_normalize_booleans_and_unsupported_types() {
  let true_res = NormalizedVariant::try_from(&json!(true)).unwrap();
  assert_eq!(true_res.name, "True");
  assert_eq!(true_res.rename_value, "true");

  let false_res = NormalizedVariant::try_from(&json!(false)).unwrap();
  assert_eq!(false_res.name, "False");
  assert_eq!(false_res.rename_value, "false");

  assert!(NormalizedVariant::try_from(&json!(null)).is_err());
  assert!(NormalizedVariant::try_from(&json!({"key": "value"})).is_err());
  assert!(NormalizedVariant::try_from(&json!([1, 2, 3])).is_err());
}

#[test]
fn test_infer_variant_name_single_types() {
  let type_cases = [
    (SchemaType::String, "String"),
    (SchemaType::Number, "Number"),
    (SchemaType::Integer, "Integer"),
    (SchemaType::Boolean, "Boolean"),
    (SchemaType::Array, "Array"),
    (SchemaType::Object, "Object"),
    (SchemaType::Null, "Null"),
  ];

  for (schema_type, expected) in type_cases {
    let schema = ObjectSchema {
      schema_type: Some(SchemaTypeSet::Single(schema_type)),
      ..Default::default()
    };
    assert_eq!(
      schema.infer_variant_name(0),
      expected,
      "type mismatch for {schema_type:?}"
    );
  }
}

#[test]
fn test_infer_variant_name_special_cases() {
  let enum_schema = ObjectSchema {
    enum_values: vec![json!("a"), json!("b")],
    ..Default::default()
  };
  assert_eq!(enum_schema.infer_variant_name(0), "Enum");

  let multi_type_schema = ObjectSchema {
    schema_type: Some(SchemaTypeSet::Multiple(vec![SchemaType::String, SchemaType::Number])),
    ..Default::default()
  };
  assert_eq!(multi_type_schema.infer_variant_name(0), "Mixed");

  let no_type_schema = ObjectSchema::default();
  assert_eq!(no_type_schema.infer_variant_name(5), "Variant5");
}

#[test]
fn test_infer_variant_name_object_variants() {
  let bare_object = ObjectSchema {
    schema_type: Some(SchemaTypeSet::Single(SchemaType::Object)),
    ..Default::default()
  };
  assert_eq!(
    bare_object.infer_variant_name(0),
    "Object",
    "bare object without properties should be Object"
  );

  let object_with_single_property = ObjectSchema {
    schema_type: Some(SchemaTypeSet::Single(SchemaType::Object)),
    properties: [("items".to_string(), ObjectOrReference::Object(ObjectSchema::default()))]
      .into_iter()
      .collect(),
    ..Default::default()
  };
  assert_eq!(
    object_with_single_property.infer_variant_name(0),
    "Items",
    "object with single property should use property name"
  );

  let object_with_single_required = ObjectSchema {
    schema_type: Some(SchemaTypeSet::Single(SchemaType::Object)),
    properties: [
      ("id".to_string(), ObjectOrReference::Object(ObjectSchema::default())),
      ("name".to_string(), ObjectOrReference::Object(ObjectSchema::default())),
    ]
    .into_iter()
    .collect(),
    required: vec!["id".to_string()],
    ..Default::default()
  };
  assert_eq!(
    object_with_single_required.infer_variant_name(0),
    "Id",
    "object with single required field should use required field name"
  );

  let object_with_multiple_properties = ObjectSchema {
    schema_type: Some(SchemaTypeSet::Single(SchemaType::Object)),
    properties: [
      ("id".to_string(), ObjectOrReference::Object(ObjectSchema::default())),
      ("name".to_string(), ObjectOrReference::Object(ObjectSchema::default())),
    ]
    .into_iter()
    .collect(),
    ..Default::default()
  };
  assert_eq!(
    object_with_multiple_properties.infer_variant_name(0),
    "Object",
    "object with multiple properties and no single required should be Object"
  );
}

fn make_variant(name: &str) -> VariantDef {
  VariantDef::builder()
    .name(EnumVariantToken::from(name))
    .content(VariantContent::Unit)
    .build()
}

#[test]
fn test_strip_common_affixes_no_op_cases() {
  let empty: Vec<VariantDef> = vec![];
  let empty = strip_common_affixes(empty);
  assert!(empty.is_empty());

  let single = vec![make_variant("UserResponse")];
  let single = strip_common_affixes(single);
  assert_eq!(single[0].name, EnumVariantToken::new("UserResponse"));
}

#[test]
fn test_strip_common_affixes_strips_prefix_suffix_or_both() {
  let suffix_variants = vec![make_variant("CreateResponse"), make_variant("UpdateResponse")];
  let suffix_variants = strip_common_affixes(suffix_variants);
  assert_eq!(suffix_variants[0].name, EnumVariantToken::new("Create"));
  assert_eq!(suffix_variants[1].name, EnumVariantToken::new("Update"));

  let prefix_variants = vec![make_variant("UserCreate"), make_variant("UserUpdate")];
  let prefix_variants = strip_common_affixes(prefix_variants);
  assert_eq!(prefix_variants[0].name, EnumVariantToken::new("Create"));
  assert_eq!(prefix_variants[1].name, EnumVariantToken::new("Update"));

  let both_variants = vec![make_variant("UserCreateResponse"), make_variant("UserUpdateResponse")];
  let both_variants = strip_common_affixes(both_variants);
  assert_eq!(both_variants[0].name, EnumVariantToken::new("Create"));
  assert_eq!(both_variants[1].name, EnumVariantToken::new("Update"));
}

#[test]
fn test_strip_common_affixes_no_common_parts() {
  let variants = vec![make_variant("CreateUser"), make_variant("DeletePost")];
  let variants = strip_common_affixes(variants);
  assert_eq!(variants[0].name, EnumVariantToken::new("CreateUser"));
  assert_eq!(variants[1].name, EnumVariantToken::new("DeletePost"));
}

#[test]
fn test_strip_common_affixes_safety_guards() {
  let collision_variants = vec![make_variant("UserResponse"), make_variant("UserResponse")];
  let collision_variants = strip_common_affixes(collision_variants);
  assert_eq!(collision_variants[0].name, EnumVariantToken::new("UserResponse"));
  assert_eq!(collision_variants[1].name, EnumVariantToken::new("UserResponse"));

  let empty_name_variants = vec![make_variant("Response"), make_variant("Response")];
  let empty_name_variants = strip_common_affixes(empty_name_variants);
  assert_eq!(empty_name_variants[0].name, EnumVariantToken::new("Response"));
  assert_eq!(empty_name_variants[1].name, EnumVariantToken::new("Response"));
}

#[test]
fn test_strip_common_affixes_preserves_variant_content() {
  let tuple_type = TypeRef::new("TestStruct");
  let variants = vec![
    VariantDef::builder()
      .name(EnumVariantToken::from("CreateResponse"))
      .content(VariantContent::Tuple(vec![tuple_type]))
      .build(),
    make_variant("UpdateResponse"),
  ];

  let variants = strip_common_affixes(variants);

  assert_eq!(variants[0].name, EnumVariantToken::new("Create"));
  assert_eq!(variants[1].name, EnumVariantToken::new("Update"));
  assert!(matches!(variants[0].content, VariantContent::Tuple(_)));
  assert!(matches!(variants[1].content, VariantContent::Unit));
}

#[test]
fn schema_ext_is_primitive() {
  let mut schema = ObjectSchema {
    schema_type: Some(SchemaTypeSet::Single(SchemaType::String)),
    ..ObjectSchema::default()
  };
  assert!(schema.is_primitive(), "string without properties should be primitive");

  schema
    .properties
    .insert("foo".to_string(), ObjectOrReference::Object(ObjectSchema::default()));
  assert!(!schema.is_primitive(), "schema with properties should not be primitive");

  let string_enum_schema = ObjectSchema {
    schema_type: Some(SchemaTypeSet::Single(SchemaType::String)),
    enum_values: vec![json!("auto"), json!("none")],
    ..ObjectSchema::default()
  };
  assert!(
    !string_enum_schema.is_primitive(),
    "string enum with 2+ values should NOT be primitive"
  );

  let single_enum_schema = ObjectSchema {
    schema_type: Some(SchemaTypeSet::Single(SchemaType::String)),
    enum_values: vec![json!("only_value")],
    ..ObjectSchema::default()
  };
  assert!(
    single_enum_schema.is_primitive(),
    "string enum with 1 value should be primitive (const-like)"
  );
}

#[test]
fn schema_ext_is_nullable_object() {
  let null_only = ObjectSchema {
    schema_type: Some(SchemaTypeSet::Single(SchemaType::Null)),
    ..ObjectSchema::default()
  };
  assert!(null_only.is_nullable_object(), "null-only schema should be nullable");

  let nullable_object = ObjectSchema {
    schema_type: Some(SchemaTypeSet::Multiple(vec![SchemaType::Object, SchemaType::Null])),
    ..ObjectSchema::default()
  };
  assert!(
    nullable_object.is_nullable_object(),
    "object|null without properties should be nullable"
  );

  let mut nullable_with_props = nullable_object.clone();
  nullable_with_props
    .properties
    .insert("x".into(), ObjectOrReference::Object(ObjectSchema::default()));
  assert!(
    !nullable_with_props.is_nullable_object(),
    "object|null with properties should not be nullable"
  );
}

#[test]
fn schema_ext_has_inline_union_array_items() {
  use crate::tests::common::create_test_graph;

  let type_a = ObjectSchema {
    schema_type: Some(SchemaTypeSet::Single(SchemaType::Object)),
    properties: [("field_a".to_string(), ObjectOrReference::Object(make_string_schema()))]
      .into_iter()
      .collect(),
    ..Default::default()
  };
  let type_b = ObjectSchema {
    schema_type: Some(SchemaTypeSet::Single(SchemaType::Object)),
    properties: [("field_b".to_string(), ObjectOrReference::Object(make_string_schema()))]
      .into_iter()
      .collect(),
    ..Default::default()
  };

  let graph = create_test_graph(BTreeMap::from([
    ("TypeA".to_string(), type_a),
    ("TypeB".to_string(), type_b),
  ]));

  let array_with_union_items = ObjectSchema {
    schema_type: Some(SchemaTypeSet::Single(SchemaType::Array)),
    items: Some(Box::new(Schema::Object(Box::new(ObjectOrReference::Object(
      ObjectSchema {
        one_of: vec![
          ObjectOrReference::Ref {
            ref_path: "#/components/schemas/TypeA".to_string(),
            summary: None,
            description: None,
          },
          ObjectOrReference::Ref {
            ref_path: "#/components/schemas/TypeB".to_string(),
            summary: None,
            description: None,
          },
        ],
        ..Default::default()
      },
    ))))),
    ..Default::default()
  };

  assert!(
    array_with_union_items.has_inline_union_array_items(graph.spec()),
    "array with oneOf items should have union items"
  );

  let array_with_string_items = ObjectSchema {
    schema_type: Some(SchemaTypeSet::Single(SchemaType::Array)),
    items: Some(Box::new(Schema::Object(Box::new(ObjectOrReference::Object(
      make_string_schema(),
    ))))),
    ..Default::default()
  };

  assert!(
    !array_with_string_items.has_inline_union_array_items(graph.spec()),
    "array with string items should not have union items"
  );

  let array_with_ref_items = ObjectSchema {
    schema_type: Some(SchemaTypeSet::Single(SchemaType::Array)),
    items: Some(Box::new(Schema::Object(Box::new(ObjectOrReference::Ref {
      ref_path: "#/components/schemas/TypeA".to_string(),
      summary: None,
      description: None,
    })))),
    ..Default::default()
  };

  assert!(
    !array_with_ref_items.has_inline_union_array_items(graph.spec()),
    "array with ref items should not have inline union items"
  );

  let array_without_items = ObjectSchema {
    schema_type: Some(SchemaTypeSet::Single(SchemaType::Array)),
    items: None,
    ..Default::default()
  };

  assert!(
    !array_without_items.has_inline_union_array_items(graph.spec()),
    "array without items should not have union items"
  );
}

#[test]
fn extract_common_variant_prefix_cases() {
  use crate::generator::naming::inference::extract_common_variant_prefix;

  struct Case {
    variants: Vec<&'static str>,
    expected: Option<&'static str>,
    description: &'static str,
  }

  let cases = [
    Case {
      variants: vec![
        "BetaResponseCharLocationCitation",
        "BetaResponseUrlCitation",
        "BetaResponseFileCitation",
      ],
      expected: Some("BetaCitation"),
      description: "first prefix (Beta) + suffix (Citation) - terse naming",
    },
    Case {
      variants: vec!["ContentBlockStart", "ContentBlockDelta", "ContentBlockStop"],
      expected: Some("ContentBlock"),
      description: "full common prefix (ContentBlock), no suffix",
    },
    Case {
      variants: vec!["Tool", "BashTool"],
      expected: None,
      description: "no common prefix (Tool vs Bash) - returns None",
    },
    Case {
      variants: vec!["TypeA", "TypeB", "TypeC"],
      expected: Some("Type"),
      description: "common prefix (Type) only, no suffix",
    },
    Case {
      variants: vec!["AlphaFoo", "BetaBar", "GammaQux"],
      expected: None,
      description: "no common prefix - returns None",
    },
    Case {
      variants: vec!["RequestBody"],
      expected: None,
      description: "single variant - returns None",
    },
    Case {
      variants: vec![],
      expected: None,
      description: "empty variants - returns None",
    },
    Case {
      variants: vec!["ApiErrorNotFound", "ApiErrorBadRequest", "ApiErrorUnauthorized"],
      expected: Some("ApiError"),
      description: "full common prefix (ApiError), no suffix",
    },
    Case {
      variants: vec!["BetaMessageStartEvent", "BetaMessageDeltaEvent", "BetaMessageStopEvent"],
      expected: Some("BetaEvent"),
      description: "first prefix (Beta) + suffix (Event) - terse naming",
    },
    Case {
      variants: vec!["FooBarBaz", "FooQuxBaz"],
      expected: Some("FooBaz"),
      description: "first prefix (Foo) + suffix (Baz) - terse naming",
    },
    Case {
      variants: vec!["StreamEventStart", "StreamEventData", "StreamEventEnd"],
      expected: Some("StreamEvent"),
      description: "full common prefix (StreamEvent), no suffix",
    },
  ];

  for case in cases {
    let variants = case
      .variants
      .iter()
      .map(|name| ObjectOrReference::Ref {
        ref_path: format!("#/components/schemas/{name}"),
        summary: None,
        description: None,
      })
      .collect::<Vec<ObjectOrReference<ObjectSchema>>>();

    let result = extract_common_variant_prefix(&variants);

    assert_eq!(
      result.as_ref().map(|r| r.name.as_str()),
      case.expected,
      "{}: expected {:?}, got {:?}",
      case.description,
      case.expected,
      result.as_ref().map(|r| r.name.as_str())
    );
  }
}

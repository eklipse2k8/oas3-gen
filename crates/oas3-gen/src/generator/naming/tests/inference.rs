use std::collections::{BTreeMap, BTreeSet};

use oas3::spec::{ObjectOrReference, ObjectSchema, SchemaType, SchemaTypeSet};
use serde_json::{Value, json};

use crate::{
  generator::{
    ast::{TypeRef, VariantContent, VariantDef},
    naming::{
      constants::REQUEST_BODY_SUFFIX,
      inference::{
        InlineTypeScanner, VariantNameNormalizer, derive_method_names, infer_name_from_context, infer_variant_name,
        strip_common_affixes,
      },
    },
  },
  tests::common::create_test_graph,
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
    let variants_vec: Vec<String> = variants.into_iter().map(String::from).collect();
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
    let input_strings: Vec<String> = inputs.into_iter().map(String::from).collect();
    let refs: Vec<&String> = input_strings.iter().collect();
    assert_eq!(
      InlineTypeScanner::longest_common_suffix(&refs),
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
    assert_eq!(
      InlineTypeScanner::is_valid_common_name(input),
      expected,
      "Failed for input '{input}'"
    );
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
    let candidates: BTreeSet<(String, bool)> = candidate_pairs.iter().map(|(s, b)| ((*s).to_string(), *b)).collect();
    let used: BTreeSet<String> = used_list.into_iter().map(String::from).collect();
    assert_eq!(
      InlineTypeScanner::compute_best_name(&candidates, &used),
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

  let graph = create_test_graph(BTreeMap::from([
    ("VoiceIdsShared".to_string(), voice_ids_shared),
    ("FormatType".to_string(), format_type),
  ]));

  let scanner = InlineTypeScanner::new(&graph);
  let result = scanner.scan_and_compute_names().expect("Should scan successfully");

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

  let graph = create_test_graph(BTreeMap::from([
    ("Status".to_string(), status),
    ("ChatModel".to_string(), chat_model),
    ("ModelIdsShared".to_string(), model_ids_shared),
  ]));

  let scanner = InlineTypeScanner::new(&graph);
  let result = scanner.scan_and_compute_names().expect("Should scan successfully");

  let status_values = vec!["active".to_string(), "inactive".to_string(), "pending".to_string()];
  let status_name = result.enum_names.get(&status_values);
  assert!(status_name.is_some(), "Regular enum should have a precomputed name");
  assert!(
    !status_name.unwrap().ends_with("Known"),
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
    let result = infer_name_from_context(&schema, path, status);
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
  let result = infer_name_from_context(&schema_with_property, "/api/check-access", "200");
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
    let res = VariantNameNormalizer::normalize(&val).unwrap();
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
    let res = VariantNameNormalizer::normalize(&val).unwrap();
    assert_eq!(res.name, expected_name, "name mismatch for {val:?}");
    assert_eq!(res.rename_value, expected_rename, "rename mismatch for {val:?}");
  }
}

#[test]
fn test_normalize_booleans_and_unsupported_types() {
  let true_res = VariantNameNormalizer::normalize(&json!(true)).unwrap();
  assert_eq!(true_res.name, "True");
  assert_eq!(true_res.rename_value, "true");

  let false_res = VariantNameNormalizer::normalize(&json!(false)).unwrap();
  assert_eq!(false_res.name, "False");
  assert_eq!(false_res.rename_value, "false");

  assert!(VariantNameNormalizer::normalize(&json!(null)).is_none());
  assert!(VariantNameNormalizer::normalize(&json!({"key": "value"})).is_none());
  assert!(VariantNameNormalizer::normalize(&json!([1, 2, 3])).is_none());
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
      infer_variant_name(&schema, 0),
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
  assert_eq!(infer_variant_name(&enum_schema, 0), "Enum");

  let multi_type_schema = ObjectSchema {
    schema_type: Some(SchemaTypeSet::Multiple(vec![SchemaType::String, SchemaType::Number])),
    ..Default::default()
  };
  assert_eq!(infer_variant_name(&multi_type_schema, 0), "Mixed");

  let no_type_schema = ObjectSchema::default();
  assert_eq!(infer_variant_name(&no_type_schema, 5), "Variant5");
}

fn make_variant(name: &str) -> VariantDef {
  VariantDef {
    name: name.into(),
    docs: vec![],
    content: VariantContent::Unit,
    serde_attrs: vec![],
    deprecated: false,
  }
}

#[test]
fn test_strip_common_affixes_no_op_cases() {
  let mut empty: Vec<VariantDef> = vec![];
  strip_common_affixes(&mut empty);
  assert!(empty.is_empty());

  let mut single = vec![make_variant("UserResponse")];
  strip_common_affixes(&mut single);
  assert_eq!(single[0].name, "UserResponse");
}

#[test]
fn test_strip_common_affixes_strips_prefix_suffix_or_both() {
  let mut suffix_variants = vec![make_variant("CreateResponse"), make_variant("UpdateResponse")];
  strip_common_affixes(&mut suffix_variants);
  assert_eq!(suffix_variants[0].name, "Create");
  assert_eq!(suffix_variants[1].name, "Update");

  let mut prefix_variants = vec![make_variant("UserCreate"), make_variant("UserUpdate")];
  strip_common_affixes(&mut prefix_variants);
  assert_eq!(prefix_variants[0].name, "Create");
  assert_eq!(prefix_variants[1].name, "Update");

  let mut both_variants = vec![make_variant("UserCreateResponse"), make_variant("UserUpdateResponse")];
  strip_common_affixes(&mut both_variants);
  assert_eq!(both_variants[0].name, "Create");
  assert_eq!(both_variants[1].name, "Update");
}

#[test]
fn test_strip_common_affixes_no_common_parts() {
  let mut variants = vec![make_variant("CreateUser"), make_variant("DeletePost")];
  strip_common_affixes(&mut variants);
  assert_eq!(variants[0].name, "CreateUser");
  assert_eq!(variants[1].name, "DeletePost");
}

#[test]
fn test_strip_common_affixes_safety_guards() {
  let mut collision_variants = vec![make_variant("UserResponse"), make_variant("UserResponse")];
  strip_common_affixes(&mut collision_variants);
  assert_eq!(collision_variants[0].name, "UserResponse");
  assert_eq!(collision_variants[1].name, "UserResponse");

  let mut empty_name_variants = vec![make_variant("Response"), make_variant("Response")];
  strip_common_affixes(&mut empty_name_variants);
  assert_eq!(empty_name_variants[0].name, "Response");
  assert_eq!(empty_name_variants[1].name, "Response");
}

#[test]
fn test_strip_common_affixes_preserves_variant_content() {
  let tuple_type = TypeRef::new("TestStruct");
  let mut variants = vec![
    VariantDef {
      name: "CreateResponse".into(),
      docs: vec![],
      content: VariantContent::Tuple(vec![tuple_type]),
      serde_attrs: vec![],
      deprecated: false,
    },
    make_variant("UpdateResponse"),
  ];

  strip_common_affixes(&mut variants);

  assert_eq!(variants[0].name, "Create");
  assert_eq!(variants[1].name, "Update");
  assert!(matches!(variants[0].content, VariantContent::Tuple(_)));
  assert!(matches!(variants[1].content, VariantContent::Unit));
}

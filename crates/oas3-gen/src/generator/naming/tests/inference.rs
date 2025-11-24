use std::collections::{BTreeMap, BTreeSet};

use oas3::spec::{ObjectOrReference, ObjectSchema, SchemaType, SchemaTypeSet};
use serde_json::json;

use crate::{
  generator::{
    converter::constants::REQUEST_BODY_SUFFIX,
    naming::inference::{
      InlineTypeScanner, derive_method_names, ensure_unique, infer_name_from_context, split_pascal_case,
    },
  },
  tests::common::create_test_graph,
};

#[test]
fn test_ensure_unique() {
  let cases = vec![
    (vec!["UserResponse"], "UserResponse", "UserResponse2"),
    (
      vec!["UserResponse", "UserResponse2", "UserResponse3"],
      "UserResponse",
      "UserResponse4",
    ),
    (vec![], "", ""),
    (vec!["Name2"], "Name", "Name"), // Base name free, suffix collision irrelevant
    (vec![], "UniqueName", "UniqueName"),
    (vec!["Value", "Value3"], "Value", "Value2"),
  ];

  for (used_list, input, expected) in cases {
    let used: BTreeSet<String> = used_list.into_iter().map(String::from).collect();
    assert_eq!(
      ensure_unique(input, &used),
      expected,
      "Failed for input '{input}' with used {used:?}"
    );
  }
}

#[test]
fn test_split_pascal_case() {
  let cases = vec![
    ("UserName", vec!["User", "Name"]),
    ("SimpleTest", vec!["Simple", "Test"]),
    ("HTTPSConnection", vec!["HTTPS", "Connection"]),
    ("XMLParser", vec!["XML", "Parser"]),
    ("JSONResponse", vec!["JSON", "Response"]),
    ("HTTPStatus", vec!["HTTP", "Status"]),
    ("HTTPS", vec!["HTTPS"]),
    ("XML", vec!["XML"]),
    ("User", vec!["User"]),
    ("Status", vec!["Status"]),
    ("", vec![]),
  ];

  for (input, expected) in cases {
    assert_eq!(split_pascal_case(input), expected, "Failed for input '{input}'");
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
    ("Typ", false),     // Too short
    ("abc", false),     // Lowercase
    ("123Type", false), // Not PascalCase start
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
fn test_compute_best_name() {
  // Scenario 1: Single candidate
  let candidates1: BTreeSet<(String, bool)> = [("SingleCandidate", false)]
    .iter()
    .map(|(s, b)| ((*s).to_string(), *b))
    .collect();
  let used1 = BTreeSet::new();
  assert_eq!(
    InlineTypeScanner::compute_best_name(&candidates1, &used1),
    "SingleCandidate"
  );

  // Scenario 2: Single candidate, collision
  let candidates2: BTreeSet<(String, bool)> = [("Collision", false)]
    .iter()
    .map(|(s, b)| ((*s).to_string(), *b))
    .collect();
  let mut used2 = BTreeSet::new();
  used2.insert("Collision".to_string());
  assert_eq!(InlineTypeScanner::compute_best_name(&candidates2, &used2), "Collision2");

  // Scenario 3: Multiple candidates, one is from schema.
  let candidates3: BTreeSet<(String, bool)> = [("NewName", false), ("ExistingName", true)]
    .iter()
    .map(|(s, b)| ((*s).to_string(), *b))
    .collect();
  let mut used3 = BTreeSet::new();
  used3.insert("ExistingName".to_string());
  // Logic: prioritize candidate that is from schema.
  assert_eq!(
    InlineTypeScanner::compute_best_name(&candidates3, &used3),
    "ExistingName"
  );

  // Scenario 4: Multiple candidates, none used. LCS valid.
  let candidates4: BTreeSet<(String, bool)> = [("NetUser", false), ("WebUser", false)]
    .iter()
    .map(|(s, b)| ((*s).to_string(), *b))
    .collect();
  let used4 = BTreeSet::new();
  // LCS is "User". Valid.
  assert_eq!(InlineTypeScanner::compute_best_name(&candidates4, &used4), "User");

  // Scenario 5: Multiple candidates, LCS invalid (too short). Fallback to first.
  let candidates5: BTreeSet<(String, bool)> = [("Cat", false), ("Bat", false)]
    .iter()
    .map(|(s, b)| ((*s).to_string(), *b))
    .collect();
  // LCS "at" -> invalid. Sorted: "Bat", "Cat". First is "Bat".
  let used5 = BTreeSet::new();
  assert_eq!(InlineTypeScanner::compute_best_name(&candidates5, &used5), "Bat");

  // Scenario 6: Multiple candidates, LCS valid but used (and not in candidates).
  let candidates6: BTreeSet<(String, bool)> = [("MyGroup", false), ("YourGroup", false)]
    .iter()
    .map(|(s, b)| ((*s).to_string(), *b))
    .collect();
  // LCS "Group".
  let mut used6 = BTreeSet::new();
  used6.insert("Group".to_string());
  // LCS "Group" is in used, and "Group" is NOT in candidates.
  // ensure_unique("Group") -> "Group2".
  assert_eq!(InlineTypeScanner::compute_best_name(&candidates6, &used6), "Group2");
}

#[test]
fn test_inline_type_scanner_names_string_enum_optimizer_pattern() {
  let voice_ids_shared = ObjectSchema {
    any_of: vec![
      ObjectOrReference::Object(ObjectSchema {
        schema_type: Some(SchemaTypeSet::Single(SchemaType::String)),
        ..Default::default()
      }),
      ObjectOrReference::Object(ObjectSchema {
        schema_type: Some(SchemaTypeSet::Single(SchemaType::String)),
        enum_values: vec![json!("alloy"), json!("ash"), json!("ballad")],
        ..Default::default()
      }),
    ],
    ..Default::default()
  };

  let graph = create_test_graph(BTreeMap::from([("VoiceIdsShared".to_string(), voice_ids_shared)]));

  let scanner = InlineTypeScanner::new(&graph);
  let result = scanner.scan_and_compute_names().expect("Should scan successfully");

  let enum_values = vec!["alloy".to_string(), "ash".to_string(), "ballad".to_string()];
  let name = result.enum_names.get(&enum_values);

  assert_eq!(
    name,
    Some(&"VoiceIdsSharedKnown".to_string()),
    "StringEnumOptimizer pattern should be named with 'Known' suffix"
  );
}

#[test]
fn test_inline_type_scanner_names_regular_enum_without_known_suffix() {
  let status = ObjectSchema {
    schema_type: Some(SchemaTypeSet::Single(SchemaType::String)),
    enum_values: vec![json!("active"), json!("inactive"), json!("pending")],
    ..Default::default()
  };

  let graph = create_test_graph(BTreeMap::from([("Status".to_string(), status)]));

  let scanner = InlineTypeScanner::new(&graph);
  let result = scanner.scan_and_compute_names().expect("Should scan successfully");

  let enum_values = vec!["active".to_string(), "inactive".to_string(), "pending".to_string()];
  let name = result.enum_names.get(&enum_values);

  assert!(name.is_some(), "Regular enum should have a precomputed name");
  assert!(
    !name.unwrap().ends_with("Known"),
    "Regular enum should not have 'Known' suffix"
  );
}

#[test]
fn test_inline_type_scanner_handles_anyof_with_ref() {
  let chat_model = ObjectSchema {
    schema_type: Some(SchemaTypeSet::Single(SchemaType::String)),
    enum_values: vec![json!("gpt-4"), json!("gpt-3.5-turbo")],
    ..Default::default()
  };

  let model_ids_shared = ObjectSchema {
    any_of: vec![
      ObjectOrReference::Object(ObjectSchema {
        schema_type: Some(SchemaTypeSet::Single(SchemaType::String)),
        ..Default::default()
      }),
      ObjectOrReference::Ref {
        ref_path: "#/components/schemas/ChatModel".to_string(),
        summary: None,
        description: None,
      },
    ],
    ..Default::default()
  };

  let graph = create_test_graph(BTreeMap::from([
    ("ChatModel".to_string(), chat_model),
    ("ModelIdsShared".to_string(), model_ids_shared),
  ]));

  let scanner = InlineTypeScanner::new(&graph);
  let result = scanner.scan_and_compute_names().expect("Should scan successfully");

  let chat_model_values = vec!["gpt-3.5-turbo".to_string(), "gpt-4".to_string()];
  let chat_model_name = result.enum_names.get(&chat_model_values);

  assert!(
    chat_model_name.is_some(),
    "Top-level enum values should have a precomputed name"
  );
  assert!(
    chat_model_name.unwrap().starts_with("ChatModel"),
    "Precomputed name should be based on ChatModel"
  );
}

#[test]
fn test_inline_type_scanner_anyof_with_const_values() {
  let format_type = ObjectSchema {
    any_of: vec![
      ObjectOrReference::Object(ObjectSchema {
        schema_type: Some(SchemaTypeSet::Single(SchemaType::String)),
        ..Default::default()
      }),
      ObjectOrReference::Object(ObjectSchema {
        const_value: Some(json!("json")),
        ..Default::default()
      }),
      ObjectOrReference::Object(ObjectSchema {
        const_value: Some(json!("text")),
        ..Default::default()
      }),
      ObjectOrReference::Object(ObjectSchema {
        const_value: Some(json!("xml")),
        ..Default::default()
      }),
    ],
    ..Default::default()
  };

  let graph = create_test_graph(BTreeMap::from([("FormatType".to_string(), format_type)]));

  let scanner = InlineTypeScanner::new(&graph);
  let result = scanner.scan_and_compute_names().expect("Should scan successfully");

  let enum_values = vec!["json".to_string(), "text".to_string(), "xml".to_string()];
  let name = result.enum_names.get(&enum_values);

  assert_eq!(
    name,
    Some(&"FormatTypeKnown".to_string()),
    "anyOf with const values should be named with 'Known' suffix"
  );
}

#[test]
fn test_infer_name_from_context_sanitizes_hyphens() {
  let schema = ObjectSchema::default();

  let result = infer_name_from_context(&schema, "/api/check-access-by-email", "200");

  assert_eq!(result, "check_access_by_email200Response");
  assert!(!result.contains('-'), "Result should not contain hyphens: {result}");
}

#[test]
fn test_infer_name_from_context_sanitizes_multiple_separators() {
  let schema = ObjectSchema::default();

  let result = infer_name_from_context(&schema, "/api/foo-bar.baz_qux", "201");

  assert_eq!(result, "foo_bar_baz_qux201Response");
  assert!(
    !result.contains('-') && !result.contains('.'),
    "Result should not contain hyphens or dots: {result}"
  );
}

#[test]
fn test_infer_name_from_context_with_request_body() {
  let schema = ObjectSchema::default();

  let result = infer_name_from_context(&schema, "/api/create-user", REQUEST_BODY_SUFFIX);

  assert_eq!(result, "create_userRequestBody");
  assert!(!result.contains('-'), "Result should not contain hyphens: {result}");
}

#[test]
fn test_infer_name_from_context_single_property_response() {
  let mut schema = ObjectSchema::default();
  schema
    .properties
    .insert("user".to_string(), ObjectOrReference::Object(ObjectSchema::default()));

  let result = infer_name_from_context(&schema, "/api/check-access", "200");

  assert_eq!(result, "userResponse");
  assert!(!result.contains('-'), "Result should not contain hyphens: {result}");
}

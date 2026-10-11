use oas3::spec::{ObjectOrReference, ObjectSchema};
use oas3_gen::{
  generator::{
    ast::{EnumVariantToken, TypeRef, VariantContent, VariantDef},
    naming::{
      constants::REQUEST_BODY_SUFFIX,
      inference::{NormalizedVariant, derive_method_names, extract_common_variant_prefix, strip_common_affixes},
    },
  },
  utils::SchemaExt as _,
};
use rstest::rstest;
use serde_json::{Value, json};

use super::assert_converts;
use crate::parse_schema;

fn schema_refs(names: &[&str]) -> Vec<ObjectOrReference<ObjectSchema>> {
  names
    .iter()
    .map(|name| ObjectOrReference::Ref {
      ref_path: format!("#/components/schemas/{name}"),
      summary: None,
      description: None,
    })
    .collect()
}

fn variant(name: &str, content: VariantContent) -> VariantDef {
  VariantDef::builder()
    .name(EnumVariantToken::from(name))
    .content(content)
    .build()
}

fn stripped_names(names: &[&str]) -> Vec<String> {
  strip_common_affixes(names.iter().map(|name| variant(name, VariantContent::Unit)).collect())
    .into_iter()
    .map(|variant| variant.name.to_string())
    .collect()
}

#[rstest]
fn enum_method_names_drop_words_shared_with_enum_name() {
  assert_converts(
    |(enum_name, variants): (&str, &[&str])| derive_method_names(enum_name, variants),
    &[
      (
        ("RequestType", &["GetRequest", "PostRequest"][..]),
        &["get", "post"][..],
      ),
      (
        (
          "ResponseFormatOption",
          &["ResponseFormatText", "ResponseFormatJson", "ResponseFormatXml"],
        ),
        &["text", "json", "xml"],
      ),
      (
        ("Status", &["StatusActive", "Active", "StatusInactive"]),
        &["status_active", "active", "inactive"],
      ),
      (("Status", &[]), &[]),
    ],
  );
}

#[rstest]
#[expect(
  clippy::approx_constant,
  reason = "3.14 is an enum value, not an approximation of pi"
)]
fn enum_variant_names_come_from_values_and_schema_shapes() {
  for (value, expected) in [
    (json!("active"), Some(("Active", "active"))),
    (json!("pending_approval"), Some(("PendingApproval", "pending_approval"))),
    (json!("pending-approval"), Some(("PendingApproval", "pending-approval"))),
    (json!(404), Some(("Value404", "404"))),
    (json!(-42), Some(("Value_42", "-42"))),
    (json!(0), Some(("Value0", "0"))),
    (json!(3.14), Some(("Value3_14", "3.14"))),
    (json!(-2.5), Some(("Value_2_5", "-2.5"))),
    (json!(true), Some(("True", "true"))),
    (json!(false), Some(("False", "false"))),
    (json!(null), None),
    (json!({ "key": "value" }), None),
    (json!([1, 2, 3]), None),
  ] {
    let normalized = NormalizedVariant::try_from(&value).ok();
    assert_eq!(
      normalized
        .as_ref()
        .map(|variant| (variant.name.as_str(), variant.rename_value.as_str())),
      expected,
      "variant for {value}"
    );
  }

  assert_converts(
    |(schema, index): (&Value, usize)| parse_schema(schema.clone()).infer_variant_name(index),
    &[
      ((&json!({ "type": "string" }), 0), "String"),
      ((&json!({ "type": "number" }), 0), "Number"),
      ((&json!({ "type": "integer" }), 0), "Integer"),
      ((&json!({ "type": "boolean" }), 0), "Boolean"),
      ((&json!({ "type": "array" }), 0), "Array"),
      ((&json!({ "type": "null" }), 0), "Null"),
      ((&json!({ "type": ["string", "null"] }), 0), "String"),
      ((&json!({ "type": ["string", "number"] }), 0), "Mixed"),
      ((&json!({ "enum": ["a", "b"] }), 0), "Enum"),
      ((&json!({}), 5), "Variant5"),
      ((&json!({ "type": "object" }), 0), "Object"),
      (
        (&json!({ "type": "object", "properties": { "items": {} } }), 0),
        "Items",
      ),
      (
        (
          &json!({ "type": "object", "properties": { "id": {}, "name": {} }, "required": ["id"] }),
          0,
        ),
        "Id",
      ),
      (
        (&json!({ "type": "object", "properties": { "id": {}, "name": {} } }), 0),
        "Object",
      ),
      (
        (
          &json!({ "oneOf": [{ "$ref": "#/components/schemas/StreamEventStart" }, { "$ref": "#/components/schemas/StreamEventStop" }] }),
          0,
        ),
        "StreamEvent",
      ),
    ],
  );
}

#[rstest]
fn union_names_keep_what_variants_do_not_share() {
  assert_converts(
    |names: &[&str]| extract_common_variant_prefix(&schema_refs(names)).map_or_else(String::new, |common| common.name),
    &[
      (
        &[
          "BetaResponseCharLocationCitation",
          "BetaResponseUrlCitation",
          "BetaResponseFileCitation",
        ][..],
        "BetaCitation",
      ),
      (
        &["BetaMessageStartEvent", "BetaMessageDeltaEvent", "BetaMessageStopEvent"],
        "BetaEvent",
      ),
      (&["FooBarBaz", "FooQuxBaz"], "FooBaz"),
      (
        &["ContentBlockStart", "ContentBlockDelta", "ContentBlockStop"],
        "ContentBlock",
      ),
      (
        &["ApiErrorNotFound", "ApiErrorBadRequest", "ApiErrorUnauthorized"],
        "ApiError",
      ),
      (&["TypeA", "TypeB", "TypeC"], "Type"),
      (&["Tool", "BashTool"], ""),
      (&["AlphaFoo", "BetaBar", "GammaQux"], ""),
      (&["RequestBody"], ""),
      (&[], ""),
    ],
  );

  assert_converts(
    stripped_names,
    &[
      (&["CreateResponse", "UpdateResponse"][..], &["Create", "Update"][..]),
      (&["UserCreate", "UserUpdate"], &["Create", "Update"]),
      (&["UserCreateResponse", "UserUpdateResponse"], &["Create", "Update"]),
      (&["CreateUser", "DeletePost"], &["CreateUser", "DeletePost"]),
      (&["UserResponse", "UserResponse"], &["UserResponse", "UserResponse"]),
      (&["Response", "Response"], &["Response", "Response"]),
      (&["UserResponse"], &["UserResponse"]),
      (&[], &[]),
    ],
  );

  let stripped = strip_common_affixes(vec![
    variant(
      "CreateResponse",
      VariantContent::Tuple(vec![TypeRef::new("TestStruct")]),
    ),
    variant("UpdateResponse", VariantContent::Unit),
  ]);
  assert_eq!(
    stripped,
    vec![
      variant("Create", VariantContent::Tuple(vec![TypeRef::new("TestStruct")])),
      variant("Update", VariantContent::Unit),
    ]
  );
}

#[rstest]
fn inline_body_names_come_from_title_property_or_path() {
  assert_converts(
    |(schema, path, context): (&Value, &str, &str)| parse_schema(schema.clone()).infer_name_from_context(path, context),
    &[
      (
        (&json!({}), "/api/check-access-by-email", "200"),
        "check_access_by_email200Response",
      ),
      (
        (&json!({}), "/api/foo-bar.baz_qux", "201"),
        "foo_bar_baz_qux201Response",
      ),
      ((&json!({}), "/pets/{petId}", "404"), "pet404Response"),
      ((&json!({}), "/", "200"), "Response200"),
      (
        (&json!({}), "/api/create-user", REQUEST_BODY_SUFFIX),
        "create_userRequestBody",
      ),
      ((&json!({}), "/", REQUEST_BODY_SUFFIX), "RequestBody"),
      (
        (&json!({ "properties": { "user": {} } }), "/api/check-access", "200"),
        "userResponse",
      ),
      (
        (&json!({ "properties": { "pets": {} } }), "/api/check-access", "200"),
        "petResponse",
      ),
      (
        (&json!({ "title": "Pet Summary" }), "/pets", "200"),
        "Pet_SummaryResponse",
      ),
    ],
  );
}

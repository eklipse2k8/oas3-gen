use oas3_gen::generator::ast::{Documentation, RustPrimitive, TypeRef};
use quote::{ToTokens as _, quote};
use rstest::rstest;
use serde_json::json;

#[rstest]
#[expect(
  clippy::approx_constant,
  reason = "3.14 is an example value, not an approximation of pi"
)]
fn descriptions_and_examples_render_as_doc_text() {
  let description = "Floof 1\\nFloof 2".to_string();
  assert_eq!(
    Documentation::from_optional(Some(&description))
      .to_token_stream()
      .to_string(),
    quote! {
      #[doc = " Floof 1"]
      #[doc = " Floof 2"]
    }
    .to_string()
  );

  let string = || TypeRef::new(RustPrimitive::String);
  for (type_ref, example, expected) in [
    (string().with_option(), json!(null), "None"),
    (string(), json!(null), ""),
    (string(), json!("hello"), "\"hello\""),
    (
      string(),
      json!("say \"hi\"\n\\path\tend"),
      "\"say \\\"hi\\\"\\n\\\\path\\tend\"",
    ),
    (string(), json!(true), "\"true\""),
    (string(), json!(42), "\"42\""),
    (string().with_option(), json!(2.2), "\"2.2\""),
    (string().with_boxed(), json!("boxed"), "Box::new(\"boxed\")"),
    (TypeRef::new(RustPrimitive::Bool), json!(false), "false"),
    (TypeRef::new(RustPrimitive::I32), json!(42), "42i32"),
    (TypeRef::new(RustPrimitive::F64), json!(3.14), "3.14"),
    (TypeRef::new(RustPrimitive::F64), json!(1e-6), "1e-6"),
    (TypeRef::new(RustPrimitive::F64), json!(2.5e10), "25000000000.0"),
    (string().with_vec(), json!(["foo", "bar"]), "vec![\"foo\", \"bar\"]"),
    (string().with_vec(), json!([]), "vec![]"),
    (
      TypeRef::new(RustPrimitive::I32).with_vec(),
      json!([[1, 2], [3]]),
      "vec![vec![1i32, 2i32], vec![3i32]]",
    ),
    (
      TypeRef::new(RustPrimitive::Date).with_vec(),
      json!(["2024-01-15", "2024-02-20"]),
      "vec![chrono::NaiveDate::from_ymd_opt(2024, 1, 15)?, chrono::NaiveDate::from_ymd_opt(2024, 2, 20)?]",
    ),
    (
      TypeRef::new(RustPrimitive::Date),
      json!("2024-01"),
      "chrono::NaiveDate::parse_from_str(\"2024-01\", \"%Y-%m-%d\")?",
    ),
    (
      TypeRef::new(RustPrimitive::DateTime),
      json!("2024-01-15T10:30:00Z"),
      "chrono::DateTime::parse_from_rfc3339(\"2024-01-15T10:30:00Z\")?.with_timezone(&chrono::Utc)",
    ),
    (
      TypeRef::new(RustPrimitive::Time),
      json!("14:30"),
      "chrono::NaiveTime::from_hms_opt(14, 30, 0)?",
    ),
    (
      TypeRef::new(RustPrimitive::Time),
      json!("14:30:45.123"),
      "chrono::NaiveTime::from_hms_opt(14, 30, 45)?",
    ),
    (
      TypeRef::new(RustPrimitive::Time),
      json!("25:00:00"),
      "chrono::NaiveTime::parse_from_str(\"25:00:00\", \"%H:%M:%S\")?",
    ),
    (
      TypeRef::new(RustPrimitive::Uuid),
      json!("\"550e8400-e29b-41d4-a716-446655440000\""),
      "uuid::Uuid::parse_str(\"\\\"550e8400-e29b-41d4-a716-446655440000\\\"\")?",
    ),
  ] {
    assert_eq!(
      type_ref.format_example(&example),
      expected,
      "example {example} as {}",
      type_ref.to_rust_type()
    );
  }
}

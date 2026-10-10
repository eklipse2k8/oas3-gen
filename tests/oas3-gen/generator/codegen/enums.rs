use oas3_gen::generator::{CodegenConfig, EnumDeserializePolicy, SchemaScope, TypesMode};
use rstest::rstest;

use super::assert_snippets;

#[rstest]
#[case::listed_values_match_in_any_case(&[
  r#"match s.to_ascii_lowercase().as_str() {
    "active" => Ok(Self::Active),
    "in-progress" => Ok(Self::InProgress),
    _ => Err(serde::de::Error::unknown_variant(&s, &["active", "in-progress"])),
  }"#,
  r#"if matches!(s.to_ascii_lowercase().as_str(), "auto") => Ok(Self::Auto),"#,
  r#"if s.eq_ignore_ascii_case("true") =>"#,
])]
#[case::unknown_value_catches_unlisted_values(&[
  r#""unknown" => Ok(Self::Unknown), _ => Ok(Self::Unknown),"#,
])]
#[case::forward_compatible_enum_keeps_unlisted_text(&[
  r#"match value.to_ascii_lowercase().as_str() {
    "italian" => Self::Italian,
    "thai-food" => Self::ThaiFood,
    _ => Self::Other(value),
  }"#,
  "String::deserialize(deserializer).map(Self::from)",
])]
#[case::response_only_enum_carries_no_serde_attributes(&[
  "#[derive(Debug, Clone, PartialEq, Eq, Hash, oas3_gen_support::Default)] pub enum Mood { #[default] Calm, Excited, }",
])]
fn relaxed_enums_deserialize_case_insensitively(#[case] snippets: &[&str]) -> anyhow::Result<()> {
  let types = generated!(
    "codegen/enums.json",
    TypesMode,
    Types,
    CodegenConfig::builder()
      .schema_scope(SchemaScope::All)
      .enum_deserialize(EnumDeserializePolicy::CaseInsensitive)
      .build()
  )?;
  assert_snippets(&types, snippets);
  Ok(())
}

#[rstest]
fn single_value_bool_enum_rejects_the_other_value() -> anyhow::Result<()> {
  let types = generated!(
    "codegen/enums.json",
    TypesMode,
    Types,
    CodegenConfig::builder().schema_scope(SchemaScope::All).build()
  )?;
  assert_snippets(
    &types,
    &["let value = bool::deserialize(deserializer)?; if value { Ok(Self::True) } else { Err(serde::de::Error::custom("],
  );
  Ok(())
}

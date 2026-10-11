use rstest::rstest;

use super::convert;

#[rstest]
fn schemas_and_inline_types_get_unique_names_in_spec_order() {
  let converted = convert(spec!("converter/names.json"), config!());
  assert_eq!(
    converted.declarations(),
    [
      "enum MyTestEnum",
      "enum MyTestUnion",
      "enum AlreadyPascal",
      "struct LoafNugget",
      "struct Loaf",
      "struct BetaResponseCharLocationCitation",
      "struct BetaResponseUrlCitation",
      "struct BetaMessageStartEvent",
      "struct BetaMessageStopEvent",
      "struct TypeA",
      "struct TypeB",
      "struct Holder",
      "type Matrix = Vec<Vec<i64>>",
      "type Untyped = Vec<serde_json::Value>",
      "type NoValues = String",
      "enum NullableLevel",
      "type Mixes = Vec<MixKind>",
      "type MaybeMixes = Option<Vec<MaybeMixKind>>",
      "struct MyTestUnionDataField",
      "struct LoafNugget2",
      "enum HolderCitation",
      "enum BetaEventKind",
      "enum TypeItemKind",
      "struct HolderFluff",
      "enum MixKind",
      "enum MaybeMixKind",
    ]
  );
  assert_eq!(converted.fields("Loaf"), ["nugget: Option<LoafNugget2>"]);
  assert_eq!(
    converted.fields("Holder"),
    [
      "citation: Option<HolderCitation>",
      "events: Option<Vec<BetaEventKind>>",
      "items: Option<Vec<TypeItemKind>>",
      "fluff: Option<HolderFluff>",
    ]
  );
}

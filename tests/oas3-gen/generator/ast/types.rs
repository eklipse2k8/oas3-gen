use oas3_gen::generator::ast::{DeriveTrait, RustPrimitive};
use quote::ToTokens as _;
use rstest::rstest;

#[rstest]
fn type_and_derive_names_render_the_same_as_text_and_tokens() -> anyhow::Result<()> {
  for primitive in [
    RustPrimitive::I8,
    RustPrimitive::I16,
    RustPrimitive::I32,
    RustPrimitive::I64,
    RustPrimitive::I128,
    RustPrimitive::Isize,
    RustPrimitive::U8,
    RustPrimitive::U16,
    RustPrimitive::U32,
    RustPrimitive::U64,
    RustPrimitive::U128,
    RustPrimitive::Usize,
    RustPrimitive::F32,
    RustPrimitive::F64,
    RustPrimitive::Bool,
    RustPrimitive::String,
    RustPrimitive::StaticStr,
    RustPrimitive::Bytes,
    RustPrimitive::Date,
    RustPrimitive::DateTime,
    RustPrimitive::Time,
    RustPrimitive::Duration,
    RustPrimitive::Uuid,
    RustPrimitive::Value,
    RustPrimitive::Unit,
    RustPrimitive::Custom("indexmap::IndexMap<String, Pet>".into()),
    RustPrimitive::Custom("Pet".into()),
  ] {
    let rendered = primitive.to_string();
    if primitive != RustPrimitive::StaticStr {
      assert_eq!(rendered.parse::<RustPrimitive>()?, primitive, "parsing {rendered}");
    }
    assert_eq!(
      primitive.to_token_stream().to_string(),
      syn::parse_str::<syn::Type>(&rendered)?.to_token_stream().to_string(),
      "tokens of {rendered}"
    );
  }

  for derive in [
    DeriveTrait::Debug,
    DeriveTrait::Clone,
    DeriveTrait::PartialEq,
    DeriveTrait::Eq,
    DeriveTrait::Hash,
    DeriveTrait::Serialize,
    DeriveTrait::Deserialize,
    DeriveTrait::Validate,
    DeriveTrait::Default,
    DeriveTrait::Builder,
  ] {
    let rendered = derive.to_string();
    assert_eq!(
      derive.to_token_stream().to_string(),
      syn::parse_str::<syn::Path>(&rendered)?.to_token_stream().to_string(),
      "tokens of {rendered}"
    );
  }
  Ok(())
}

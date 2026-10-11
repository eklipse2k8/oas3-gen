use oas3_gen::generator::GenerationTarget;
use rstest::rstest;

use super::GeneratedFile;

#[rstest]
fn usage_reaches_types_nested_in_composite_maps() -> anyhow::Result<()> {
  let generated = GeneratedFile::types(spec!("postprocess/serde_usage.json"), GenerationTarget::Client)?;

  let serde_derives = ["ClassifyRequest", "OutputKind", "Classification", "InputType"]
    .into_iter()
    .map(|name| {
      let serde = generated
        .derives(name)?
        .into_iter()
        .filter(|derive| derive.ends_with("Serialize") || derive.ends_with("Deserialize"))
        .collect::<Vec<_>>();
      Ok(format!("{name}: {}", serde.join(" + ")))
    })
    .collect::<anyhow::Result<Vec<_>>>()?;
  assert_eq!(
    serde_derives,
    [
      "ClassifyRequest: Serialize",
      "OutputKind: Serialize",
      "Classification: Deserialize",
      "InputType: Deserialize",
    ]
  );
  Ok(())
}

use oas3_gen::generator::GenerationTarget;
use rstest::rstest;

use super::GeneratedFile;

#[rstest]
fn raw_binary_fields_stay_values_when_the_struct_is_shared() -> anyhow::Result<()> {
  let generated = GeneratedFile::types(spec!("postprocess/multipart_raw_binary.json"), GenerationTarget::Client)?;

  let fields = ["Upload", "Returned", "Nested", "Posted"]
    .into_iter()
    .map(|name| Ok(format!("{name} {}", generated.fields(name)?.join(", "))))
    .collect::<anyhow::Result<Vec<_>>>()?;
  assert_eq!(
    fields,
    [
      "Upload blob: Vec<u8>",
      "Returned blob: serde_json::Value",
      "Nested blob: serde_json::Value",
      "Posted blob: serde_json::Value",
    ]
  );

  let warned = generated
    .warnings
    .iter()
    .filter_map(|warning| warning.split_once(" multipart property `blob` is raw binary, but `"))
    .map(|(operation, rest)| format!("{operation} {}", rest.split('`').next().unwrap_or_default()))
    .collect::<Vec<_>>();
  assert_eq!(
    warned,
    [
      "[uploadReturned] Returned",
      "[uploadNested] Nested",
      "[uploadPosted] Posted"
    ],
    "warnings: {:?}",
    generated.warnings
  );
  Ok(())
}

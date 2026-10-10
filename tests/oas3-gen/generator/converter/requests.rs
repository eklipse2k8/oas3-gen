use rstest::rstest;

use super::convert;

#[rstest]
fn multipart_encodings_the_client_cannot_honor_warn() {
  let converted = convert(spec!("converter/operations.json"), config!());
  assert_eq!(
    converted.operation("submit_form").warnings,
    [
      "multipart property `listed` allows `image/png, image/jpeg`, and every file part is labelled `image/png`",
      "multipart part `listed` requires header `X-Signature`, which generated code does not send",
      "multipart property `wildcard` declares contentType `image/*, application/*`, which names no media type a part \
       can carry, so its parts use the default",
      "multipart property `invalid` declares contentType `not a media type`, which names no media type a part can \
       carry, so its parts use the default",
      "multipart encoding names `missing`, which is not a property of the request body",
    ]
  );
  assert!(
    converted
      .warnings
      .iter()
      .all(|warning| warning.starts_with("[submitForm] multipart")),
    "{:?}",
    converted.warnings
  );
}

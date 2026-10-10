use rstest::rstest;
use serde_json::json;

use super::{convert, field};

#[rstest]
#[case::base_schema_dispatches_in_mapping_order_and_falls_back_to_its_base(
  "Park",
  "@odata.type",
  &[
    "#park.stumpy => Stumpy(Box<Stumpy>)",
    "#park.floof | #park.floof.legacy => Floof(Box<Floof>)",
    "#park.sploot => Sploot(Box<Sploot>)",
    "_ => Park(Box<ParkBase>)",
  ][..],
)]
#[case::any_of_with_mapping_is_tagged_like_one_of(
  "Corgi",
  "type",
  &["pembroke => Pembroke(Pembroke)", "cardigan => Cardigan(Cardigan)"],
)]
fn discriminator_mapping_builds_a_tagged_enum(
  #[case] name: &str,
  #[case] expected_field: &str,
  #[case] expected_dispatch: &[&str],
) {
  let converted = convert(spec!("converter/discriminator.json"), config!());
  let def = converted.discriminated_enum(name);
  let tagged = def.variants.iter().map(|variant| {
    format!(
      "{} => {}({})",
      variant.discriminator_values.join(" | "),
      variant.variant_name,
      variant.type_name.to_rust_type()
    )
  });
  let fallback = def
    .fallback
    .iter()
    .map(|variant| format!("_ => {}({})", variant.variant_name, variant.type_name.to_rust_type()));

  assert_eq!(def.discriminator_field, expected_field);
  assert_eq!(tagged.chain(fallback).collect::<Vec<_>>(), expected_dispatch);
}

#[rstest]
fn discriminator_properties_hide_unless_they_carry_choices() {
  let converted = convert(spec!("converter/discriminator.json"), config!());
  let properties = [
    ("Bark", "role"),
    ("Howl", "role"),
    ("ParkBase", "odata_type"),
    ("Stumpy", "odata_type"),
  ]
  .map(|(owner, name)| {
    let property = field(converted.struct_def(owner), name);
    let visibility = if property.doc_hidden { "#[doc(hidden)] " } else { "" };
    format!("{visibility}{owner}.{name}: {}", property.rust_type.to_rust_type())
  });

  assert_eq!(
    properties,
    [
      "Bark.role: BarkRole",
      "#[doc(hidden)] Howl.role: Option<&'static str>",
      "#[doc(hidden)] ParkBase.odata_type: Option<&'static str>",
      "#[doc(hidden)] Stumpy.odata_type: Option<&'static str>",
    ]
  );
  assert_eq!(
    field(converted.struct_def("Stumpy"), "odata_type").default_value,
    Some(json!("#park.stumpy")),
    "an inheriting schema tags itself with its mapping key"
  );
}

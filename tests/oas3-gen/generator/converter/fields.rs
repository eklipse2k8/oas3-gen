use oas3_gen::generator::ast::{OuterAttr, RustPrimitive, SerdeAsFieldAttr, SerdeAttribute, ValidationAttribute};
use rstest::rstest;
use serde_json::Number;

use super::convert;

#[rstest]
fn colliding_property_names_get_numbered_and_deprecated_duplicates_drop() {
  let converted = convert(spec!("converter/fields.json"), config!());
  let names = converted
    .struct_def("Collisions")
    .fields
    .iter()
    .map(|field| {
      let rename = field.serde_attrs.iter().find_map(|attr| match attr {
        SerdeAttribute::Rename(wire) => Some(wire.as_str()),
        _ => None,
      });
      (field.name.as_str(), rename)
    })
    .collect::<Vec<_>>();
  assert_eq!(
    names,
    [
      ("loaf", None),
      ("loaf_2", Some("Loaf")),
      ("loaf_3", Some("LOAF")),
      ("sploot_id", None),
      ("old_name", Some("old-name")),
      ("old_name_2", None),
    ]
  );
}

#[rstest]
fn format_customizations_override_serde_as() {
  let customizations = [
    ("date_time", "crate::MyDateTime"),
    ("date", "crate::MyDate"),
    ("uuid", "crate::MyUuid"),
    ("byte", "crate::MyBase64"),
  ]
  .into_iter()
  .map(|(format, path)| (format.to_string(), path.to_string()))
  .collect();
  let converted = convert(spec!("converter/fields.json"), config!(customizations: customizations));
  let stamps = converted.struct_def("Stamps");
  let overrides = stamps
    .fields
    .iter()
    .map(|field| (field.name.as_str(), field.serde_as_attr.clone()))
    .collect::<Vec<_>>();
  let custom = |custom_type: &str, optional, is_array| {
    Some(SerdeAsFieldAttr::CustomOverride {
      custom_type: custom_type.to_string(),
      optional,
      is_array,
    })
  };

  assert_eq!(
    overrides,
    [
      ("at", custom("crate::MyDateTime", false, false)),
      ("maybe_at", custom("crate::MyDateTime", true, false)),
      ("ats", custom("crate::MyDateTime", false, true)),
      ("on", custom("crate::MyDate", false, false)),
      ("id", custom("crate::MyUuid", false, false)),
      ("blob", custom("crate::MyBase64", false, false)),
      ("name", None),
    ]
  );
  assert!(stamps.outer_attrs.contains(&OuterAttr::SerdeAs));
}

#[rstest]
fn validation_skips_constraints_the_field_type_cannot_carry() {
  let converted = convert(spec!("converter/fields.json"), config!());
  let min_length = |min| {
    vec![ValidationAttribute::Length {
      min: Some(min),
      max: None,
    }]
  };
  let validations = converted
    .struct_def("Checks")
    .fields
    .iter()
    .map(|field| (field.name.as_str(), field.validation_attrs.clone()))
    .collect::<Vec<_>>();

  assert_eq!(
    validations,
    [
      ("required_string", min_length(1)),
      ("nullable_required", vec![]),
      ("nullable_explicit", min_length(5)),
      ("date_min", vec![]),
      ("bad_regex", vec![]),
      ("enum_regex", vec![]),
      ("datetime_regex", vec![]),
      ("uuid_regex", vec![]),
      ("uri", vec![ValidationAttribute::Url]),
      (
        "exclusive",
        vec![ValidationAttribute::Range {
          primitive: RustPrimitive::F64,
          min: None,
          max: None,
          exclusive_min: Some(Number::from(0)),
          exclusive_max: Some(Number::from(100)),
        }]
      ),
    ]
  );
}

#[rstest]
fn field_defaults_and_annotations_follow_precedence() {
  let converted = convert(spec!("converter/fields.json"), config!());
  let fields = converted
    .struct_def("Defaults")
    .fields
    .iter()
    .map(|field| {
      let default = field
        .default_value
        .as_ref()
        .map(|value| format!(" = {value}"))
        .unwrap_or_default();
      let docs = field.docs.to_string();
      let docs = docs.trim_end();
      let docs = if docs.is_empty() {
        String::new()
      } else {
        format!(" ({docs})")
      };
      format!("{}: {}{default}{docs}", field.name, field.rust_type.to_rust_type())
    })
    .collect::<Vec<_>>();

  assert_eq!(
    fields,
    [
      "from_default: Option<String> = \"d\"",
      "from_const: Option<String> = \"c\"",
      "from_single_enum: Option<String> = \"only\"",
      "required_union: Option<DefaultsRequiredUnion>",
      "required_plain: String",
      "annotated: Option<Closed> = \"a\" (picked)",
      "wrapped: Option<Closed> = \"b\" (Closed choices)",
      "plain: Option<Closed> = \"b\" (Closed choices)",
    ]
  );
}

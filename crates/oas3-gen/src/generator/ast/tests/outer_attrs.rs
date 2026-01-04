use quote::ToTokens;

use crate::generator::ast::{SerdeAsFieldAttr, SerdeAsSeparator};

#[test]
fn test_serde_as_separated_list_non_optional() {
  let attr = SerdeAsFieldAttr::SeparatedList {
    separator: SerdeAsSeparator::Comma,
    optional: false,
  };
  assert_eq!(
    attr.to_token_stream().to_string(),
    "# [serde_as (as = \"oas3_gen_support::StringWithCommaSeparator\")]"
  );
}

#[test]
fn test_serde_as_separated_list_optional() {
  let attr = SerdeAsFieldAttr::SeparatedList {
    separator: SerdeAsSeparator::Pipe,
    optional: true,
  };
  assert_eq!(
    attr.to_token_stream().to_string(),
    "# [serde_as (as = \"Option<oas3_gen_support::StringWithPipeSeparator>\")]"
  );
}

#[test]
fn test_serde_as_custom_override_basic() {
  let attr = SerdeAsFieldAttr::CustomOverride {
    custom_type: "crate::MyDateTime".to_string(),
    optional: false,
    is_array: false,
  };
  assert_eq!(
    attr.to_token_stream().to_string(),
    "# [serde_as (as = \"crate::MyDateTime\")]"
  );
}

#[test]
fn test_serde_as_custom_override_optional() {
  let attr = SerdeAsFieldAttr::CustomOverride {
    custom_type: "crate::MyDateTime".to_string(),
    optional: true,
    is_array: false,
  };
  assert_eq!(
    attr.to_token_stream().to_string(),
    "# [serde_as (as = \"Option<crate::MyDateTime>\")]"
  );
}

#[test]
fn test_serde_as_custom_override_array() {
  let attr = SerdeAsFieldAttr::CustomOverride {
    custom_type: "crate::MyDateTime".to_string(),
    optional: false,
    is_array: true,
  };
  assert_eq!(
    attr.to_token_stream().to_string(),
    "# [serde_as (as = \"Vec<crate::MyDateTime>\")]"
  );
}

#[test]
fn test_serde_as_custom_override_optional_array() {
  let attr = SerdeAsFieldAttr::CustomOverride {
    custom_type: "crate::MyDateTime".to_string(),
    optional: true,
    is_array: true,
  };
  assert_eq!(
    attr.to_token_stream().to_string(),
    "# [serde_as (as = \"Option<Vec<crate::MyDateTime>>\")]"
  );
}

#[test]
fn test_serde_as_custom_override_with_module_path() {
  let attr = SerdeAsFieldAttr::CustomOverride {
    custom_type: "my_crate::types::IsoDateTime".to_string(),
    optional: false,
    is_array: false,
  };
  assert_eq!(
    attr.to_token_stream().to_string(),
    "# [serde_as (as = \"my_crate::types::IsoDateTime\")]"
  );
}

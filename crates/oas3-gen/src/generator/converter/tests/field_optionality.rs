use oas3::spec::ObjectSchema;

use super::super::field_optionality::{FieldContext, FieldOptionalityPolicy};

#[test]
fn test_not_required_is_optional() {
  let policy = FieldOptionalityPolicy::standard();
  let schema = ObjectSchema::default();
  let ctx = FieldContext {
    is_required: false,
    ..Default::default()
  };
  assert!(policy.is_optional("field", &schema, ctx));
}

#[test]
fn test_has_default_is_optional() {
  let policy = FieldOptionalityPolicy::standard();
  let schema = ObjectSchema::default();
  let ctx = FieldContext {
    is_required: true,
    has_default: true,
    ..Default::default()
  };
  assert!(policy.is_optional("field", &schema, ctx));
}

#[test]
fn test_discriminator_without_enum_is_optional() {
  let policy = FieldOptionalityPolicy::standard();
  let schema = ObjectSchema::default();
  let ctx = FieldContext {
    is_required: true,
    is_discriminator_field: true,
    ..Default::default()
  };
  assert!(policy.is_optional("@odata.type", &schema, ctx));
}

#[test]
fn test_odata_on_concrete_type_requires_odata_policy() {
  let standard = FieldOptionalityPolicy::standard();
  let odata = FieldOptionalityPolicy::with_odata_support();
  let schema = ObjectSchema::default();
  let ctx = FieldContext {
    is_required: true,
    ..Default::default()
  };

  assert!(!standard.is_optional("@odata.type", &schema, ctx));
  assert!(odata.is_optional("@odata.type", &schema, ctx));
}

#[test]
fn test_odata_on_discriminated_type_not_optional() {
  let odata = FieldOptionalityPolicy::with_odata_support();
  let schema = ObjectSchema {
    discriminator: Some(oas3::spec::Discriminator {
      property_name: "@odata.type".to_string(),
      mapping: None,
    }),
    ..Default::default()
  };
  let ctx = FieldContext {
    is_required: true,
    ..Default::default()
  };
  assert!(!odata.is_optional("@odata.type", &schema, ctx));
}

#[test]
fn test_required_field_without_special_rules_not_optional() {
  let policy = FieldOptionalityPolicy::standard();
  let schema = ObjectSchema::default();
  let ctx = FieldContext {
    is_required: true,
    ..Default::default()
  };
  assert!(!policy.is_optional("regular_field", &schema, ctx));
}

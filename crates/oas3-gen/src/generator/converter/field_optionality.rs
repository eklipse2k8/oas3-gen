use oas3::spec::ObjectSchema;

/// Context about a field used to determine optionality.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct FieldContext {
  pub is_required: bool,
  pub has_default: bool,
  pub is_discriminator_field: bool,
  pub discriminator_has_enum: bool,
}

/// Policy for determining if a field in a struct should be `Option<T>`.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct FieldOptionalityPolicy {
  odata_support: bool,
}

impl FieldOptionalityPolicy {
  pub fn standard() -> Self {
    Self { odata_support: false }
  }

  pub fn with_odata_support() -> Self {
    Self { odata_support: true }
  }

  /// Determines if a field should be optional.
  ///
  /// A field is optional if any of these conditions are met:
  /// - Not in the required array
  /// - Has a default value
  /// - Is a discriminator field without an enum constraint
  /// - (With OData support) Is an `@odata.*` field on a non-discriminated type
  pub fn is_optional(self, prop_name: &str, parent_schema: &ObjectSchema, ctx: FieldContext) -> bool {
    if !ctx.is_required {
      return true;
    }
    if ctx.has_default {
      return true;
    }
    if ctx.is_discriminator_field && !ctx.discriminator_has_enum {
      return true;
    }
    if self.odata_support
      && prop_name.starts_with("@odata.")
      && parent_schema.discriminator.is_none()
      && parent_schema.all_of.is_empty()
    {
      return true;
    }
    false
  }
}

#[cfg(test)]
mod tests {
  use oas3::spec::ObjectSchema;

  use super::*;

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
}

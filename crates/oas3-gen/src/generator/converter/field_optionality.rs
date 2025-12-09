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

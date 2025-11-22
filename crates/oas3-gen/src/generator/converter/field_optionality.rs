use oas3::spec::ObjectSchema;

/// Context passed to optionality rules to determine if a field should be optional.
pub(crate) struct FieldOptionalityContext<'a> {
  pub prop_name: &'a str,
  pub parent_schema: &'a ObjectSchema,
  pub is_required: bool,
  pub has_default: bool,
  pub is_discriminator_field: bool,
  pub discriminator_has_enum: bool,
}

/// A rule that determines if a field should be optional based on context.
pub(crate) trait FieldOptionalityRule: Send + Sync {
  /// Returns true if this rule applies to the current context.
  fn applies(&self, ctx: &FieldOptionalityContext) -> bool;
  /// Returns true if the field should be optional.
  fn should_be_optional(&self, ctx: &FieldOptionalityContext) -> bool;
}

struct NotRequiredRule;

impl FieldOptionalityRule for NotRequiredRule {
  fn applies(&self, ctx: &FieldOptionalityContext) -> bool {
    !ctx.is_required
  }

  fn should_be_optional(&self, _ctx: &FieldOptionalityContext) -> bool {
    true
  }
}

struct HasDefaultRule;

impl FieldOptionalityRule for HasDefaultRule {
  fn applies(&self, ctx: &FieldOptionalityContext) -> bool {
    ctx.has_default
  }

  fn should_be_optional(&self, _ctx: &FieldOptionalityContext) -> bool {
    true
  }
}

struct DiscriminatorWithoutEnumRule;

impl FieldOptionalityRule for DiscriminatorWithoutEnumRule {
  fn applies(&self, ctx: &FieldOptionalityContext) -> bool {
    ctx.is_discriminator_field && !ctx.discriminator_has_enum
  }

  fn should_be_optional(&self, _ctx: &FieldOptionalityContext) -> bool {
    true
  }
}

struct ODataMetadataOnConcreteTypeRule;

impl FieldOptionalityRule for ODataMetadataOnConcreteTypeRule {
  fn applies(&self, ctx: &FieldOptionalityContext) -> bool {
    ctx.prop_name.starts_with("@odata.")
      && ctx.parent_schema.discriminator.is_none()
      && ctx.parent_schema.all_of.is_empty()
  }

  fn should_be_optional(&self, _ctx: &FieldOptionalityContext) -> bool {
    true
  }
}

#[derive(Debug, Clone, Copy)]
pub(crate) enum PolicyKind {
  Standard,
  WithODataSupport,
}

/// Policy for determining if a field in a struct should be `Option<T>`.
///
/// Supports different strategies, e.g., standard OpenAPI rules vs OData-specific hacks.
#[derive(Debug, Clone)]
pub(crate) struct FieldOptionalityPolicy {
  kind: PolicyKind,
}

impl FieldOptionalityPolicy {
  /// Creates a standard policy based on strict OpenAPI required/default rules.
  pub fn standard() -> Self {
    Self {
      kind: PolicyKind::Standard,
    }
  }

  /// Creates a policy that handles OData-specific fields (e.g. `@odata.type`).
  pub fn with_odata_support() -> Self {
    Self {
      kind: PolicyKind::WithODataSupport,
    }
  }

  /// Determines if a field should be optional based on the configured policy.
  pub fn compute_optionality(&self, ctx: &FieldOptionalityContext) -> bool {
    let rules: Vec<Box<dyn FieldOptionalityRule>> = match self.kind {
      PolicyKind::Standard => vec![
        Box::new(NotRequiredRule),
        Box::new(HasDefaultRule),
        Box::new(DiscriminatorWithoutEnumRule),
      ],
      PolicyKind::WithODataSupport => vec![
        Box::new(NotRequiredRule),
        Box::new(HasDefaultRule),
        Box::new(DiscriminatorWithoutEnumRule),
        Box::new(ODataMetadataOnConcreteTypeRule),
      ],
    };

    for rule in &rules {
      if rule.applies(ctx) {
        return rule.should_be_optional(ctx);
      }
    }
    false
  }
}

impl Default for FieldOptionalityPolicy {
  fn default() -> Self {
    Self::standard()
  }
}

#[cfg(test)]
mod tests {
  use oas3::spec::ObjectSchema;

  use super::*;

  fn create_test_schema() -> ObjectSchema {
    ObjectSchema::default()
  }

  #[test]
  fn test_not_required_rule() {
    let policy = FieldOptionalityPolicy::standard();
    let schema = create_test_schema();

    let ctx = FieldOptionalityContext {
      prop_name: "field",
      parent_schema: &schema,
      is_required: false,
      has_default: false,
      is_discriminator_field: false,
      discriminator_has_enum: false,
    };

    assert!(policy.compute_optionality(&ctx));
  }

  #[test]
  fn test_has_default_rule() {
    let policy = FieldOptionalityPolicy::standard();
    let schema = create_test_schema();

    let ctx = FieldOptionalityContext {
      prop_name: "field",
      parent_schema: &schema,
      is_required: true,
      has_default: true,
      is_discriminator_field: false,
      discriminator_has_enum: false,
    };

    assert!(policy.compute_optionality(&ctx));
  }

  #[test]
  fn test_discriminator_without_enum_rule() {
    let policy = FieldOptionalityPolicy::standard();
    let schema = create_test_schema();

    let ctx = FieldOptionalityContext {
      prop_name: "@odata.type",
      parent_schema: &schema,
      is_required: true,
      has_default: false,
      is_discriminator_field: true,
      discriminator_has_enum: false,
    };

    assert!(policy.compute_optionality(&ctx));
  }

  #[test]
  fn test_odata_on_concrete_type_requires_odata_policy() {
    let standard_policy = FieldOptionalityPolicy::standard();
    let odata_policy = FieldOptionalityPolicy::with_odata_support();
    let schema = create_test_schema();

    let ctx = FieldOptionalityContext {
      prop_name: "@odata.type",
      parent_schema: &schema,
      is_required: true,
      has_default: false,
      is_discriminator_field: false,
      discriminator_has_enum: false,
    };

    assert!(!standard_policy.compute_optionality(&ctx));
    assert!(odata_policy.compute_optionality(&ctx));
  }

  #[test]
  fn test_odata_on_discriminated_type_not_optional() {
    let odata_policy = FieldOptionalityPolicy::with_odata_support();
    let mut schema = create_test_schema();
    schema.discriminator = Some(oas3::spec::Discriminator {
      property_name: "@odata.type".to_string(),
      mapping: None,
    });

    let ctx = FieldOptionalityContext {
      prop_name: "@odata.type",
      parent_schema: &schema,
      is_required: true,
      has_default: false,
      is_discriminator_field: false,
      discriminator_has_enum: false,
    };

    assert!(!odata_policy.compute_optionality(&ctx));
  }

  #[test]
  fn test_required_field_without_special_rules_not_optional() {
    let policy = FieldOptionalityPolicy::standard();
    let schema = create_test_schema();

    let ctx = FieldOptionalityContext {
      prop_name: "regular_field",
      parent_schema: &schema,
      is_required: true,
      has_default: false,
      is_discriminator_field: false,
      discriminator_has_enum: false,
    };

    assert!(!policy.compute_optionality(&ctx));
  }
}

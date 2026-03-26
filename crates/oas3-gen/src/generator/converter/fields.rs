use std::{
  collections::{BTreeMap, BTreeSet},
  rc::Rc,
};

use anyhow::Context as _;
use itertools::{Either, Itertools};
use oas3::spec::{ObjectOrReference, ObjectSchema, Schema};
use regex::Regex;

use super::{ConversionOutput, type_resolver::TypeResolver};
use crate::{
  generator::{
    ast::{
      FieldDef, FieldNameToken, RustPrimitive, RustType, SerdeAsFieldAttr, SerdeAttribute, StructKind, TypeRef,
      ValidationAttribute,
    },
    converter::ConverterContext,
    schema_registry::DiscriminatorMapping,
  },
  utils::SchemaExt,
};

/// Contains resolved field information including type, inline definitions, and validation.
///
/// Returned by [`FieldConverter::resolve_with_metadata`] to bundle all data
/// needed for field construction in one pass.
#[derive(Clone, Debug)]
pub(crate) struct ResolvedFieldData {
  pub(crate) type_ref: TypeRef,
  pub(crate) inline_types: Vec<RustType>,
  pub(crate) validation_attrs: Vec<ValidationAttribute>,
  pub(crate) schema: ObjectSchema,
}

/// Converts OpenAPI object properties into Rust struct field definitions.
///
/// Coordinates type resolution, validation attribute extraction, and serde
/// attribute generation. Applies special handling for discriminator fields,
/// OData properties, and custom type mappings.
#[derive(Clone, Debug)]
pub(crate) struct FieldConverter {
  context: Rc<ConverterContext>,
  type_resolver: TypeResolver,
}

impl FieldConverter {
  /// Creates a new field converter with configuration from the context.
  pub(crate) fn new(context: &Rc<ConverterContext>) -> Self {
    Self {
      context: context.clone(),
      type_resolver: TypeResolver::new(context.clone()),
    }
  }

  /// Resolves a schema reference and extracts type information with validation metadata.
  ///
  /// This centralizes the common pattern of resolving the schema reference,
  /// checking for inline enums, resolving to a Rust type, and extracting
  /// validation attributes and default values.
  pub(crate) fn resolve_with_metadata(
    &self,
    parent_name: &str,
    prop_name: &str,
    schema_ref: &ObjectOrReference<ObjectSchema>,
    is_required: bool,
  ) -> anyhow::Result<ResolvedFieldData> {
    let spec = self.context.graph().spec();
    let schema = schema_ref.resolve(spec)?;

    let (type_ref, inline_types) = if schema.has_inline_enum(spec) {
      let result = self
        .type_resolver
        .resolve_property(parent_name, prop_name, &schema, schema_ref)?;
      (result.result, result.inline_types)
    } else {
      (self.type_resolver.resolve_type(&schema)?, vec![])
    };

    let validation_attrs = Self::extract_all_validation(prop_name, is_required, &schema, &type_ref);

    Ok(ResolvedFieldData {
      type_ref,
      inline_types,
      validation_attrs,
      schema,
    })
  }

  /// Collects struct fields from all properties in an object schema.
  ///
  /// Iterates over `schema.properties`, resolves each property's type,
  /// deduplicates field names, and adds an `additionalProperties` catch-all
  /// field when the schema defines one. Returns the fields along with any
  /// inline types extracted from nested object or enum properties.
  pub(crate) fn build_struct_fields(
    &self,
    parent_name: &str,
    schema: &ObjectSchema,
    schema_name: Option<&str>,
    kind: StructKind,
  ) -> anyhow::Result<ConversionOutput<Vec<FieldDef>>> {
    let spec = self.context.graph().spec();
    let required = schema.required.iter().collect::<BTreeSet<_>>();
    let discriminator_mapping = schema_name.and_then(|name| self.context.graph().mapping(name));

    let mut fields = vec![];
    let mut inline_types = vec![];

    for (prop_name, prop_schema_ref) in &schema.properties {
      let prop_schema = prop_schema_ref
        .resolve(spec)
        .context(format!("Schema resolution failed for property '{prop_name}'"))?;

      let resolved = self
        .type_resolver
        .resolve_property(parent_name, prop_name, &prop_schema, prop_schema_ref)?;

      fields.push(self.convert_field(
        prop_name,
        schema,
        &prop_schema,
        resolved.result,
        required.contains(prop_name),
        discriminator_mapping,
      ));
      inline_types.extend(resolved.inline_types);
    }

    fields = Self::deduplicate_names(fields);

    if let Some(ref additional) = schema.additional_properties {
      match additional {
        Schema::Boolean(b) if !b.0 => {}
        Schema::Object(_) | Schema::Boolean(_) => {
          let value_type = self.type_resolver.additional_properties_type(additional)?;
          fields.push(FieldDef::builder().additional_properties(&value_type).build());
        }
      }
    }

    if matches!(kind, StructKind::Schema) && self.context.config().enable_builders() {
      fields = fields.into_iter().map(FieldDef::with_builder_attrs).collect::<Vec<_>>();
    }

    Ok(ConversionOutput::with_inline_types(fields, inline_types))
  }

  fn convert_field(
    &self,
    prop_name: &str,
    parent_schema: &ObjectSchema,
    prop_schema: &ObjectSchema,
    resolved_type: TypeRef,
    is_required: bool,
    discriminator_mapping: Option<&DiscriminatorMapping>,
  ) -> FieldDef {
    let discriminator_value = discriminator_mapping
      .filter(|m| m.field_name == prop_name)
      .map(|m| m.field_value.as_str());

    let is_base_discriminator = parent_schema
      .discriminator
      .as_ref()
      .is_some_and(|d| d.property_name == prop_name);

    let is_discriminator = discriminator_value.is_some() || is_base_discriminator;
    let discriminator_has_enum = is_discriminator && prop_schema.has_selectable_values();
    let is_base = is_base_discriminator && discriminator_value.is_none();

    let is_odata_optional = self.context.config().odata_support()
      && prop_name.starts_with("@odata.")
      && parent_schema.discriminator.is_none()
      && !parent_schema.has_intersection();

    let should_be_optional = !is_required
      || prop_schema.default.is_some()
      || (is_discriminator && !discriminator_has_enum)
      || is_odata_optional;

    let final_type = if should_be_optional && !resolved_type.nullable {
      resolved_type.with_option()
    } else {
      resolved_type
    };

    let validation_attrs = Self::extract_all_validation(prop_name, is_required, prop_schema, &final_type);
    let default_value = Self::extract_default_value(prop_schema);

    let rust_field_name = FieldNameToken::from_raw(prop_name);
    let serde_attrs = if rust_field_name == prop_name {
      BTreeSet::new()
    } else {
      BTreeSet::from([SerdeAttribute::Rename(prop_name.to_string())])
    };

    let serde_as_attr = self.customization_for_type(&final_type);

    let field = FieldDef::builder()
      .schema(prop_schema)
      .maybe_default_value(default_value)
      .maybe_serde_as_attr(serde_as_attr)
      .name(rust_field_name)
      .rust_type(final_type)
      .serde_attrs(serde_attrs)
      .validation_attrs(validation_attrs)
      .build();

    let should_hide = is_discriminator && !discriminator_has_enum;
    if should_hide {
      field.with_discriminator_behavior(discriminator_value, is_base)
    } else {
      field
    }
  }

  fn customization_for_type(&self, type_ref: &TypeRef) -> Option<SerdeAsFieldAttr> {
    let key = match &type_ref.base_type {
      RustPrimitive::DateTime => "date_time",
      RustPrimitive::Date => "date",
      RustPrimitive::Time => "time",
      RustPrimitive::Duration => "duration",
      RustPrimitive::Uuid => "uuid",
      RustPrimitive::Custom(name) => name,
      _ => return None,
    };
    let custom_type = self.context.config().customizations.get(key)?;
    Some(SerdeAsFieldAttr::CustomOverride {
      custom_type: custom_type.clone(),
      optional: type_ref.nullable,
      is_array: type_ref.is_array,
    })
  }

  /// Extracts a default value from schema `default`, `const`, or single-value `enum`.
  ///
  /// Prefers `default` over `const` over single-element enum arrays.
  pub(crate) fn extract_default_value(schema: &ObjectSchema) -> Option<serde_json::Value> {
    schema
      .default
      .clone()
      .or_else(|| schema.const_value.clone())
      .or_else(|| schema.enum_values.iter().exactly_one().ok().cloned())
  }

  /// Extracts validation attributes from schema constraints.
  ///
  /// Generates `#[validate(email)]`, `#[validate(url)]`, `#[validate(range)]`,
  /// `#[validate(length)]`, and `#[validate(regex)]` based on format,
  /// min/max values, and pattern constraints.
  pub(crate) fn extract_all_validation(
    prop_name: &str,
    is_required: bool,
    schema: &ObjectSchema,
    type_ref: &TypeRef,
  ) -> Vec<ValidationAttribute> {
    let mut attrs = Vec::with_capacity(3);

    if let Some(ref format) = schema.format {
      match format.as_str() {
        "email" => attrs.push(ValidationAttribute::Email),
        "uri" | "url" => attrs.push(ValidationAttribute::Url),
        _ => {}
      }
    }

    if schema.is_numeric() {
      if let Some(range_attr) = ValidationAttribute::range(schema, type_ref) {
        attrs.push(range_attr);
      }
      return attrs;
    }

    if schema.is_unconstrained_string() {
      let is_non_string_format = schema.format.as_ref().is_some_and(|f| {
        matches!(
          f.as_str(),
          "date" | "date-time" | "duration" | "time" | "binary" | "byte" | "uuid"
        )
      });

      if !is_non_string_format {
        if let Some(length_attr) =
          ValidationAttribute::length(schema.min_length, schema.max_length, is_required && !type_ref.nullable)
        {
          attrs.push(length_attr);
        }

        if let Some(pattern) = schema.pattern.as_ref() {
          if Regex::new(pattern).is_ok() {
            let skip = matches!(
              &type_ref.base_type,
              RustPrimitive::DateTime | RustPrimitive::Date | RustPrimitive::Time | RustPrimitive::Uuid
            );
            if !skip {
              attrs.push(ValidationAttribute::Regex(pattern.clone()));
            }
          } else {
            eprintln!("Warning: Invalid regex pattern '{pattern}' for property '{prop_name}'");
          }
        }
      }
      return attrs;
    }

    if schema.is_array()
      && let Some(length_attr) = ValidationAttribute::length(schema.min_items, schema.max_items, false)
    {
      attrs.push(length_attr);
    }

    attrs
  }

  /// Resolves field name collisions by appending numeric suffixes.
  ///
  /// When multiple properties map to the same Rust field name (e.g., `foo-bar`
  /// and `foo_bar` both become `foo_bar`), this removes deprecated duplicates
  /// when a non-deprecated version exists, then appends `_2`, `_3`, etc.
  /// to remaining duplicates.
  pub(crate) fn deduplicate_names(fields: Vec<FieldDef>) -> Vec<FieldDef> {
    let duplicate_names = fields
      .iter()
      .counts_by(|f| f.name.clone())
      .into_iter()
      .filter(|(_, value)| *value > 1)
      .map(|(name, _)| name)
      .collect::<BTreeSet<_>>();

    if duplicate_names.is_empty() {
      return fields;
    }

    let (deprecated, non_deprecated): (BTreeSet<_>, BTreeSet<_>) = fields
      .iter()
      .filter(|f| duplicate_names.contains(&f.name))
      .partition_map(|f| {
        let name = f.name.clone();
        if f.deprecated {
          Either::Left(name)
        } else {
          Either::Right(name)
        }
      });

    let mut occurrence = BTreeMap::<FieldNameToken, usize>::new();

    fields
      .into_iter()
      .filter_map(|field| {
        let name = field.name.clone();

        if field.deprecated && deprecated.contains(&name) && non_deprecated.contains(&name) {
          return None;
        }

        let n = *occurrence.entry(name.clone()).and_modify(|n| *n += 1).or_insert(1);

        if n > 1 {
          Some(field.renamed_to(&format!("{name}_{n}")))
        } else {
          Some(field)
        }
      })
      .collect()
  }
}

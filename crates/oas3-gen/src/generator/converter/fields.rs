use std::{
  collections::{BTreeMap, BTreeSet, HashMap},
  rc::Rc,
};

use anyhow::Context as _;
use itertools::{Either, Itertools};
use oas3::spec::{ObjectOrReference, ObjectSchema, Schema};
use regex::Regex;
use string_cache::DefaultAtom;

use super::{ConversionOutput, RustType, type_resolver::TypeResolver};
use crate::{
  generator::{
    ast::{
      FieldDef, FieldNameToken, OuterAttr, RustPrimitive, SerdeAsFieldAttr, SerdeAttribute, TypeRef,
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
  odata_support: bool,
  customizations: HashMap<String, String>,
}

impl FieldConverter {
  /// Creates a new field converter with configuration from the context.
  pub(crate) fn new(context: &Rc<ConverterContext>) -> Self {
    let config = context.config();
    Self {
      context: context.clone(),
      type_resolver: TypeResolver::new(context.clone()),
      odata_support: config.odata_support(),
      customizations: config.customizations.clone(),
    }
  }

  /// Resolves a schema reference and extracts type information with validation metadata.
  ///
  /// This centralizes the common pattern of:
  /// 1. Resolving the schema reference
  /// 2. Checking for inline enums (which require `resolve_property` for inline type creation)
  /// 3. Resolving to a Rust type
  /// 4. Extracting validation attributes and default values
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
  /// extracts validation attributes, and builds field definitions.
  /// Deduplicates field names that collide after Rust identifier conversion.
  /// Returns the fields along with any inline types extracted from
  /// nested object or enum properties.
  pub(crate) fn collect_fields(
    &self,
    parent_name: &str,
    schema: &ObjectSchema,
    schema_name: Option<&str>,
  ) -> anyhow::Result<ConversionOutput<Vec<FieldDef>>> {
    let required = schema.required.iter().collect::<BTreeSet<_>>();
    let discriminator_mapping = schema_name
      .and_then(|name| self.context.graph().mapping(name))
      .map(DiscriminatorMapping::as_tuple);

    let (fields, inline_types) = itertools::process_results(
      schema.properties.iter().map(|(prop_name, prop_schema_ref)| {
        let prop_schema = prop_schema_ref
          .resolve(self.context.graph().spec())
          .context(format!("Schema resolution failed for property '{prop_name}'"))?;

        let resolved = self
          .type_resolver
          .resolve_property(parent_name, prop_name, &prop_schema, prop_schema_ref)?;

        let field = self.convert_field(
          prop_name,
          schema,
          &prop_schema,
          resolved.result,
          required.contains(prop_name),
          discriminator_mapping.as_ref(),
        );

        anyhow::Ok((field, resolved.inline_types))
      }),
      |iter| iter.unzip::<_, _, Vec<_>, Vec<_>>(),
    )?;
    let inline_types = inline_types.into_iter().flatten().collect::<Vec<_>>();

    Ok(ConversionOutput::with_inline_types(
      Self::deduplicate_names(fields),
      inline_types,
    ))
  }

  /// Converts `additionalProperties` into serde attributes and an optional catch-all field.
  ///
  /// Returns `#[serde(deny_unknown_fields)]` when `additionalProperties: false`,
  /// or a `HashMap<String, T>` field when `additionalProperties` specifies a schema.
  /// Returns empty attributes and `None` when `additionalProperties` is not set.
  pub(crate) fn build_additional_properties(
    &self,
    schema: &ObjectSchema,
  ) -> anyhow::Result<(Vec<SerdeAttribute>, Option<FieldDef>)> {
    let Some(ref additional) = schema.additional_properties else {
      return Ok((vec![], None));
    };

    match additional {
      Schema::Boolean(b) if !b.0 => Ok((vec![SerdeAttribute::DenyUnknownFields], None)),
      Schema::Object(_) => {
        let value_type = self.type_resolver.additional_properties_type(additional)?;
        Ok((
          vec![],
          Some(FieldDef::builder().additional_properties(&value_type).build()),
        ))
      }
      Schema::Boolean(_) => Ok((vec![], None)),
    }
  }

  /// Derives struct-level serde and outer attributes from field characteristics.
  ///
  /// Adds `#[serde(default)]` if any field has a default value. Adds
  /// `#[serde_as]` if any field uses serde_with custom serialization.
  pub(crate) fn struct_attributes(
    fields: &[FieldDef],
    base_serde: Vec<SerdeAttribute>,
  ) -> (Vec<SerdeAttribute>, Vec<OuterAttr>) {
    let default_serde = fields
      .iter()
      .any(|f| f.default_value.is_some())
      .then_some(SerdeAttribute::Default);

    let serde_attrs = base_serde.into_iter().chain(default_serde).collect();

    let outer_attrs = fields
      .iter()
      .any(|f| f.serde_as_attr.is_some())
      .then_some(OuterAttr::SerdeAs)
      .into_iter()
      .collect();

    (serde_attrs, outer_attrs)
  }

  /// Converts a single property into a Rust field definition.
  ///
  /// Applies optionality rules (required, default, discriminator, OData),
  /// extracts validation attributes from schema constraints, and generates
  /// serde rename attributes when the Rust field name differs from the
  /// JSON property name.
  pub(crate) fn convert_field(
    &self,
    prop_name: &str,
    parent_schema: &ObjectSchema,
    prop_schema: &ObjectSchema,
    resolved_type: TypeRef,
    is_required: bool,
    discriminator_mapping: Option<&(String, String)>,
  ) -> FieldDef {
    let discriminator_info = DiscriminatorFieldInfo::new(prop_name, parent_schema, prop_schema, discriminator_mapping);

    let should_be_optional = self.is_field_optional(
      prop_name,
      parent_schema,
      prop_schema,
      discriminator_info.as_ref(),
      is_required,
    );
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

    let serde_as_attr = self.get_customization_for_type(&final_type);

    let field = FieldDef::builder()
      .schema(prop_schema)
      .maybe_default_value(default_value)
      .maybe_serde_as_attr(serde_as_attr)
      .name(rust_field_name)
      .rust_type(final_type)
      .serde_attrs(serde_attrs)
      .validation_attrs(validation_attrs)
      .build();

    match discriminator_info.as_ref().filter(|d| d.should_hide()) {
      Some(info) => field.with_discriminator_behavior(info.value.as_deref(), info.is_base),
      None => field,
    }
  }

  /// Looks up a custom serde_with type override for the field's primitive type.
  ///
  /// Returns `Some` if the configuration specifies a custom serialization
  /// type for this primitive (e.g., `date_time` → `time::OffsetDateTime`).
  fn get_customization_for_type(&self, type_ref: &TypeRef) -> Option<SerdeAsFieldAttr> {
    let key = Self::primitive_to_key(&type_ref.base_type)?;
    let custom_type = self.customizations.get(&key)?;
    Some(SerdeAsFieldAttr::CustomOverride {
      custom_type: custom_type.clone(),
      optional: type_ref.nullable,
      is_array: type_ref.is_array,
    })
  }

  /// Maps a primitive type to its customization lookup key.
  fn primitive_to_key(primitive: &RustPrimitive) -> Option<String> {
    match primitive {
      RustPrimitive::DateTime => Some("date_time".to_string()),
      RustPrimitive::Date => Some("date".to_string()),
      RustPrimitive::Time => Some("time".to_string()),
      RustPrimitive::Duration => Some("duration".to_string()),
      RustPrimitive::Uuid => Some("uuid".to_string()),
      RustPrimitive::Custom(name) => Some(name.to_string()),
      _ => None,
    }
  }

  #[cfg(test)]
  pub(crate) fn extract_parameter_metadata(
    prop_name: &str,
    is_required: bool,
    schema: &ObjectSchema,
    type_ref: &TypeRef,
  ) -> (Vec<ValidationAttribute>, Option<serde_json::Value>) {
    let validation_attrs = Self::extract_all_validation(prop_name, is_required, schema, type_ref);
    let default_value = Self::extract_default_value(schema);
    (validation_attrs, default_value)
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
  /// min/max values, and pattern constraints. Skips regex validation for
  /// types with known formats (date, datetime, uuid).
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

    if schema.is_freeform_string() {
      let has_non_string_format = schema.format.as_ref().is_some_and(|f| Self::is_non_string_format(f));

      if !has_non_string_format {
        if let Some(length_attr) = ValidationAttribute::length(schema.min_length, schema.max_length, is_required) {
          attrs.push(length_attr);
        }

        if let Some(pattern) = schema.pattern.as_ref() {
          if Regex::new(pattern).is_ok() {
            if let Some(regex) = Self::filter_regex_validation(Some(pattern.clone()), type_ref) {
              attrs.push(ValidationAttribute::Regex(regex));
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

  /// Filters out regex validation for types with built-in format validation.
  fn filter_regex_validation(regex: Option<String>, type_ref: &TypeRef) -> Option<String> {
    match &type_ref.base_type {
      RustPrimitive::DateTime | RustPrimitive::Date | RustPrimitive::Time | RustPrimitive::Uuid => None,
      _ => regex,
    }
  }

  /// Returns `true` if the format indicates a non-string primitive type.
  fn is_non_string_format(format: &str) -> bool {
    matches!(
      format,
      "date" | "date-time" | "duration" | "time" | "binary" | "byte" | "uuid"
    )
  }

  /// Determines whether a field should be wrapped in `Option<T>`.
  ///
  /// Returns `true` when:
  /// - The field is not in the `required` array
  /// - The field has a default value
  /// - The field is a discriminator without enum values
  /// - OData support is enabled and the field is an `@odata.*` property
  fn is_field_optional(
    &self,
    prop_name: &str,
    parent_schema: &ObjectSchema,
    prop_schema: &ObjectSchema,
    discriminator_info: Option<&DiscriminatorFieldInfo>,
    is_required: bool,
  ) -> bool {
    let has_default = prop_schema.default.is_some();
    let is_discriminator_field = discriminator_info.is_some();
    let discriminator_has_enum = discriminator_info.is_some_and(|i| i.has_enum);

    if !is_required || has_default {
      return true;
    }
    if is_discriminator_field && !discriminator_has_enum {
      return true;
    }
    if self.odata_support
      && prop_name.starts_with("@odata.")
      && parent_schema.discriminator.is_none()
      && !parent_schema.has_intersection()
    {
      return true;
    }
    false
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

/// Tracks discriminator-related metadata for a field during conversion.
///
/// Used to determine special handling for fields that serve as discriminator
/// properties in polymorphic schemas.
#[derive(Debug, Clone)]
pub(crate) struct DiscriminatorFieldInfo {
  pub value: Option<DefaultAtom>,
  pub is_base: bool,
  pub has_enum: bool,
}

impl DiscriminatorFieldInfo {
  /// Creates discriminator info if the field is a discriminator property.
  ///
  /// Returns `None` if the field is not related to discriminator handling.
  /// Sets `is_base` for discriminator properties on base types, and `value`
  /// for child types with a known discriminator value.
  pub fn new(
    prop_name: &str,
    parent_schema: &ObjectSchema,
    prop_schema: &ObjectSchema,
    discriminator_mapping: Option<&(String, String)>,
  ) -> Option<Self> {
    let value = discriminator_mapping
      .filter(|(prop, _)| prop == prop_name)
      .map(|(_, v)| DefaultAtom::from(v.as_str()));

    let is_base_discriminator = parent_schema
      .discriminator
      .as_ref()
      .is_some_and(|d| d.property_name == prop_name);

    let is_child_discriminator = value.is_some();

    if !is_child_discriminator && !is_base_discriminator {
      return None;
    }

    Some(Self {
      value,
      is_base: is_base_discriminator && !is_child_discriminator,
      has_enum: prop_schema.has_enum_values(),
    })
  }

  /// Returns `true` if the discriminator field should be hidden from the struct.
  ///
  /// Fields are hidden when they lack enum values and either have a fixed
  /// discriminator value (child types) or are the base type's discriminator.
  pub fn should_hide(&self) -> bool {
    !self.has_enum && (self.value.is_some() || self.is_base)
  }
}

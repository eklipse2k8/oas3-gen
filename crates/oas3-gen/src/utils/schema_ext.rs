use inflections::Inflect;
use itertools::Itertools;
use oas3::{
  Spec,
  spec::{ObjectOrReference, ObjectSchema, Schema, SchemaType, SchemaTypeSet},
};

use crate::{
  generator::{
    ast::Documentation,
    converter::union_types::{EnumValueEntry, UnionKind},
    naming::{
      constants::{REQUEST_BODY_SUFFIX, RESPONSE_PREFIX, RESPONSE_SUFFIX},
      identifiers::{sanitize, to_rust_type_name},
      inference::{NormalizedVariant, extract_common_variant_prefix},
    },
  },
  utils::refs::extract_schema_ref_name,
};

/// Extension methods for `ObjectSchema` to query its type properties conveniently.
pub(crate) trait SchemaExt {
  /// Returns true if the schema represents a primitive type (no properties, oneOf, anyOf, allOf).
  fn is_primitive(&self) -> bool;

  /// Returns true if the schema is explicitly null type.
  fn is_null(&self) -> bool;

  /// Returns true if the schema is a nullable placeholder (pure null or empty object with null).
  /// This includes schemas like `{type: "null"}` and `{type: ["object", "null"]}` with no properties.
  fn is_nullable_object(&self) -> bool;

  /// Returns true if the schema is an array type.
  fn is_array(&self) -> bool;

  /// Returns true if the schema is a string type.
  fn is_string(&self) -> bool;

  /// Returns true if the schema is an object type.
  fn is_object(&self) -> bool;

  /// Returns true if the schema is a numeric type (integer or number).
  fn is_numeric(&self) -> bool;

  /// Returns true if the schema is a nullable array type `[array, null]`.
  fn is_nullable_array(&self) -> bool;

  /// Returns true if the schema has exactly the single specified type.
  fn is_single_type(&self, schema_type: SchemaType) -> bool;

  /// Returns the single `SchemaType` if exactly one is defined, None otherwise.
  fn single_type(&self) -> Option<SchemaType>;

  /// Returns the non-null type from a two-type nullable set (e.g., `[string, null]` -> `string`).
  fn non_null_type(&self) -> Option<SchemaType>;

  /// Returns true if the schema represents an inline object definition.
  /// This excludes enums, unions, arrays, and schemas without properties.
  fn is_inline_object(&self) -> bool;

  /// Returns true if the schema is a discriminated base type with a non-empty mapping.
  fn is_discriminated_base_type(&self) -> bool;

  /// Returns true if the schema has no type constraints (no properties, no type info).
  /// An empty schema `{}` or one with only `additionalProperties: {}` both return true,
  /// as neither constrains the shape of the data.
  fn is_empty_object(&self) -> bool;

  /// Returns true if the schema has inline oneOf or anyOf variants.
  fn has_union(&self) -> bool;

  /// Returns true if this is an array with inline union items (oneOf/anyOf in items).
  fn has_inline_union_array_items(&self, spec: &Spec) -> bool;

  /// Extracts the inline array items schema if present and not a reference.
  /// Returns None if: no items, items is a boolean schema, or items is a $ref.
  fn inline_array_items<'a>(&'a self, spec: &'a Spec) -> Option<ObjectSchema>;

  /// Returns true if the schema has enum values defined.
  fn has_enum_values(&self) -> bool;

  /// Returns true if the schema has an inline enum (multiple enum values directly or in array items).
  fn has_inline_enum(&self, spec: &Spec) -> bool;

  /// Returns true if the schema has allOf composition.
  fn has_intersection(&self) -> bool;

  /// Returns true if the schema requires a dedicated type definition.
  /// This includes schemas with enum values, oneOf/anyOf unions, or typed object properties.
  fn requires_type_definition(&self) -> bool;

  /// Returns true if the schema has a relaxed enum pattern in anyOf.
  /// A relaxed enum has both freeform string variants and constrained string variants,
  /// allowing APIs to accept known values plus arbitrary strings for forward compatibility.
  fn has_relaxed_anyof_enum(&self) -> bool;

  /// Returns an iterator over all union variants (`anyOf` and `oneOf`) in a schema.
  ///
  /// # Example
  /// ```text
  /// schema.any_of = [A, B], schema.one_of = [C] => yields A, B, C
  /// ```
  fn union_variants(&self) -> impl Iterator<Item = &ObjectOrReference<ObjectSchema>>;

  /// Returns the union variants slice and kind, or None if not a union.
  ///
  /// This is the preferred method when you need both the variants and the union kind
  /// (oneOf vs anyOf) together, avoiding duplicate logic.
  fn union_variants_with_kind(&self) -> Option<(&[ObjectOrReference<ObjectSchema>], UnionKind)>;

  /// Returns true if any variant in the union is a null type.
  fn has_null_variant(&self, spec: &Spec) -> bool;

  /// Returns the first non-null variant in a union, if present.
  fn find_non_null_variant<'a>(&'a self, spec: &Spec) -> Option<&'a ObjectOrReference<ObjectSchema>>;

  /// Returns the single non-null variant if this union has exactly one.
  /// Returns None if there are 0 or 2+ non-null variants.
  fn single_non_null_variant<'a>(&'a self, spec: &Spec) -> Option<&'a ObjectOrReference<ObjectSchema>>;

  /// Returns true if this is a single-variant union where the variant is inline (not a $ref).
  fn has_inline_single_variant(&self, spec: &Spec) -> bool;

  /// Returns the single `SchemaType` if exactly one is defined, or the non-null type
  /// from a two-type nullable set (e.g., `[string, null]` -> `string`).
  fn single_type_or_nullable(&self) -> Option<SchemaType>;

  /// Returns true if the schema is a string type (including nullable string).
  fn is_string_type(&self) -> bool;

  /// Returns true if schema is an unconstrained string type (no enum/const restrictions).
  ///
  /// # Example
  /// ```text
  /// { "type": "string" }                    => true
  /// { "type": "string", "enum": ["a"] }     => false
  /// { "type": "string", "const": "x" }      => false
  /// ```
  fn is_freeform_string(&self) -> bool;

  /// Returns true if schema has enum values or a const constraint.
  ///
  /// # Example
  /// ```text
  /// { "enum": ["a", "b"] }  => true
  /// { "const": "x" }        => true
  /// { "type": "string" }    => false
  /// ```
  fn is_constrained(&self) -> bool;

  /// Checks if a schema matches the "relaxed enum" pattern.
  ///
  /// A relaxed enum is defined as having a freeform string variant (no enum values, no const)
  /// alongside other variants that are constrained (enum values or const).
  fn is_relaxed_enum_pattern(&self) -> bool;

  /// Infers a variant name for an inline schema in a union.
  fn infer_variant_name(&self, index: usize) -> String;

  /// Infers a union variant label from the schema, checking const value, ref name, and title.
  fn infer_union_variant_label(&self, ref_name: Option<&str>, index: usize) -> String;

  /// Infers a variant name for an object schema based on its properties.
  fn infer_object_variant_name(&self) -> String;

  /// Infers a name from the schema's required fields if exactly one exists.
  fn infer_name_from_required_fields(&self) -> Option<String>;

  /// Infers a name from the schema's $ref properties if exactly one exists.
  fn infer_name_from_ref_properties(&self) -> Option<String>;

  /// Infers a name from the schema's properties if exactly one exists.
  fn infer_name_from_single_property(&self) -> Option<String>;

  /// Infers a name for an inline schema based on its context (path, operation).
  ///
  /// Checks in order: title, single property name, path segments.
  fn infer_name_from_context(&self, path: &str, context: &str) -> String;

  /// Extracts all enum value entries from the schema with proper metadata.
  ///
  /// Handles multiple patterns:
  /// - Direct `enum` arrays: each value becomes an entry
  /// - `const` values: single entry with schema's docs/deprecated
  /// - oneOf/anyOf with const variants: per-variant metadata preserved
  /// - Relaxed enum patterns: known values extracted from constrained variants
  ///
  /// Returns entries with documentation and deprecated status from the source schema
  /// when available, enabling per-variant metadata in generated code.
  fn extract_enum_entries(&self, spec: &Spec) -> Vec<EnumValueEntry>;

  /// Returns true if the schema should be registered as an enum in the name index.
  ///
  /// This includes schemas with direct enum values and relaxed anyOf enum patterns.
  fn should_register_as_enum(&self) -> bool;
}

impl SchemaExt for ObjectSchema {
  fn is_primitive(&self) -> bool {
    self.properties.is_empty()
      && self.one_of.is_empty()
      && self.any_of.is_empty()
      && self.all_of.is_empty()
      && self.enum_values.len() <= 1
      && (self.schema_type.is_some() || self.enum_values.is_empty())
  }

  fn is_null(&self) -> bool {
    self.is_single_type(SchemaType::Null)
  }

  fn is_nullable_object(&self) -> bool {
    if self.is_null() {
      return true;
    }
    if let Some(SchemaTypeSet::Multiple(types)) = &self.schema_type {
      types.contains(&SchemaType::Null)
        && types.contains(&SchemaType::Object)
        && self.properties.is_empty()
        && self.additional_properties.is_none()
    } else {
      false
    }
  }

  fn is_array(&self) -> bool {
    self.is_single_type(SchemaType::Array)
  }

  fn is_string(&self) -> bool {
    self.is_single_type(SchemaType::String)
  }

  fn is_object(&self) -> bool {
    self.is_single_type(SchemaType::Object)
  }

  fn is_numeric(&self) -> bool {
    matches!(
      &self.schema_type,
      Some(SchemaTypeSet::Single(SchemaType::Number | SchemaType::Integer))
    )
  }

  fn is_nullable_array(&self) -> bool {
    match &self.schema_type {
      Some(SchemaTypeSet::Multiple(types)) => {
        types.len() == 2 && types.contains(&SchemaType::Array) && types.contains(&SchemaType::Null)
      }
      _ => false,
    }
  }

  fn is_single_type(&self, schema_type: SchemaType) -> bool {
    matches!(
      &self.schema_type,
      Some(SchemaTypeSet::Single(t)) if *t == schema_type
    )
  }

  fn single_type(&self) -> Option<SchemaType> {
    match &self.schema_type {
      Some(SchemaTypeSet::Single(t)) => Some(*t),
      _ => None,
    }
  }

  fn non_null_type(&self) -> Option<SchemaType> {
    match &self.schema_type {
      Some(SchemaTypeSet::Multiple(types)) if types.len() == 2 && types.contains(&SchemaType::Null) => {
        types.iter().find(|t| **t != SchemaType::Null).copied()
      }
      _ => None,
    }
  }

  fn is_inline_object(&self) -> bool {
    if !self.enum_values.is_empty() {
      return false;
    }

    if !self.one_of.is_empty() || !self.any_of.is_empty() {
      return false;
    }

    if self.is_array() {
      return false;
    }

    let is_object_type = self.single_type() == Some(SchemaType::Object) || self.schema_type.is_none();
    is_object_type && !self.properties.is_empty()
  }

  fn is_discriminated_base_type(&self) -> bool {
    self
      .discriminator
      .as_ref()
      .and_then(|d| d.mapping.as_ref().map(|m| !m.is_empty()))
      .unwrap_or(false)
      && !self.properties.is_empty()
  }

  fn is_empty_object(&self) -> bool {
    self.properties.is_empty()
      && self.one_of.is_empty()
      && self.any_of.is_empty()
      && self.all_of.is_empty()
      && self.enum_values.is_empty()
      && self.schema_type.is_none()
  }

  fn has_union(&self) -> bool {
    !self.one_of.is_empty() || !self.any_of.is_empty()
  }

  fn has_inline_union_array_items(&self, spec: &Spec) -> bool {
    if !self.is_array() {
      return false;
    }
    self.inline_array_items(spec).is_some_and(|items| items.has_union())
  }

  fn inline_array_items<'a>(&'a self, spec: &'a Spec) -> Option<ObjectSchema> {
    let items_box = self.items.as_ref()?;
    let items_schema_ref = match items_box.as_ref() {
      Schema::Object(o) => o,
      Schema::Boolean(_) => return None,
    };

    if matches!(&**items_schema_ref, ObjectOrReference::Ref { .. }) {
      return None;
    }

    items_schema_ref.resolve(spec).ok()
  }

  fn has_enum_values(&self) -> bool {
    !self.enum_values.is_empty()
  }

  fn has_inline_enum(&self, spec: &Spec) -> bool {
    self.enum_values.len() > 1
      || self
        .inline_array_items(spec)
        .is_some_and(|items| items.enum_values.len() > 1)
  }

  fn has_intersection(&self) -> bool {
    !self.all_of.is_empty()
  }

  fn requires_type_definition(&self) -> bool {
    self.has_enum_values() || self.has_union() || (!self.properties.is_empty() && self.additional_properties.is_none())
  }

  fn has_relaxed_anyof_enum(&self) -> bool {
    !self.any_of.is_empty() && has_mixed_string_variants(self.any_of.iter())
  }

  fn union_variants(&self) -> impl Iterator<Item = &ObjectOrReference<ObjectSchema>> {
    self.any_of.iter().chain(&self.one_of)
  }

  fn union_variants_with_kind(&self) -> Option<(&[ObjectOrReference<ObjectSchema>], UnionKind)> {
    let kind = UnionKind::from_schema(self);
    let variants = match kind {
      UnionKind::OneOf => &self.one_of,
      UnionKind::AnyOf => &self.any_of,
    };
    if variants.is_empty() {
      None
    } else {
      Some((variants, kind))
    }
  }

  fn has_null_variant(&self, spec: &Spec) -> bool {
    self.union_variants().any(|v| variant_is_nullable(v, spec))
  }

  fn find_non_null_variant<'a>(&'a self, spec: &Spec) -> Option<&'a ObjectOrReference<ObjectSchema>> {
    self.union_variants().find(|v| !variant_is_nullable(v, spec))
  }

  fn single_non_null_variant<'a>(&'a self, spec: &Spec) -> Option<&'a ObjectOrReference<ObjectSchema>> {
    let mut non_null_variants = self.union_variants().filter(|v| !variant_is_nullable(v, spec));
    let first = non_null_variants.next()?;
    non_null_variants.next().is_none().then_some(first)
  }

  fn has_inline_single_variant(&self, spec: &Spec) -> bool {
    self
      .single_non_null_variant(spec)
      .is_some_and(|v| extract_schema_ref_name(v).is_none())
  }

  fn single_type_or_nullable(&self) -> Option<SchemaType> {
    self.single_type().or_else(|| self.non_null_type())
  }

  fn is_string_type(&self) -> bool {
    matches!(self.single_type_or_nullable(), Some(SchemaType::String))
  }

  fn is_freeform_string(&self) -> bool {
    self.is_string_type() && self.enum_values.is_empty() && self.const_value.is_none()
  }

  fn is_constrained(&self) -> bool {
    !self.enum_values.is_empty() || self.const_value.is_some()
  }

  fn is_relaxed_enum_pattern(&self) -> bool {
    has_mixed_string_variants(self.union_variants())
  }

  fn infer_variant_name(&self, index: usize) -> String {
    if !self.enum_values.is_empty() {
      return "Enum".to_string();
    }
    if let Some(typ) = self.single_type_or_nullable() {
      return match typ {
        SchemaType::String => "String".to_string(),
        SchemaType::Number => "Number".to_string(),
        SchemaType::Integer => "Integer".to_string(),
        SchemaType::Boolean => "Boolean".to_string(),
        SchemaType::Array => "Array".to_string(),
        SchemaType::Object => self.infer_object_variant_name(),
        SchemaType::Null => "Null".to_string(),
      };
    }
    if self.schema_type.is_some() {
      return "Mixed".to_string();
    }
    let variants = if self.one_of.is_empty() {
      &self.any_of
    } else {
      &self.one_of
    };

    extract_common_variant_prefix(variants).map_or_else(|| format!("Variant{index}"), |c| c.name)
  }

  fn infer_union_variant_label(&self, ref_name: Option<&str>, index: usize) -> String {
    if let Some(const_value) = &self.const_value
      && let Ok(normalized) = NormalizedVariant::try_from(const_value)
    {
      return normalized.name;
    }

    if let Some(schema_name) = ref_name {
      return to_rust_type_name(schema_name);
    }

    if let Some(title) = &self.title {
      return to_rust_type_name(title);
    }

    self.infer_variant_name(index)
  }

  fn infer_object_variant_name(&self) -> String {
    if self.properties.is_empty() {
      return "Object".to_string();
    }

    if let Some(name) = self.infer_name_from_required_fields() {
      return name;
    }

    if let Some(name) = self.infer_name_from_ref_properties() {
      return name;
    }

    if let Some(name) = self.infer_name_from_single_property() {
      return name;
    }

    "Object".to_string()
  }

  fn infer_name_from_required_fields(&self) -> Option<String> {
    if self.required.len() == 1 {
      return Some(self.required[0].to_pascal_case());
    }
    None
  }

  fn infer_name_from_ref_properties(&self) -> Option<String> {
    let mut ref_names = self.properties.values().filter_map(extract_schema_ref_name);

    if let Some(first) = ref_names.next()
      && ref_names.next().is_none()
    {
      return Some(first.to_pascal_case());
    }

    None
  }

  fn infer_name_from_single_property(&self) -> Option<String> {
    if self.properties.len() == 1 {
      return self.properties.keys().next().map(|name| name.to_pascal_case());
    }
    None
  }

  fn infer_name_from_context(&self, path: &str, context: &str) -> String {
    let is_request = context == REQUEST_BODY_SUFFIX;

    let with_suffix = |base: &str| {
      let sanitized_base = sanitize(base);
      if is_request {
        format!("{sanitized_base}{REQUEST_BODY_SUFFIX}")
      } else {
        format!("{sanitized_base}{RESPONSE_SUFFIX}")
      }
    };

    let with_context_suffix = |base: &str| {
      let sanitized_base = sanitize(base);
      if is_request {
        format!("{sanitized_base}{REQUEST_BODY_SUFFIX}")
      } else {
        format!("{sanitized_base}{context}{RESPONSE_SUFFIX}")
      }
    };

    if let Some(title) = &self.title {
      return with_suffix(title);
    }

    if self.properties.len() == 1
      && let Some((prop_name, _)) = self.properties.iter().next()
    {
      let singular = cruet::to_singular(prop_name);
      return with_suffix(&singular);
    }

    let segments = path
      .split('/')
      .filter(|s| !s.is_empty() && !s.starts_with('{'))
      .collect::<Vec<_>>();

    segments
      .last()
      .map(|&s| with_context_suffix(&cruet::to_singular(s)))
      .or_else(|| segments.first().map(|&s| with_context_suffix(s)))
      .unwrap_or_else(|| {
        if is_request {
          REQUEST_BODY_SUFFIX.to_string()
        } else {
          format!("{RESPONSE_PREFIX}{context}")
        }
      })
  }

  fn extract_enum_entries(&self, spec: &Spec) -> Vec<EnumValueEntry> {
    if !self.enum_values.is_empty() {
      return self
        .enum_values
        .iter()
        .map(|value| EnumValueEntry {
          value: value.clone(),
          docs: Documentation::default(),
          deprecated: false,
        })
        .collect();
    }

    if let Some(const_val) = &self.const_value {
      return vec![EnumValueEntry {
        value: const_val.clone(),
        docs: Documentation::from_optional(self.description.as_ref()),
        deprecated: self.deprecated.unwrap_or(false),
      }];
    }

    self
      .union_variants()
      .filter_map(|v| v.resolve(spec).ok())
      .flat_map(|s| extract_variant_entries(&s))
      .unique_by(|e| e.value.clone())
      .collect()
  }

  fn should_register_as_enum(&self) -> bool {
    self.has_enum_values() || self.has_relaxed_anyof_enum()
  }
}

fn extract_variant_entries(schema: &ObjectSchema) -> Vec<EnumValueEntry> {
  if schema.is_freeform_string() {
    return vec![];
  }

  if let Some(const_val) = &schema.const_value {
    return vec![EnumValueEntry {
      value: const_val.clone(),
      docs: Documentation::from_optional(schema.description.as_ref()),
      deprecated: schema.deprecated.unwrap_or(false),
    }];
  }

  schema
    .enum_values
    .iter()
    .map(|value| EnumValueEntry {
      value: value.clone(),
      docs: Documentation::default(),
      deprecated: false,
    })
    .collect()
}

fn variant_is_nullable(variant: &ObjectOrReference<ObjectSchema>, spec: &Spec) -> bool {
  variant.resolve(spec).is_ok_and(|schema| schema.is_nullable_object())
}

/// Checks if variants contain both freeform strings and constrained strings.
///
/// Used to detect "relaxed enum" patterns where an API accepts known enum values
/// plus arbitrary strings for forward compatibility.
///
/// # Example
/// ```text
/// anyOf: [{ type: string }, { type: string, enum: ["a", "b"] }] => true
/// anyOf: [{ type: string, enum: ["a"] }, { type: string, enum: ["b"] }] => false
/// ```
///
pub(crate) fn has_mixed_string_variants<'a>(
  variants: impl Iterator<Item = &'a ObjectOrReference<ObjectSchema>>,
) -> bool {
  let mut has_freeform = false;
  let mut has_constrained = false;

  for v in variants {
    if let ObjectOrReference::Object(s) = v {
      if s.is_freeform_string() {
        has_freeform = true;
      } else if s.is_constrained() {
        has_constrained = true;
      }
    }

    if has_freeform && has_constrained {
      return true;
    }
  }

  false
}

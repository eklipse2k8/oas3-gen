//! Schema converter for transforming OpenAPI schemas to Rust AST
//!
//! This module handles the conversion of OpenAPI schema definitions into
//! Rust type definitions (structs, enums, type aliases) with proper validation,
//! serde attributes, and documentation.

use std::collections::BTreeSet;

use oas3::spec::{ObjectOrReference, ObjectSchema, Schema, SchemaType, SchemaTypeSet};
use regex::Regex;
use serde_json::Number;

use super::{
  SchemaGraph,
  ast::{EnumDef, FieldDef, RustType, StructDef, TypeRef, VariantContent, VariantDef},
  utils::doc_comment_lines,
};
use crate::reserved::{to_rust_field_name, to_rust_type_name};

/// Converter that transforms OpenAPI schemas into Rust AST structures
pub struct SchemaConverter<'a> {
  graph: &'a SchemaGraph,
}

impl<'a> SchemaConverter<'a> {
  pub fn new(graph: &'a SchemaGraph) -> Self {
    Self { graph }
  }

  /// Convert a schema to Rust type definitions
  /// Returns the main type and any inline types that were generated
  pub fn convert_schema(&self, name: &str, schema: &ObjectSchema) -> anyhow::Result<Vec<RustType>> {
    // Determine the type of Rust definition we need to create

    // Check if this is an allOf composition
    if !schema.all_of.is_empty() {
      return self.convert_all_of_schema(name, schema);
    }

    // Check if this is an enum (oneOf/anyOf)
    if !schema.one_of.is_empty() {
      return Ok(vec![self.convert_one_of_enum(name, schema)?]);
    }

    if !schema.any_of.is_empty() {
      return self.convert_any_of_enum(name, schema);
    }

    // Check if this is a simple enum (string with enum values)
    if !schema.enum_values.is_empty() {
      return Ok(vec![self.convert_simple_enum(name, schema, &schema.enum_values)?]);
    }

    // Check if this is a struct (object with properties)
    if !schema.properties.is_empty() {
      let (main_type, inline_types) = self.convert_struct(name, schema)?;
      let mut all_types = vec![main_type];
      all_types.extend(inline_types);
      return Ok(all_types);
    }

    // Otherwise, might be a type alias or something we can skip
    Ok(vec![])
  }

  /// Convert an allOf schema by merging all schemas into one struct
  fn convert_all_of_schema(&self, name: &str, schema: &ObjectSchema) -> anyhow::Result<Vec<RustType>> {
    // Merge all allOf schemas into a single schema
    let mut merged_schema = schema.clone();

    for all_of_ref in &schema.all_of {
      if let Ok(all_of_schema) = all_of_ref.resolve(self.graph.spec()) {
        // Merge properties
        for (prop_name, prop_ref) in &all_of_schema.properties {
          merged_schema.properties.insert(prop_name.clone(), prop_ref.clone());
        }

        // Merge required fields
        merged_schema.required.extend(all_of_schema.required.iter().cloned());
      }
    }

    // Now convert as a regular struct
    let (main_type, inline_types) = self.convert_struct(name, &merged_schema)?;
    let mut all_types = vec![main_type];
    all_types.extend(inline_types);
    Ok(all_types)
  }

  /// Convert a schema with oneOf into a Rust enum
  fn convert_one_of_enum(&self, name: &str, schema: &ObjectSchema) -> anyhow::Result<RustType> {
    let mut variants = Vec::new();
    let mut seen_names = BTreeSet::new();

    // Get discriminator property name if present
    let discriminator_property = schema.discriminator.as_ref().map(|d| d.property_name.as_str());

    for (i, variant_schema_ref) in schema.one_of.iter().enumerate() {
      if let Ok(variant_schema) = variant_schema_ref.resolve(self.graph.spec()) {
        // Skip null variants - they're handled by making the field Option<T>
        if variant_schema.schema_type == Some(SchemaTypeSet::Single(SchemaType::Null)) {
          continue;
        }

        // Generate a good variant name
        let mut variant_name = if let Some(ref title) = variant_schema.title {
          to_rust_type_name(title)
        } else {
          // Infer name from type
          self.infer_variant_name(&variant_schema, i)
        };

        // Ensure uniqueness
        if seen_names.contains(&variant_name) {
          variant_name = format!("{}{}", variant_name, i);
        }
        seen_names.insert(variant_name.clone());

        let docs = variant_schema
          .description
          .as_ref()
          .map(|d| doc_comment_lines(d))
          .unwrap_or_default();

        let deprecated = variant_schema.deprecated.unwrap_or(false);

        // Determine the variant content based on the schema type
        // For discriminated unions (with tag), we MUST use struct variants (serde requirement)
        // For non-discriminated (untagged), we can use tuple variants to avoid duplication
        let content = if discriminator_property.is_some() {
          // Has discriminator - must use struct variant for serde(tag) to work
          // serde internally-tagged enums REQUIRE all variants to be struct-like
          if !variant_schema.properties.is_empty() {
            let fields = self.convert_fields_with_exclusions(&variant_schema, discriminator_property)?;
            VariantContent::Struct(fields)
          } else {
            // Primitive or non-object - wrap in single-field struct for serde compatibility
            let type_ref = self.schema_to_type_ref(&variant_schema)?;
            let field = FieldDef {
              name: "value".to_string(),
              docs: vec![],
              rust_type: type_ref,
              optional: false,
              serde_attrs: vec![],
              validation_attrs: vec![],
              regex_validation: None,
              default_value: None,
              read_only: false,
              write_only: false,
              deprecated: false,
              multiple_of: None,
              unique_items: false,
            };
            VariantContent::Struct(vec![field])
          }
        } else {
          // No discriminator - can use tuple variants to avoid duplication
          if let Some(ref title) = variant_schema.title {
            if self.graph.get_schema(title).is_some() {
              // Reference to existing schema - use tuple variant
              let type_ref = TypeRef::new(to_rust_type_name(title));
              VariantContent::Tuple(vec![type_ref])
            } else if !variant_schema.properties.is_empty() {
              // Inline object - struct variant
              let fields = self.convert_fields(&variant_schema)?;
              VariantContent::Struct(fields)
            } else {
              // Other types - tuple variant
              let type_ref = self.schema_to_type_ref(&variant_schema)?;
              VariantContent::Tuple(vec![type_ref])
            }
          } else if !variant_schema.properties.is_empty() {
            // Anonymous object - inline struct variant
            let fields = self.convert_fields(&variant_schema)?;
            VariantContent::Struct(fields)
          } else {
            // Primitive - tuple variant
            let type_ref = self.schema_to_type_ref(&variant_schema)?;
            VariantContent::Tuple(vec![type_ref])
          }
        };

        variants.push(VariantDef {
          name: to_rust_type_name(&variant_name),
          docs,
          content,
          serde_attrs: vec![],
          deprecated,
        });
      }
    }

    // Check if there's a discriminator
    let discriminator = schema.discriminator.as_ref().map(|d| d.property_name.clone());

    Ok(RustType::Enum(EnumDef {
      name: to_rust_type_name(name),
      docs: schema
        .description
        .as_ref()
        .map(|d| doc_comment_lines(d))
        .unwrap_or_default(),
      variants,
      discriminator,
      derives: vec!["Debug".into(), "Clone".into(), "Serialize".into(), "Deserialize".into()],
      serde_attrs: vec![],
    }))
  }

  /// Convert a schema with anyOf into an untagged Rust enum
  /// May return multiple types (e.g., for catch-all enums with inner/outer structure)
  fn convert_any_of_enum(&self, name: &str, schema: &ObjectSchema) -> anyhow::Result<Vec<RustType>> {
    // Check if this is a string enum with const values pattern (common for forward-compatible enums)
    let has_freeform_string = schema.any_of.iter().any(|s| {
      if let Ok(resolved) = s.resolve(self.graph.spec()) {
        resolved.const_value.is_none() && resolved.schema_type == Some(SchemaTypeSet::Single(SchemaType::String))
      } else {
        false
      }
    });

    let const_values: Vec<_> = schema
      .any_of
      .iter()
      .filter_map(|s| {
        if let Ok(resolved) = s.resolve(self.graph.spec()) {
          resolved.const_value.as_ref().map(|v| {
            (
              v.clone(),
              resolved.description.clone(),
              resolved.deprecated.unwrap_or(false),
            )
          })
        } else {
          None
        }
      })
      .collect();

    // Special case: freeform string + const values = forward-compatible enum
    // Returns multiple types (inner Known enum + outer untagged wrapper)
    if has_freeform_string && !const_values.is_empty() {
      return self.convert_string_enum_with_catchall(name, schema, &const_values);
    }

    // Otherwise, treat as a regular untagged enum
    let mut variants = Vec::new();
    let mut seen_names = BTreeSet::new();

    for (i, variant_schema_ref) in schema.any_of.iter().enumerate() {
      if let Ok(variant_schema) = variant_schema_ref.resolve(self.graph.spec()) {
        // Skip null variants - they're handled by making the field Option<T>
        if variant_schema.schema_type == Some(SchemaTypeSet::Single(SchemaType::Null)) {
          continue;
        }

        // Generate a good variant name
        let mut variant_name = if let Some(ref title) = variant_schema.title {
          to_rust_type_name(title)
        } else {
          // Infer name from type
          self.infer_variant_name(&variant_schema, i)
        };

        // Ensure uniqueness
        if seen_names.contains(&variant_name) {
          variant_name = format!("{}{}", variant_name, i);
        }
        seen_names.insert(variant_name.clone());

        let docs = variant_schema
          .description
          .as_ref()
          .map(|d| doc_comment_lines(d))
          .unwrap_or_default();

        let deprecated = variant_schema.deprecated.unwrap_or(false);

        // Determine variant content - prefer tuple variants for existing schemas
        let content = if let Some(ref title) = variant_schema.title {
          // If this variant has a title and matches an existing schema, use tuple variant
          if self.graph.get_schema(title).is_some() {
            let type_ref = TypeRef::new(to_rust_type_name(title));
            VariantContent::Tuple(vec![type_ref])
          } else if !variant_schema.properties.is_empty() {
            // Inline object without matching schema - create struct variant
            let fields = self.convert_fields(&variant_schema)?;
            VariantContent::Struct(fields)
          } else {
            // Other types - tuple variant
            let type_ref = self.schema_to_type_ref(&variant_schema)?;
            VariantContent::Tuple(vec![type_ref])
          }
        } else if !variant_schema.properties.is_empty() {
          // Anonymous object - create inline struct variant
          let fields = self.convert_fields(&variant_schema)?;
          VariantContent::Struct(fields)
        } else {
          // Not an object - create tuple variant wrapping the type
          let type_ref = self.schema_to_type_ref(&variant_schema)?;
          VariantContent::Tuple(vec![type_ref])
        };

        variants.push(VariantDef {
          name: to_rust_type_name(&variant_name),
          docs,
          content,
          serde_attrs: vec![],
          deprecated,
        });
      }
    }

    let enum_name = to_rust_type_name(name);

    // Fix self-referential fields in variants by adding Box wrapping
    for variant in &mut variants {
      if let VariantContent::Struct(ref mut fields) = variant.content {
        for field in fields {
          if field.rust_type.base_type == enum_name && !field.rust_type.boxed {
            field.rust_type = field.rust_type.clone().with_boxed();
          }
        }
      }
    }

    Ok(vec![RustType::Enum(EnumDef {
      name: enum_name,
      docs: schema
        .description
        .as_ref()
        .map(|d| doc_comment_lines(d))
        .unwrap_or_default(),
      variants,
      discriminator: None,
      derives: vec!["Debug".into(), "Clone".into(), "Serialize".into(), "Deserialize".into()],
      serde_attrs: vec!["untagged".into()],
    })])
  }

  /// Convert a string enum with const values + a catch-all for unknown strings
  /// This generates TWO enums:
  /// 1. Inner "Known" enum with unit variants for known const values
  /// 2. Outer untagged enum with Known(InnerEnum) + Other(String) variants
  fn convert_string_enum_with_catchall(
    &self,
    name: &str,
    schema: &ObjectSchema,
    const_values: &[(serde_json::Value, Option<String>, bool)],
  ) -> anyhow::Result<Vec<RustType>> {
    let base_name = to_rust_type_name(name);
    let known_name = format!("{}Known", base_name);

    // Create inner enum with known values (simple unit enum)
    let mut known_variants = Vec::new();
    let mut seen_names = BTreeSet::new();

    for (i, (value, description, deprecated)) in const_values.iter().enumerate() {
      if let Some(str_val) = value.as_str() {
        let mut variant_name = to_rust_type_name(str_val);

        if seen_names.contains(&variant_name) {
          variant_name = format!("{}{}", variant_name, i);
        }
        seen_names.insert(variant_name.clone());

        let docs = description.as_ref().map(|d| doc_comment_lines(d)).unwrap_or_default();

        known_variants.push(VariantDef {
          name: variant_name,
          docs,
          content: VariantContent::Unit,
          serde_attrs: vec![format!("rename = \"{}\"", str_val)],
          deprecated: *deprecated,
        });
      }
    }

    let inner_enum = RustType::Enum(EnumDef {
      name: known_name.clone(),
      docs: vec!["/// Known string values".to_string()],
      variants: known_variants,
      discriminator: None,
      derives: vec![
        "Debug".into(),
        "Clone".into(),
        "PartialEq".into(),
        "Eq".into(),
        "Serialize".into(),
        "Deserialize".into(),
      ],
      serde_attrs: vec![],
    });

    // Create outer untagged enum that wraps the known enum + Other variant
    let outer_variants = vec![
      VariantDef {
        name: "Known".to_string(),
        docs: vec!["/// A known string value".to_string()],
        content: VariantContent::Tuple(vec![TypeRef::new(&known_name)]),
        serde_attrs: vec![],
        deprecated: false,
      },
      VariantDef {
        name: "Other".to_string(),
        docs: vec!["/// An unknown string value not in the known set".to_string()],
        content: VariantContent::Tuple(vec![TypeRef::new("String")]),
        serde_attrs: vec![],
        deprecated: false,
      },
    ];

    let outer_enum = RustType::Enum(EnumDef {
      name: base_name,
      docs: schema
        .description
        .as_ref()
        .map(|d| doc_comment_lines(d))
        .unwrap_or_default(),
      variants: outer_variants,
      discriminator: None,
      derives: vec![
        "Debug".into(),
        "Clone".into(),
        "PartialEq".into(),
        "Eq".into(),
        "Serialize".into(),
        "Deserialize".into(),
      ],
      serde_attrs: vec!["untagged".into()],
    });

    // Return both enums: inner first (must be defined before outer references it)
    Ok(vec![inner_enum, outer_enum])
  }

  /// Infer a variant name from the schema type
  fn infer_variant_name(&self, schema: &ObjectSchema, index: usize) -> String {
    // Check if it's an enum
    if !schema.enum_values.is_empty() {
      return "Enum".to_string();
    }

    // Check the schema type
    if let Some(ref schema_type) = schema.schema_type {
      match schema_type {
        SchemaTypeSet::Single(typ) => match typ {
          SchemaType::String => "String".to_string(),
          SchemaType::Number => "Number".to_string(),
          SchemaType::Integer => "Integer".to_string(),
          SchemaType::Boolean => "Boolean".to_string(),
          SchemaType::Array => "Array".to_string(),
          SchemaType::Object => "Object".to_string(),
          SchemaType::Null => "Null".to_string(),
        },
        SchemaTypeSet::Multiple(_) => "Mixed".to_string(),
      }
    } else {
      // Fallback
      format!("Variant{}", index)
    }
  }

  /// Convert a simple string enum
  fn convert_simple_enum(
    &self,
    name: &str,
    schema: &ObjectSchema,
    enum_values: &[serde_json::Value],
  ) -> anyhow::Result<RustType> {
    let mut variants = Vec::new();
    let mut seen_names = BTreeSet::new();

    for (i, value) in enum_values.iter().enumerate() {
      if let Some(str_val) = value.as_str() {
        let mut variant_name = to_rust_type_name(str_val);

        // Ensure uniqueness - append index if needed
        if seen_names.contains(&variant_name) {
          variant_name = format!("{}{}", variant_name, i);
        }
        seen_names.insert(variant_name.clone());

        variants.push(VariantDef {
          name: variant_name,
          docs: vec![],
          content: VariantContent::Unit,
          serde_attrs: vec![format!("rename = \"{}\"", str_val)],
          deprecated: false,
        });
      }
    }

    Ok(RustType::Enum(EnumDef {
      name: to_rust_type_name(name),
      docs: schema
        .description
        .as_ref()
        .map(|d| doc_comment_lines(d))
        .unwrap_or_default(),
      variants,
      discriminator: None,
      derives: vec!["Debug".into(), "Clone".into(), "Serialize".into(), "Deserialize".into()],
      serde_attrs: vec![],
    }))
  }

  /// Convert an object schema to a Rust struct
  /// Returns the struct and any inline types that were generated
  pub fn convert_struct(&self, name: &str, schema: &ObjectSchema) -> anyhow::Result<(RustType, Vec<RustType>)> {
    let (mut fields, inline_types) = self.convert_fields_with_inline_types(name, schema)?;

    // Individual rename attributes are more explicit and handle all edge cases correctly
    let mut serde_attrs = vec![];

    // Handle additionalProperties
    if let Some(ref additional) = schema.additional_properties {
      match additional {
        Schema::Boolean(bool_schema) => {
          if !bool_schema.0 {
            // additionalProperties: false -> deny unknown fields
            serde_attrs.push("deny_unknown_fields".to_string());
          }
          // additionalProperties: true is the default, no action needed
        }
        Schema::Object(schema_ref) => {
          // additionalProperties with schema -> add HashMap field
          if let Ok(additional_schema) = schema_ref.resolve(self.graph.spec()) {
            let value_type = self.schema_to_type_ref(&additional_schema)?;
            let map_type = TypeRef::new(format!(
              "std::collections::HashMap<String, {}>",
              value_type.to_rust_type()
            ));

            fields.push(FieldDef {
              name: "additional_properties".to_string(),
              docs: vec!["/// Additional properties not defined in the schema".to_string()],
              rust_type: map_type,
              optional: false,
              serde_attrs: vec!["flatten".to_string()],
              validation_attrs: vec![],
              regex_validation: None,
              default_value: None,
              read_only: false,
              write_only: false,
              deprecated: false,
              multiple_of: None,
              unique_items: false,
            });
          }
        }
      }
    }

    // Only add serde(default) at struct level if ALL fields have defaults or are Option/Vec
    // Otherwise we get compilation errors when trying to Default::default() complex types
    let all_fields_defaultable = fields.iter().all(|f| {
      f.default_value.is_some()
        || f.rust_type.nullable
        || f.rust_type.is_array
        || matches!(
          f.rust_type.base_type.as_str(),
          "String"
            | "bool"
            | "i8"
            | "i16"
            | "i32"
            | "i64"
            | "i128"
            | "isize"
            | "u8"
            | "u16"
            | "u32"
            | "u64"
            | "u128"
            | "usize"
            | "f32"
            | "f64"
            | "serde_json::Value"
        )
    });

    if all_fields_defaultable && fields.iter().any(|f| f.default_value.is_some()) {
      serde_attrs.push("default".to_string());
    }

    // Optimize derives based on field directionality
    let all_read_only = !fields.is_empty() && fields.iter().all(|f| f.read_only);
    let all_write_only = !fields.is_empty() && fields.iter().all(|f| f.write_only);

    let mut derives = vec!["Debug".into(), "Clone".into()];

    // Add Serialize/Deserialize based on field directionality
    if !all_read_only {
      // Include Serialize unless ALL fields are read-only (response-only)
      derives.push("Serialize".into());
    }

    if !all_write_only {
      // Include Deserialize unless ALL fields are write-only (request-only)
      derives.push("Deserialize".into());
    }

    // Always include Validate for runtime validation
    derives.push("Validate".into());

    let struct_type = RustType::Struct(StructDef {
      name: to_rust_type_name(name),
      docs: schema
        .description
        .as_ref()
        .map(|d| doc_comment_lines(d))
        .unwrap_or_default(),
      fields,
      derives,
      serde_attrs,
    });

    Ok((struct_type, inline_types))
  }

  pub fn extract_validation_pattern<'s>(&self, prop_name: &str, schema: &'s ObjectSchema) -> Option<&'s String> {
    match (schema.schema_type.as_ref(), schema.pattern.as_ref()) {
      (Some(SchemaTypeSet::Single(SchemaType::String)), Some(pattern)) => {
        if Regex::new(pattern).is_ok() {
          Some(pattern)
        } else {
          eprintln!(
            "Warning: Invalid regex pattern '{}' for property '{}'",
            pattern, prop_name
          );
          None
        }
      }
      _ => None,
    }
  }

  fn render_number(is_float: bool, num: &Number) -> String {
    if is_float {
      if num.to_string().contains(".") {
        num.to_string()
      } else {
        format!("{}.0", num)
      }
    } else {
      format!("{}i64", num.as_i64().unwrap_or_default())
    }
  }

  /// Extract validation attributes from an OpenAPI schema
  pub fn extract_validation_attrs(&self, _prop_name: &str, is_required: bool, schema: &ObjectSchema) -> Vec<String> {
    let mut attrs = Vec::new();

    // Handle format-based validation
    if let Some(ref format) = schema.format {
      match format.as_str() {
        "email" => attrs.push("email".to_string()),
        "uri" | "url" => attrs.push("url".to_string()),
        _ => {}
      }
    }

    if let Some(ref schema_type) = schema.schema_type {
      if matches!(
        schema_type,
        SchemaTypeSet::Single(SchemaType::Number) | SchemaTypeSet::Single(SchemaType::Integer)
      ) {
        let mut parts = Vec::<String>::new();
        let is_float = matches!(schema_type, SchemaTypeSet::Single(SchemaType::Number));

        // multipleOf validation constraint
        // Note: validator crate doesn't have built-in support for multipleOf
        // We document this in field comments for manual validation
        if schema.multiple_of.is_some() {
          // multipleOf is tracked in FieldDef and documented in generated code
        }

        // exclusive_minimum
        if let Some(exclusive_min) = schema
          .exclusive_minimum
          .as_ref()
          .map(|v| format!("exclusive_min = {}", Self::render_number(is_float, v)))
        {
          parts.push(exclusive_min);
        }

        // exclusive_maximum
        if let Some(exclusive_max) = schema
          .exclusive_maximum
          .as_ref()
          .map(|v| format!("exclusive_max = {}", Self::render_number(is_float, v)))
        {
          parts.push(exclusive_max);
        }

        // minimum
        if let Some(min) = schema
          .minimum
          .as_ref()
          .map(|v| format!("min = {}", Self::render_number(is_float, v)))
        {
          parts.push(min);
        }

        // maximum
        if let Some(max) = schema
          .maximum
          .as_ref()
          .map(|v| format!("max = {}", Self::render_number(is_float, v)))
        {
          parts.push(max);
        }

        if !parts.is_empty() {
          attrs.push(format!("range({})", parts.join(", ")));
        }
      }

      // string length validation (skip for date/time/binary/uuid formats as they map to non-string types)
      if matches!(schema_type, SchemaTypeSet::Single(SchemaType::String)) && schema.enum_values.is_empty() {
        let is_non_string_format = schema
          .format
          .as_ref()
          .map(|f| matches!(f.as_str(), "date" | "date-time" | "time" | "binary" | "byte" | "uuid"))
          .unwrap_or(false);

        if !is_non_string_format {
          if let (Some(min), Some(max)) = (schema.min_length, schema.max_length) {
            attrs.push(format!("length(min = {min}, max = {max})"));
          } else if let Some(min) = schema.min_length {
            attrs.push(format!("length(min = {min})"));
          } else if let Some(max) = schema.max_length {
            attrs.push(format!("length(max = {max})"));
          } else if is_required {
            // Require non-empty string for required fields
            attrs.push("length(min = 1)".to_string());
          }
        }
      }

      // array length validation
      if matches!(schema_type, SchemaTypeSet::Single(SchemaType::Array)) {
        if let (Some(min), Some(max)) = (schema.min_items, schema.max_items) {
          attrs.push(format!("length(min = {min}, max = {max})"));
        } else if let Some(min) = schema.min_items {
          attrs.push(format!("length(min = {min})"));
        } else if let Some(max) = schema.max_items {
          attrs.push(format!("length(max = {max})"));
        }
      }
    }

    attrs
  }

  /// Extract default value from an OpenAPI schema
  pub fn extract_default_value(&self, schema: &ObjectSchema) -> Option<serde_json::Value> {
    schema.default.clone()
  }

  /// Convert schema properties to struct fields, excluding specified field names
  fn convert_fields_with_exclusions(
    &self,
    schema: &ObjectSchema,
    exclude_field: Option<&str>,
  ) -> anyhow::Result<Vec<FieldDef>> {
    let mut fields = Vec::new();

    let mut properties: Vec<_> = schema.properties.iter().collect();
    properties.sort_by(|(a, _), (b, _)| a.cmp(b));

    for (prop_name, prop_schema_ref) in properties {
      // Skip excluded fields (e.g., discriminator fields)
      if let Some(exclude) = exclude_field
        && prop_name == exclude
      {
        continue;
      }

      // Check if this is a direct $ref first
      let rust_type = if let ObjectOrReference::Ref { ref_path, .. } = prop_schema_ref {
        // Extract type name directly from the reference
        if let Some(ref_name) = SchemaGraph::extract_ref_name(ref_path) {
          TypeRef::new(to_rust_type_name(&ref_name))
        } else {
          // Fallback to resolution
          if let Ok(prop_schema) = prop_schema_ref.resolve(self.graph.spec()) {
            self.schema_to_type_ref(&prop_schema)?
          } else {
            TypeRef::new("serde_json::Value")
          }
        }
      } else {
        // Inline schema - resolve and convert
        if let Ok(prop_schema) = prop_schema_ref.resolve(self.graph.spec()) {
          self.schema_to_type_ref(&prop_schema)?
        } else {
          TypeRef::new("serde_json::Value")
        }
      };

      let is_required = schema.required.contains(prop_name);
      let optional = !is_required;

      let mut serde_attrs = vec![];
      // Add rename if the Rust field name differs from the original OpenAPI property name
      // This automatically handles: keywords (type -> r#type), special chars (user-id -> user_id), case changes (userId -> user_id)
      let rust_field_name = to_rust_field_name(prop_name);
      if rust_field_name != *prop_name {
        serde_attrs.push(format!("rename = \"{}\"", prop_name));
      }

      // Add skip_serializing_if for optional fields or nullable types
      if optional || rust_type.nullable {
        serde_attrs.push("skip_serializing_if = \"Option::is_none\"".to_string());
      }

      // Extract validation attributes, default value, and read/write metadata from resolved schema
      let (
        docs,
        validation_attrs,
        regex_validation,
        default_value,
        read_only,
        write_only,
        deprecated,
        multiple_of,
        unique_items,
      ) = if let Ok(prop_schema) = prop_schema_ref.resolve(self.graph.spec()) {
        let docs = prop_schema
          .description
          .as_ref()
          .map(|d| doc_comment_lines(d))
          .unwrap_or_default();
        let validation = self.extract_validation_attrs(prop_name, is_required, &prop_schema);
        let regex_validation = self.extract_validation_pattern(prop_name, &prop_schema);
        let default = self.extract_default_value(&prop_schema);
        let read_only = prop_schema.read_only.unwrap_or(false);
        let write_only = prop_schema.write_only.unwrap_or(false);
        let deprecated = prop_schema.deprecated.unwrap_or(false);
        let multiple_of = prop_schema.multiple_of.clone();
        let unique_items = prop_schema.unique_items.unwrap_or(false);
        (
          docs,
          validation,
          regex_validation.cloned(),
          default,
          read_only,
          write_only,
          deprecated,
          multiple_of,
          unique_items,
        )
      } else {
        (vec![], vec![], None, None, false, false, false, None, false)
      };

      // Skip regex validation for types that don't support it (chrono, uuid types)
      let regex_validation = match rust_type.base_type.as_str() {
        "chrono::DateTime<chrono::Utc>" | "chrono::NaiveDate" | "chrono::NaiveTime" | "uuid::Uuid" => None,
        _ => regex_validation,
      };

      // Check nullable before moving rust_type
      let is_nullable = rust_type.nullable;

      // Don't double-wrap: if the type is already nullable, don't wrap again
      let final_type = if optional && !is_nullable {
        rust_type.with_option()
      } else {
        rust_type
      };

      fields.push(FieldDef {
        name: to_rust_field_name(prop_name),
        docs,
        rust_type: final_type,
        optional: optional || is_nullable,
        serde_attrs,
        validation_attrs,
        regex_validation,
        default_value,
        read_only,
        write_only,
        deprecated,
        multiple_of,
        unique_items,
      });
    }

    Ok(fields)
  }

  /// Convert schema properties to struct fields (convenience wrapper)
  fn convert_fields(&self, schema: &ObjectSchema) -> anyhow::Result<Vec<FieldDef>> {
    self.convert_fields_with_exclusions(schema, None)
  }

  /// Convert schema properties to struct fields, generating inline enum types for anyOf unions
  fn convert_fields_with_inline_types(
    &self,
    parent_name: &str,
    schema: &ObjectSchema,
  ) -> anyhow::Result<(Vec<FieldDef>, Vec<RustType>)> {
    let mut fields = Vec::new();
    let mut inline_types = Vec::new();

    // Keep parent name unconverted - conversion will happen when creating the enum
    // This ensures consistent case handling for compound names

    let mut properties: Vec<_> = schema.properties.iter().collect();
    properties.sort_by(|(a, _), (b, _)| a.cmp(b));

    for (prop_name, prop_schema_ref) in properties {
      // Check if this is a direct $ref first
      let rust_type = if let ObjectOrReference::Ref { ref_path, .. } = prop_schema_ref {
        // Extract type name directly from the reference
        if let Some(ref_name) = SchemaGraph::extract_ref_name(ref_path) {
          TypeRef::new(to_rust_type_name(&ref_name))
        } else {
          // Fallback to resolution
          if let Ok(prop_schema) = prop_schema_ref.resolve(self.graph.spec()) {
            self.schema_to_type_ref(&prop_schema)?
          } else {
            TypeRef::new("serde_json::Value")
          }
        }
      } else {
        // Inline schema - resolve and convert
        if let Ok(prop_schema) = prop_schema_ref.resolve(self.graph.spec()) {
          // Special handling for inline anyOf unions
          // Check if this is just a nullable pattern (anyOf with null)
          let has_null = prop_schema.any_of.iter().any(|v| {
            if let Ok(resolved) = v.resolve(self.graph.spec()) {
              resolved.schema_type == Some(SchemaTypeSet::Single(SchemaType::Null))
            } else {
              false
            }
          });

          // If anyOf has null and exactly 2 variants, it's just an optional type

          if !prop_schema.any_of.is_empty() && has_null && prop_schema.any_of.len() == 2 {
            // Extract the non-null type and wrap in Option (since it's nullable)
            let mut found_type = None;
            for variant_ref in &prop_schema.any_of {
              // Check if it's a $ref first (before resolving)
              if let ObjectOrReference::Ref { ref_path, .. } = variant_ref {
                if let Some(ref_name) = SchemaGraph::extract_ref_name(ref_path) {
                  found_type = Some(TypeRef::new(to_rust_type_name(&ref_name)).with_option());
                  break;
                }
              } else if let Ok(resolved) = variant_ref.resolve(self.graph.spec())
                && resolved.schema_type != Some(SchemaTypeSet::Single(SchemaType::Null))
              {
                // Found the actual type - wrap in Option since this is a nullable pattern
                found_type = Some(self.schema_to_type_ref(&resolved)?.with_option());
                break;
              }
            }
            // Use found type or fallback
            found_type.unwrap_or_else(|| self.schema_to_type_ref(&prop_schema).unwrap().with_option())
          } else if !prop_schema.any_of.is_empty()
            && (prop_schema.title.is_none()
              || prop_schema
                .title
                .as_ref()
                .map(|t| self.graph.get_schema(t).is_none())
                .unwrap_or(true))
          {
            // Generate inline enum for non-nullable anyOf unions
            // Create compound name from parent and property - conversion happens in convert_any_of_enum
            // May return multiple types (e.g., inner Known enum + outer wrapper for catch-all enums)
            let enum_name = format!("{}.{}", parent_name, prop_name);
            let enum_types = self.convert_any_of_enum(&enum_name, &prop_schema)?;
            // Get the converted name from the last type (outer/main enum)
            let type_name = if let Some(RustType::Enum(enum_def)) = enum_types.last() {
              enum_def.name.clone()
            } else {
              to_rust_type_name(&enum_name)
            };
            // Add all generated types to inline_types
            inline_types.extend(enum_types);
            TypeRef::new(&type_name)
          } else {
            self.schema_to_type_ref(&prop_schema)?
          }
        } else {
          TypeRef::new("serde_json::Value")
        }
      };

      let is_required = schema.required.contains(prop_name);
      let optional = !is_required;

      let mut serde_attrs = vec![];
      // Add rename if the Rust field name differs from the original OpenAPI property name
      // This automatically handles: keywords (type -> r#type), special chars (user-id -> user_id), case changes (userId -> user_id)
      let rust_field_name = to_rust_field_name(prop_name);
      if rust_field_name != *prop_name {
        serde_attrs.push(format!("rename = \"{}\"", prop_name));
      }

      // Add skip_serializing_if for optional fields or nullable types
      if optional || rust_type.nullable {
        serde_attrs.push("skip_serializing_if = \"Option::is_none\"".to_string());
      }

      // Extract validation attributes, default value, and read/write metadata from resolved schema
      let (
        docs,
        validation_attrs,
        regex_validation,
        default_value,
        read_only,
        write_only,
        deprecated,
        multiple_of,
        unique_items,
      ) = if let Ok(prop_schema) = prop_schema_ref.resolve(self.graph.spec()) {
        let docs = prop_schema
          .description
          .as_ref()
          .map(|d| doc_comment_lines(d))
          .unwrap_or_default();
        let required = schema.required.contains(prop_name);
        let validation = self.extract_validation_attrs(prop_name, required, &prop_schema);
        let regex_validation = self.extract_validation_pattern(prop_name, &prop_schema);
        let default = self.extract_default_value(&prop_schema);
        let read_only = prop_schema.read_only.unwrap_or(false);
        let write_only = prop_schema.write_only.unwrap_or(false);
        let deprecated = prop_schema.deprecated.unwrap_or(false);
        let multiple_of = prop_schema.multiple_of.clone();
        let unique_items = prop_schema.unique_items.unwrap_or(false);
        (
          docs,
          validation,
          regex_validation.cloned(),
          default,
          read_only,
          write_only,
          deprecated,
          multiple_of,
          unique_items,
        )
      } else {
        (vec![], vec![], None, None, false, false, false, None, false)
      };

      // Skip regex validation for types that don't support it (chrono, uuid types)
      let regex_validation = match rust_type.base_type.as_str() {
        "chrono::DateTime<chrono::Utc>" | "chrono::NaiveDate" | "chrono::NaiveTime" | "uuid::Uuid" => None,
        _ => regex_validation,
      };

      // Check nullable before moving rust_type
      let is_nullable = rust_type.nullable;

      // Don't double-wrap: if the type is already nullable, don't wrap again
      let final_type = if optional && !is_nullable {
        rust_type.with_option()
      } else {
        rust_type
      };

      fields.push(FieldDef {
        name: to_rust_field_name(prop_name),
        docs,
        rust_type: final_type,
        optional: optional || is_nullable,
        serde_attrs,
        validation_attrs,
        regex_validation,
        default_value,
        read_only,
        write_only,
        deprecated,
        multiple_of,
        unique_items,
      });
    }

    Ok((fields, inline_types))
  }

  /// Convert an OpenAPI schema to a TypeRef (exposed for OperationConverter)
  pub fn schema_to_type_ref(&self, schema: &ObjectSchema) -> anyhow::Result<TypeRef> {
    // First priority: Check the schema type - if it has a concrete type, use that
    // This prevents title conflicts (e.g., a string field titled "Message" being confused with Message struct)
    if let Some(ref schema_type) = schema.schema_type {
      // If it has a concrete type AND properties/oneOf/anyOf, it might be a complex type
      // Only use title-based lookup for objects without explicit primitive types
      if !matches!(schema_type, SchemaTypeSet::Single(SchemaType::Object)) {
        // It's a primitive type - continue to primitive type handling below
      } else if let Some(ref title) = schema.title
        && self.graph.get_schema(title).is_some()
        && !schema.properties.is_empty()
      {
        // It's an object with a title that matches a schema and has properties
        let is_cyclic = self.graph.is_cyclic(title);
        let mut type_ref = TypeRef::new(to_rust_type_name(title));
        if is_cyclic {
          type_ref = type_ref.with_boxed();
        }
        return Ok(type_ref);
      }
    } else if let Some(ref title) = schema.title
      && self.graph.get_schema(title).is_some()
      && !schema.properties.is_empty()
    {
      // No explicit type, but has title matching a schema and has properties - likely a reference
      let is_cyclic = self.graph.is_cyclic(title);
      let mut type_ref = TypeRef::new(to_rust_type_name(title));
      if is_cyclic {
        type_ref = type_ref.with_boxed();
      }
      return Ok(type_ref);
    }

    // Check for inline oneOf/anyOf - detect nullable pattern
    if !schema.one_of.is_empty() || !schema.any_of.is_empty() {
      let variants = if !schema.one_of.is_empty() {
        &schema.one_of
      } else {
        &schema.any_of
      };

      // Check if this is the nullable pattern: anyOf/oneOf with [T, null]
      let has_null = variants.iter().any(|v| {
        if let Ok(resolved) = v.resolve(self.graph.spec()) {
          resolved.schema_type == Some(SchemaTypeSet::Single(SchemaType::Null))
        } else {
          false
        }
      });

      if has_null && variants.len() == 2 {
        // This is a nullable type - extract the non-null variant
        for variant_ref in variants {
          // Check if it's a direct $ref first
          if let ObjectOrReference::Ref { ref_path, .. } = variant_ref
            && let Some(ref_name) = SchemaGraph::extract_ref_name(ref_path)
          {
            return Ok(TypeRef::new(to_rust_type_name(&ref_name)).with_option());
          }

          // Otherwise resolve
          if let Ok(resolved) = variant_ref.resolve(self.graph.spec()) {
            // Skip null types
            if resolved.schema_type == Some(SchemaTypeSet::Single(SchemaType::Null)) {
              continue;
            }

            // Found the actual type - recurse to get it
            let inner_type = self.schema_to_type_ref(&resolved)?;
            return Ok(inner_type.with_option());
          }
        }
      }

      // Try to extract type from the first non-null, non-string variant (for non-nullable unions)
      // Prefer complex types (arrays, objects) over simple types (strings)
      let mut fallback_type: Option<TypeRef> = None;

      for variant_ref in variants {
        // Check if it's a direct $ref
        if let ObjectOrReference::Ref { ref_path, .. } = variant_ref
          && let Some(ref_name) = SchemaGraph::extract_ref_name(ref_path)
        {
          return Ok(TypeRef::new(to_rust_type_name(&ref_name)));
        }

        // Try resolving
        if let Ok(resolved) = variant_ref.resolve(self.graph.spec()) {
          // Skip null types
          if resolved.schema_type == Some(SchemaTypeSet::Single(SchemaType::Null)) {
            continue;
          }

          // Handle array types specially
          if resolved.schema_type == Some(SchemaTypeSet::Single(SchemaType::Array)) {
            let unique_items = resolved.unique_items.unwrap_or(false);
            // Check array items for oneOf
            if let Some(ref items_box) = resolved.items
              && let Schema::Object(items_ref) = items_box.as_ref()
              && let Ok(items_schema) = items_ref.resolve(self.graph.spec())
            {
              // Items have oneOf - extract first ref
              if !items_schema.one_of.is_empty() {
                for one_of_ref in &items_schema.one_of {
                  if let ObjectOrReference::Ref { ref_path, .. } = one_of_ref
                    && let Some(ref_name) = SchemaGraph::extract_ref_name(ref_path)
                  {
                    return Ok(
                      TypeRef::new(to_rust_type_name(&ref_name))
                        .with_vec()
                        .with_unique_items(unique_items),
                    );
                  }
                }
              }
            }
          }

          // Save string types as fallback but prefer arrays/objects
          if resolved.schema_type == Some(SchemaTypeSet::Single(SchemaType::String)) && fallback_type.is_none() {
            fallback_type = Some(TypeRef::new("String"));
            continue;
          }

          // Check for nested oneOf (common pattern)
          if !resolved.one_of.is_empty() {
            for nested_ref in &resolved.one_of {
              if let ObjectOrReference::Ref { ref_path, .. } = nested_ref
                && let Some(ref_name) = SchemaGraph::extract_ref_name(ref_path)
              {
                return Ok(TypeRef::new(to_rust_type_name(&ref_name)));
              }
            }
          }

          // Use title if available
          if let Some(ref variant_title) = resolved.title
            && self.graph.get_schema(variant_title).is_some()
          {
            return Ok(TypeRef::new(to_rust_type_name(variant_title)));
          }
        }
      }

      // Use fallback if we found one
      if let Some(t) = fallback_type {
        return Ok(t);
      }

      // Fall through if we couldn't resolve to a concrete type
    }

    // Check schema type for primitives
    // This handles inline primitive types
    if let Some(ref schema_type) = schema.schema_type {
      match schema_type {
        SchemaTypeSet::Single(typ) => {
          let base_type = match typ {
            SchemaType::String => {
              // Check for format field to handle special string types
              if let Some(ref format) = schema.format {
                match format.as_str() {
                  "date" => "chrono::NaiveDate",
                  "date-time" => "chrono::DateTime<chrono::Utc>",
                  "time" => "chrono::NaiveTime",
                  "binary" => "Vec<u8>",  // Raw binary (multipart/form-data)
                  "byte" => "String",     // Base64-encoded binary (JSON)
                  "uuid" => "uuid::Uuid", // UUID
                  _ => "String",
                }
              } else {
                "String"
              }
            }
            SchemaType::Number => "f64",
            SchemaType::Integer => "i64",
            SchemaType::Boolean => "bool",
            SchemaType::Array => {
              // Handle array items
              let unique_items = schema.unique_items.unwrap_or(false);

              if let Some(ref items_box) = schema.items
                && let Schema::Object(items_ref) = items_box.as_ref()
              {
                // Check if this is a $ref first
                if let ObjectOrReference::Ref { ref_path, .. } = items_ref.as_ref() {
                  // Extract the type name from the reference
                  if let Some(ref_name) = SchemaGraph::extract_ref_name(ref_path) {
                    return Ok(
                      TypeRef::new(to_rust_type_name(&ref_name))
                        .with_vec()
                        .with_unique_items(unique_items),
                    );
                  }
                }

                // Otherwise resolve and check for oneOf/anyOf in items
                if let Ok(items_schema) = items_ref.resolve(self.graph.spec()) {
                  // If items have oneOf, extract the first ref type
                  if !items_schema.one_of.is_empty() {
                    for one_of_ref in &items_schema.one_of {
                      if let ObjectOrReference::Ref { ref_path, .. } = one_of_ref
                        && let Some(ref_name) = SchemaGraph::extract_ref_name(ref_path)
                      {
                        return Ok(
                          TypeRef::new(to_rust_type_name(&ref_name))
                            .with_vec()
                            .with_unique_items(unique_items),
                        );
                      }
                    }
                  }

                  // Regular item type conversion
                  let item_type = self.schema_to_type_ref(&items_schema)?;
                  return Ok(
                    TypeRef::new(item_type.to_rust_type())
                      .with_vec()
                      .with_unique_items(unique_items),
                  );
                }
              }
              return Ok(
                TypeRef::new("serde_json::Value")
                  .with_vec()
                  .with_unique_items(unique_items),
              );
            }
            SchemaType::Object => {
              // Object without a matching schema reference
              return Ok(TypeRef::new("serde_json::Value"));
            }
            SchemaType::Null => {
              return Ok(TypeRef::new("()").with_option());
            }
          };
          return Ok(TypeRef::new(base_type));
        }
        SchemaTypeSet::Multiple(_) => {
          // Handle nullable types - check if it's a simple nullable pattern
          return Ok(TypeRef::new("serde_json::Value"));
        }
      }
    }

    // Default to serde_json::Value for schemas without type or title
    Ok(TypeRef::new("serde_json::Value"))
  }
}

#[cfg(test)]
mod tests {
  use std::collections::BTreeMap;

  use oas3::spec::{Discriminator, Spec};
  use serde_json::json;

  use super::*;

  /// Helper to create a minimal OpenAPI spec with given schemas
  fn create_test_spec(schemas: BTreeMap<String, ObjectSchema>) -> Spec {
    let mut spec_json = json!({
      "openapi": "3.0.0",
      "info": {
        "title": "Test API",
        "version": "1.0.0"
      },
      "paths": {},
      "components": {
        "schemas": {}
      }
    });

    // Add schemas to the spec
    if let Some(components) = spec_json.get_mut("components")
      && let Some(schemas_obj) = components.get_mut("schemas")
    {
      for (name, schema) in schemas {
        let schema_json = serde_json::to_value(schema).unwrap();
        schemas_obj[name] = schema_json;
      }
    }

    serde_json::from_value(spec_json).unwrap()
  }

  #[test]
  fn test_discriminated_union_uses_struct_variants() {
    // Create a oneOf schema with discriminator
    let mut one_of_schema = ObjectSchema::default();

    // Add object variant
    let mut object_variant = ObjectSchema {
      schema_type: Some(SchemaTypeSet::Single(SchemaType::Object)),
      title: Some("ObjectVariant".to_string()),
      ..Default::default()
    };
    object_variant.properties.insert(
      "type".to_string(),
      ObjectOrReference::Object(ObjectSchema {
        schema_type: Some(SchemaTypeSet::Single(SchemaType::String)),
        const_value: Some(json!("object")),
        ..Default::default()
      }),
    );
    object_variant.properties.insert(
      "name".to_string(),
      ObjectOrReference::Object(ObjectSchema {
        schema_type: Some(SchemaTypeSet::Single(SchemaType::String)),
        ..Default::default()
      }),
    );
    object_variant.required.push("type".to_string());
    object_variant.required.push("name".to_string());

    // Add string variant (primitive type)
    let string_variant = ObjectSchema {
      schema_type: Some(SchemaTypeSet::Single(SchemaType::String)),
      title: Some("StringVariant".to_string()),
      ..Default::default()
    };

    // Add integer variant (primitive type)
    let integer_variant = ObjectSchema {
      schema_type: Some(SchemaTypeSet::Single(SchemaType::Integer)),
      title: Some("IntegerVariant".to_string()),
      ..Default::default()
    };

    one_of_schema.one_of.push(ObjectOrReference::Object(object_variant));
    one_of_schema.one_of.push(ObjectOrReference::Object(string_variant));
    one_of_schema.one_of.push(ObjectOrReference::Object(integer_variant));

    one_of_schema.discriminator = Some(Discriminator {
      property_name: "type".to_string(),
      mapping: Some(BTreeMap::new()),
    });

    let mut schemas = BTreeMap::new();
    schemas.insert("TestUnion".to_string(), one_of_schema);

    let spec = create_test_spec(schemas);
    let graph = SchemaGraph::new(spec).unwrap();
    let converter = SchemaConverter::new(&graph);

    let result = converter
      .convert_schema("TestUnion", graph.get_schema("TestUnion").unwrap())
      .unwrap();

    assert_eq!(result.len(), 1, "Should generate exactly one type");

    if let RustType::Enum(enum_def) = &result[0] {
      assert_eq!(enum_def.name, "TestUnion");
      assert!(enum_def.discriminator.is_some(), "Should have discriminator");

      // Check that ALL variants are struct variants (required for serde internally-tagged enums)
      for variant in &enum_def.variants {
        match &variant.content {
          VariantContent::Struct(fields) => {
            // For primitive types, should have a single "value" field
            if variant.name == "StringVariant" || variant.name == "IntegerVariant" {
              assert_eq!(
                fields.len(),
                1,
                "Primitive variant {} should have exactly one field",
                variant.name
              );
              assert_eq!(
                fields[0].name, "value",
                "Primitive variant field should be named 'value'"
              );
            }
          }
          VariantContent::Tuple(_) => {
            panic!(
              "Discriminated union variant {} must be a struct, not tuple",
              variant.name
            );
          }
          VariantContent::Unit => {
            panic!(
              "Discriminated union variant {} must be a struct, not unit",
              variant.name
            );
          }
        }
      }

      assert_eq!(
        enum_def.serde_attrs.len(),
        0,
        "serde_attrs should be empty (discriminator is separate)"
      );
    } else {
      panic!("Expected enum, got {:?}", result[0]);
    }
  }

  #[test]
  fn test_catch_all_enum_generates_two_level_structure() {
    // Create anyOf with const values + freeform string
    let mut any_of_schema = ObjectSchema::default();

    // Add const values
    any_of_schema.any_of.push(ObjectOrReference::Object(ObjectSchema {
      schema_type: Some(SchemaTypeSet::Single(SchemaType::String)),
      const_value: Some(json!("known-value-1")),
      description: Some("First known value".to_string()),
      ..Default::default()
    }));

    any_of_schema.any_of.push(ObjectOrReference::Object(ObjectSchema {
      schema_type: Some(SchemaTypeSet::Single(SchemaType::String)),
      const_value: Some(json!("known-value-2")),
      description: Some("Second known value".to_string()),
      ..Default::default()
    }));

    // Add freeform string (no const)
    any_of_schema.any_of.push(ObjectOrReference::Object(ObjectSchema {
      schema_type: Some(SchemaTypeSet::Single(SchemaType::String)),
      description: Some("Any other string".to_string()),
      ..Default::default()
    }));

    let mut schemas = BTreeMap::new();
    schemas.insert("CatchAllEnum".to_string(), any_of_schema);

    let spec = create_test_spec(schemas);
    let graph = SchemaGraph::new(spec).unwrap();
    let converter = SchemaConverter::new(&graph);

    let result = converter
      .convert_schema("CatchAllEnum", graph.get_schema("CatchAllEnum").unwrap())
      .unwrap();

    assert_eq!(
      result.len(),
      2,
      "Should generate TWO types (inner Known enum + outer untagged wrapper)"
    );

    // Check inner enum (Known values)
    if let RustType::Enum(inner_enum) = &result[0] {
      assert_eq!(inner_enum.name, "CatchAllEnumKnown");
      assert_eq!(inner_enum.variants.len(), 2, "Should have 2 known variants");
      assert!(inner_enum.serde_attrs.is_empty(), "Inner enum should not be untagged");

      // All variants should be unit variants with rename
      for variant in &inner_enum.variants {
        assert!(
          matches!(variant.content, VariantContent::Unit),
          "Known enum variants should be unit variants"
        );
        assert!(
          variant.serde_attrs.iter().any(|a| a.starts_with("rename")),
          "Known enum variants should have rename attribute"
        );
      }
    } else {
      panic!("First type should be inner Known enum, got {:?}", result[0]);
    }

    // Check outer enum (wrapper)
    if let RustType::Enum(outer_enum) = &result[1] {
      assert_eq!(outer_enum.name, "CatchAllEnum");
      assert_eq!(outer_enum.variants.len(), 2, "Should have 2 variants (Known + Other)");
      assert!(
        outer_enum.serde_attrs.contains(&"untagged".to_string()),
        "Outer enum should be untagged"
      );

      // Check Known variant
      let known_variant = outer_enum.variants.iter().find(|v| v.name == "Known").unwrap();
      match &known_variant.content {
        VariantContent::Tuple(types) => {
          assert_eq!(types.len(), 1);
          assert_eq!(types[0].base_type, "CatchAllEnumKnown");
        }
        _ => panic!("Known variant should be a tuple variant"),
      }

      // Check Other variant
      let other_variant = outer_enum.variants.iter().find(|v| v.name == "Other").unwrap();
      match &other_variant.content {
        VariantContent::Tuple(types) => {
          assert_eq!(types.len(), 1);
          assert_eq!(types[0].base_type, "String");
        }
        _ => panic!("Other variant should be a tuple variant"),
      }
    } else {
      panic!("Second type should be outer wrapper enum, got {:?}", result[1]);
    }
  }

  #[test]
  fn test_simple_string_enum() {
    let enum_schema = ObjectSchema {
      schema_type: Some(SchemaTypeSet::Single(SchemaType::String)),
      enum_values: vec![json!("value1"), json!("value2"), json!("value3")],
      ..Default::default()
    };

    let mut schemas = BTreeMap::new();
    schemas.insert("SimpleEnum".to_string(), enum_schema);

    let spec = create_test_spec(schemas);
    let graph = SchemaGraph::new(spec).unwrap();
    let converter = SchemaConverter::new(&graph);

    let result = converter
      .convert_schema("SimpleEnum", graph.get_schema("SimpleEnum").unwrap())
      .unwrap();

    assert_eq!(result.len(), 1, "Should generate exactly one enum");

    if let RustType::Enum(enum_def) = &result[0] {
      assert_eq!(enum_def.name, "SimpleEnum");
      assert_eq!(enum_def.variants.len(), 3);
      assert!(enum_def.discriminator.is_none());
      assert!(enum_def.serde_attrs.is_empty(), "Simple enum should not be untagged");

      // All variants should be unit variants with rename
      for variant in &enum_def.variants {
        assert!(matches!(variant.content, VariantContent::Unit));
        assert!(variant.serde_attrs.iter().any(|a| a.starts_with("rename")));
      }
    } else {
      panic!("Expected enum, got {:?}", result[0]);
    }
  }

  #[test]
  fn test_nullable_pattern_detection() {
    // Create anyOf with [Type, null] pattern
    let mut any_of_schema = ObjectSchema::default();

    // Add a string type
    any_of_schema.any_of.push(ObjectOrReference::Object(ObjectSchema {
      schema_type: Some(SchemaTypeSet::Single(SchemaType::String)),
      ..Default::default()
    }));

    // Add null type
    any_of_schema.any_of.push(ObjectOrReference::Object(ObjectSchema {
      schema_type: Some(SchemaTypeSet::Single(SchemaType::Null)),
      ..Default::default()
    }));

    let mut schemas = BTreeMap::new();
    schemas.insert("NullableString".to_string(), any_of_schema);

    let spec = create_test_spec(schemas);
    let graph = SchemaGraph::new(spec).unwrap();
    let converter = SchemaConverter::new(&graph);

    // For nullable patterns, schema_to_type_ref should return Option<String>
    let type_ref = converter
      .schema_to_type_ref(graph.get_schema("NullableString").unwrap())
      .unwrap();

    assert_eq!(type_ref.base_type, "String");
    assert!(
      type_ref.nullable,
      "Should detect nullable pattern and set nullable=true"
    );
    assert_eq!(type_ref.to_rust_type(), "Option<String>");
  }

  #[test]
  fn test_untagged_any_of_enum() {
    // Create anyOf with multiple non-const types (should be untagged enum)
    let mut any_of_schema = ObjectSchema::default();

    any_of_schema.any_of.push(ObjectOrReference::Object(ObjectSchema {
      schema_type: Some(SchemaTypeSet::Single(SchemaType::String)),
      title: Some("StringVariant".to_string()),
      ..Default::default()
    }));

    any_of_schema.any_of.push(ObjectOrReference::Object(ObjectSchema {
      schema_type: Some(SchemaTypeSet::Single(SchemaType::Integer)),
      title: Some("IntegerVariant".to_string()),
      ..Default::default()
    }));

    let mut schemas = BTreeMap::new();
    schemas.insert("UntaggedUnion".to_string(), any_of_schema);

    let spec = create_test_spec(schemas);
    let graph = SchemaGraph::new(spec).unwrap();
    let converter = SchemaConverter::new(&graph);

    let result = converter
      .convert_schema("UntaggedUnion", graph.get_schema("UntaggedUnion").unwrap())
      .unwrap();

    assert_eq!(result.len(), 1, "Should generate one enum");

    if let RustType::Enum(enum_def) = &result[0] {
      assert_eq!(enum_def.name, "UntaggedUnion");
      assert!(enum_def.discriminator.is_none(), "Should not have discriminator");
      assert!(
        enum_def.serde_attrs.contains(&"untagged".to_string()),
        "Should be untagged"
      );
      assert_eq!(enum_def.variants.len(), 2);
    } else {
      panic!("Expected enum, got {:?}", result[0]);
    }
  }
}

//! Schema converter for transforming OpenAPI schemas to Rust AST
//!
//! This module handles the conversion of OpenAPI schema definitions into
//! Rust type definitions (structs, enums, type aliases) with proper validation,
//! serde attributes, and documentation.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use oas3::spec::{ObjectOrReference, ObjectSchema, Schema, SchemaType, SchemaTypeSet};
use regex::Regex;
use serde_json::Number;

use super::{
  ast::{EnumDef, FieldDef, RustType, StructDef, TypeRef, VariantContent, VariantDef},
  schema_graph::SchemaGraph,
  utils::doc_comment_lines,
};
use crate::reserved::{to_rust_field_name, to_rust_type_name};

/// Field metadata extracted from an OpenAPI schema property
struct FieldMetadata {
  docs: Vec<String>,
  validation_attrs: Vec<String>,
  regex_validation: Option<String>,
  default_value: Option<serde_json::Value>,
  read_only: bool,
  write_only: bool,
  deprecated: bool,
  multiple_of: Option<serde_json::Number>,
}

/// Converter that transforms OpenAPI schemas into Rust AST structures
pub(crate) struct SchemaConverter<'a> {
  graph: &'a SchemaGraph,
}

impl<'a> SchemaConverter<'a> {
  pub(crate) fn new(graph: &'a SchemaGraph) -> Self {
    Self { graph }
  }

  /// Check if a schema is a discriminated base type (has discriminator with mappings and properties)
  fn is_discriminated_base_type(&self, schema: &ObjectSchema) -> bool {
    schema
      .discriminator
      .as_ref()
      .map(|d| d.mapping.as_ref().map(|m| !m.is_empty()).unwrap_or(false))
      .unwrap_or(false)
      && !schema.properties.is_empty()
  }

  /// Compute the inheritance depth of a schema (0 for schemas with no allOf)
  fn compute_inheritance_depth(&self, schema_name: &str, memo: &mut HashMap<String, usize>) -> usize {
    // Check memo first
    if let Some(&depth) = memo.get(schema_name) {
      return depth;
    }

    // Get the schema from components
    let Some(schema_ref) = self
      .graph
      .spec()
      .components
      .as_ref()
      .and_then(|c| c.schemas.get(schema_name))
    else {
      return 0;
    };

    let Ok(schema) = schema_ref.resolve(self.graph.spec()) else {
      return 0;
    };

    // Find max depth among all allOf parents
    let depth = if schema.all_of.is_empty() {
      0
    } else {
      schema
        .all_of
        .iter()
        .filter_map(|all_of_ref| match all_of_ref {
          ObjectOrReference::Ref { ref_path, .. } => SchemaGraph::extract_ref_name(ref_path),
          _ => None,
        })
        .map(|parent_name| self.compute_inheritance_depth(&parent_name, memo))
        .max()
        .unwrap_or(0)
        + 1
    };

    memo.insert(schema_name.to_string(), depth);
    depth
  }

  /// Extract child schemas from discriminator mapping, sorted by depth (deepest first)
  fn extract_discriminator_children(&self, schema: &ObjectSchema) -> Vec<(String, String)> {
    let Some(discriminator) = schema.discriminator.as_ref() else {
      return vec![];
    };

    let Some(mapping) = discriminator.mapping.as_ref() else {
      return vec![];
    };

    let mut children: Vec<(String, String)> = mapping
      .iter()
      .filter_map(|(disc_value, ref_path)| {
        let schema_name = SchemaGraph::extract_ref_name(ref_path)?;
        Some((disc_value.clone(), schema_name))
      })
      .collect();

    // Sort by inheritance depth (deepest first)
    let mut depth_memo = HashMap::new();
    children
      .sort_by_cached_key(|(_, schema_name)| -(self.compute_inheritance_depth(schema_name, &mut depth_memo) as i32));

    children
  }

  /// Convert a child schema that extends a discriminated parent
  fn convert_discriminated_child(
    &self,
    name: &str,
    schema: &ObjectSchema,
    parent_name: &str,
    parent_schema: &ObjectSchema,
  ) -> anyhow::Result<Vec<RustType>> {
    let struct_name = to_rust_type_name(name);
    let parent_type_name = format!("{}Fields", to_rust_type_name(parent_name));

    let Some(discriminator_prop_name) = parent_schema.discriminator.as_ref().map(|d| d.property_name.clone()) else {
      return Err(anyhow::anyhow!("Parent schema is not discriminated"));
    };

    let discriminator_value = self.get_discriminator_value_for_child(name, schema, &discriminator_prop_name);

    // Create discriminator field as String with default value
    // We'll use the default_value in the FieldDef to trigger Default impl generation
    let disc_field = FieldDef {
      name: to_rust_field_name(&discriminator_prop_name),
      docs: vec![],
      rust_type: TypeRef::new("String"),
      serde_attrs: vec![
        "default".to_string(), // Use Default::default() when field missing from JSON
        format!("rename = \"{}\"", discriminator_prop_name),
      ],
      validation_attrs: vec![],
      regex_validation: None,
      default_value: Some(serde_json::Value::String(discriminator_value.clone())),
      read_only: false,
      write_only: false,
      deprecated: false,
      multiple_of: None,
    };

    // Create flattened parent field
    let parent_field = FieldDef {
      name: "__inherited_properties".to_string(),
      docs: vec![],
      rust_type: TypeRef::new(parent_type_name),
      serde_attrs: vec!["flatten".to_string()],
      validation_attrs: vec![],
      regex_validation: None,
      default_value: None,
      read_only: false,
      write_only: false,
      deprecated: false,
      multiple_of: None,
    };

    // Extract this child's own properties (from inline schema in allOf)
    // Exclude the discriminator field since we're adding it separately
    let mut own_fields = Vec::new();
    let mut inline_types = Vec::new();

    for all_of_item in &schema.all_of {
      if let ObjectOrReference::Object(inline_schema) = all_of_item {
        let (child_fields, child_inline_types) = self.convert_fields_with_inline_types_and_exclusions(
          &struct_name,
          inline_schema,
          Some(&discriminator_prop_name),
        )?;
        own_fields.extend(child_fields);
        inline_types.extend(child_inline_types);
      }
    }

    let mut fields = vec![disc_field, parent_field];
    fields.extend(own_fields);

    let derives = vec![
      "Debug".into(),
      "Clone".into(),
      "Serialize".into(),
      "Deserialize".into(),
      "Validate".into(),
      "Default".into(), // Always derive Default
    ];

    let struct_type = RustType::Struct(StructDef {
      name: struct_name,
      docs: schema
        .description
        .as_ref()
        .map(|d| doc_comment_lines(d))
        .unwrap_or_default(),
      fields,
      derives,
      serde_attrs: vec![], // Don't use struct-level default - discriminator field has its own default
    });

    let mut all_types = vec![struct_type];
    all_types.extend(inline_types);
    Ok(all_types)
  }

  /// Get the discriminator value for a child schema
  fn get_discriminator_value_for_child(
    &self,
    _child_name: &str,
    child_schema: &ObjectSchema,
    discriminator_prop_name: &str,
  ) -> String {
    for all_of_item in &child_schema.all_of {
      if let ObjectOrReference::Object(inline_schema) = all_of_item
        && let Some(disc_prop) = inline_schema.properties.get(discriminator_prop_name)
        && let Ok(disc_schema) = disc_prop.resolve(self.graph.spec())
        && let Some(default) = disc_schema.default.as_ref()
        && let Some(default_str) = default.as_str()
      {
        return default_str.to_string();
      }
    }
    format!("#{}", _child_name)
  }

  /// Create a discriminated enum for schemas with discriminator mappings
  fn create_discriminated_enum(&self, base_name: &str, schema: &ObjectSchema) -> anyhow::Result<RustType> {
    use crate::generator::ast::{DiscriminatedEnumDef, DiscriminatedVariant};

    let children = self.extract_discriminator_children(schema);
    let enum_name = to_rust_type_name(base_name);
    let base_struct_name = format!("{}Base", enum_name);

    let discriminator_field = schema
      .discriminator
      .as_ref()
      .map(|d| d.property_name.clone())
      .unwrap_or_else(|| "@odata.type".to_string());

    let mut variants = Vec::new();

    // Add child variants (most specific first - already sorted by depth)
    for (disc_value, child_schema_name) in children {
      let child_type_name = to_rust_type_name(&child_schema_name);

      // Generate variant name by removing common prefix
      let variant_name = if child_type_name.starts_with(&enum_name) {
        child_type_name
          .strip_prefix(&enum_name)
          .unwrap_or(&child_type_name)
          .to_string()
      } else {
        child_type_name.clone()
      };

      // Ensure variant name is not empty
      let variant_name = if variant_name.is_empty() {
        child_type_name.clone()
      } else {
        variant_name
      };

      // Always use Box<> for child variants to prevent infinite recursion
      // Child types might reference the parent enum, creating cycles
      let final_type_name = format!("Box<{}>", child_type_name);

      variants.push(DiscriminatedVariant {
        discriminator_value: disc_value,
        variant_name,
        type_name: final_type_name,
      });
    }

    // Add fallback variant (for inheritance hierarchies where base type can appear)
    let base_variant_name = to_rust_type_name(base_name.split('.').next_back().unwrap_or(base_name));
    let fallback = Some(DiscriminatedVariant {
      discriminator_value: "".into(), // Not used for fallback
      variant_name: base_variant_name,
      type_name: base_struct_name,
    });

    Ok(RustType::DiscriminatedEnum(DiscriminatedEnumDef {
      name: enum_name,
      docs: schema
        .description
        .as_ref()
        .map(|d| doc_comment_lines(d))
        .unwrap_or_default(),
      discriminator_field,
      variants,
      fallback,
    }))
  }

  /// Convert a schema to Rust type definitions
  /// Returns the main type and any inline types that were generated
  pub(crate) fn convert_schema(&self, name: &str, schema: &ObjectSchema) -> anyhow::Result<Vec<RustType>> {
    // Determine the type of Rust definition we need to create

    // Check if this is an allOf composition
    if !schema.all_of.is_empty() {
      return self.convert_all_of_schema(name, schema);
    }

    // Check if this is an enum (oneOf/anyOf)
    if !schema.one_of.is_empty() {
      return self.convert_one_of_enum(name, schema);
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
      let (main_type, mut inline_types) = self.convert_struct(name, schema)?;

      // If this is a discriminated base type, also generate the discriminated enum
      let mut all_types = if self.is_discriminated_base_type(schema) {
        // Generate the discriminated enum that wraps all variants
        let discriminated_enum = self.create_discriminated_enum(name, schema)?;
        vec![discriminated_enum, main_type] // Enum first, then base struct
      } else {
        vec![main_type]
      };

      all_types.append(&mut inline_types);
      return Ok(all_types);
    }

    // Otherwise, might be a type alias or something we can skip
    Ok(vec![])
  }

  /// Recursively collect all properties, required fields, and discriminators from a schema's allOf chain
  fn collect_all_of_properties(
    &self,
    schema: &ObjectSchema,
    properties: &mut BTreeMap<String, ObjectOrReference<ObjectSchema>>,
    required: &mut Vec<String>,
    discriminator: &mut Option<oas3::spec::Discriminator>,
  ) -> anyhow::Result<()> {
    // First, recursively process all allOf references to get inherited properties
    for all_of_ref in &schema.all_of {
      if let Ok(all_of_schema) = all_of_ref.resolve(self.graph.spec()) {
        self.collect_all_of_properties(&all_of_schema, properties, required, discriminator)?;
      }
    }

    // Then add this schema's own properties (later schemas can override)
    for (prop_name, prop_ref) in &schema.properties {
      properties.insert(prop_name.clone(), prop_ref.clone());
    }

    // Merge required fields (avoid duplicates)
    for req in &schema.required {
      if !required.contains(req) {
        required.push(req.clone());
      }
    }

    // Preserve discriminator if present (last one wins)
    if schema.discriminator.is_some() {
      *discriminator = schema.discriminator.clone();
    }

    Ok(())
  }

  /// Convert an allOf schema by merging all schemas into one struct
  fn convert_all_of_schema(&self, name: &str, schema: &ObjectSchema) -> anyhow::Result<Vec<RustType>> {
    // Check if any parent has a discriminator
    // Need to merge the parent's allOf first to check for discriminators in inline schemas
    let discriminated_parent = schema.all_of.iter().find_map(|all_of_ref| {
      let ObjectOrReference::Ref { ref_path, .. } = all_of_ref else {
        return None;
      };
      let parent_name = SchemaGraph::extract_ref_name(ref_path)?;

      let parent_ref = self.graph.spec().components.as_ref()?.schemas.get(&parent_name)?;
      let parent_schema = parent_ref.resolve(self.graph.spec()).ok()?;

      // Merge parent's allOf to get complete schema with discriminators from inline schemas
      let merged_parent = if !parent_schema.all_of.is_empty() {
        let mut merged_props = BTreeMap::new();
        let mut merged_req = Vec::new();
        let mut merged_disc = None;
        self
          .collect_all_of_properties(&parent_schema, &mut merged_props, &mut merged_req, &mut merged_disc)
          .ok()?;

        let mut merged = parent_schema.clone();
        merged.properties = merged_props;
        merged.required = merged_req;
        merged.discriminator = merged_disc;
        merged
      } else {
        parent_schema.clone()
      };

      let is_disc_base = self.is_discriminated_base_type(&merged_parent);

      if is_disc_base {
        Some((parent_name, merged_parent))
      } else {
        None
      }
    });

    if let Some((parent_name, parent_schema)) = discriminated_parent {
      // Generate child struct with discriminator field and flattened parent
      return self.convert_discriminated_child(name, schema, &parent_name, &parent_schema);
    }

    let mut merged_properties = BTreeMap::new();
    let mut merged_required = Vec::new();
    let mut merged_discriminator = None;

    self.collect_all_of_properties(
      schema,
      &mut merged_properties,
      &mut merged_required,
      &mut merged_discriminator,
    )?;

    // Create a merged schema with all collected properties and discriminator
    let mut merged_schema = schema.clone();
    merged_schema.properties = merged_properties;
    merged_schema.required = merged_required;
    merged_schema.discriminator = merged_discriminator.clone();

    // Now convert as a regular struct
    let (main_type, mut inline_types) = self.convert_struct(name, &merged_schema)?;

    // If the merged schema is a discriminated base type, also generate the enum
    let mut all_types = if self.is_discriminated_base_type(&merged_schema) {
      let discriminated_enum = self.create_discriminated_enum(name, &merged_schema)?;
      vec![discriminated_enum, main_type] // Enum first, then base struct
    } else {
      vec![main_type]
    };

    all_types.append(&mut inline_types);
    Ok(all_types)
  }

  fn convert_one_of_enum(&self, name: &str, schema: &ObjectSchema) -> anyhow::Result<Vec<RustType>> {
    let mut inline_types = Vec::new();

    let discriminator_prop = schema.discriminator.as_ref().map(|d| d.property_name.as_str());

    let discriminator_map: BTreeMap<String, String> = schema
      .discriminator
      .as_ref()
      .and_then(|d| d.mapping.as_ref())
      .map(|mapping| {
        mapping
          .iter()
          .filter_map(|(val, ref_path)| SchemaGraph::extract_ref_name(ref_path).map(|name| (name, val.clone())))
          .collect()
      })
      .unwrap_or_default();

    let mut seen_names = BTreeSet::new();
    let mut variants_intermediate: Vec<_> = schema
      .one_of
      .iter()
      .enumerate()
      .filter_map(|(i, variant_schema_ref)| {
        let resolved_schema = variant_schema_ref.resolve(self.graph.spec()).ok()?;
        if resolved_schema.schema_type == Some(SchemaTypeSet::Single(SchemaType::Null)) {
          return None;
        }

        let mut variant_name = resolved_schema
          .title
          .as_deref()
          .map(to_rust_type_name)
          .unwrap_or_else(|| self.infer_variant_name(&resolved_schema, i));

        if !seen_names.insert(variant_name.clone()) {
          variant_name = format!("{}{}", variant_name, i);
          seen_names.insert(variant_name.clone());
        }

        let (content, mut generated_types) = self
          .determine_variant_content(name, &resolved_schema, discriminator_prop)
          .ok()?;
        inline_types.append(&mut generated_types);

        let mut serde_attrs = Vec::new();
        if discriminator_prop.is_some()
          && let ObjectOrReference::Ref { ref_path, .. } = variant_schema_ref
          && let Some(schema_name) = SchemaGraph::extract_ref_name(ref_path)
          && let Some(disc_value) = discriminator_map.get(&schema_name)
        {
          serde_attrs.push(format!("rename = \"{}\"", disc_value));
        }

        Some(VariantDef {
          name: variant_name,
          docs: resolved_schema
            .description
            .as_deref()
            .map(doc_comment_lines)
            .unwrap_or_default(),
          content,
          serde_attrs,
          deprecated: resolved_schema.deprecated.unwrap_or(false),
        })
      })
      .collect();

    let original_names: Vec<_> = variants_intermediate.iter().map(|v| v.name.clone()).collect();
    let stripped_names = Self::strip_common_affixes(&original_names);

    for (variant, stripped_name) in variants_intermediate.iter_mut().zip(stripped_names) {
      variant.name = stripped_name;
    }

    let main_enum = RustType::Enum(EnumDef {
      name: to_rust_type_name(name),
      docs: schema.description.as_deref().map(doc_comment_lines).unwrap_or_default(),
      variants: variants_intermediate,
      discriminator: schema.discriminator.as_ref().map(|d| d.property_name.clone()),
      derives: vec![
        "Debug".into(),
        "Clone".into(),
        "Serialize".into(),
        "Deserialize".into(),
        "Default".into(),
      ],
      serde_attrs: vec![],
    });

    inline_types.push(main_enum);
    Ok(inline_types)
  }

  fn determine_variant_content(
    &self,
    parent_name: &str,
    schema: &ObjectSchema,
    discriminator_prop: Option<&str>,
  ) -> anyhow::Result<(VariantContent, Vec<RustType>)> {
    if let Some(disc_prop) = discriminator_prop {
      return if !schema.properties.is_empty() {
        let (fields, inline_types) =
          self.convert_fields_with_inline_types_and_exclusions(parent_name, schema, Some(disc_prop))?;
        Ok((VariantContent::Struct(fields), inline_types))
      } else {
        let field = FieldDef {
          name: "value".to_string(),
          rust_type: self.schema_to_type_ref(schema)?,
          ..Default::default()
        };
        Ok((VariantContent::Struct(vec![field]), vec![]))
      };
    }

    match (&schema.title, !schema.properties.is_empty()) {
      (Some(title), _) if self.graph.get_schema(title).is_some() => {
        let type_ref = TypeRef::new(to_rust_type_name(title));
        Ok((VariantContent::Tuple(vec![type_ref]), vec![]))
      }
      (_, true) => {
        let fields = self.convert_fields(schema)?;
        Ok((VariantContent::Struct(fields), vec![]))
      }
      _ => {
        let type_ref = self.schema_to_type_ref(schema)?;
        Ok((VariantContent::Tuple(vec![type_ref]), vec![]))
      }
    }
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
      // Check if this is a $ref before resolving
      let ref_schema_name = if let ObjectOrReference::Ref { ref_path, .. } = variant_schema_ref {
        SchemaGraph::extract_ref_name(ref_path)
      } else {
        None
      };

      if let Ok(variant_schema) = variant_schema_ref.resolve(self.graph.spec()) {
        // Skip null variants - they're handled by making the field Option<T>
        if variant_schema.schema_type == Some(SchemaTypeSet::Single(SchemaType::Null)) {
          continue;
        }

        // If this was a $ref to a schema in components, use a tuple variant
        if let Some(ref schema_name) = ref_schema_name {
          let rust_type_name = to_rust_type_name(schema_name);
          let mut type_ref = TypeRef::new(&rust_type_name);
          // Apply Box wrapping if this schema is part of a cycle
          if self.graph.is_cyclic(schema_name) {
            type_ref = type_ref.with_boxed();
          }
          let docs = variant_schema
            .description
            .as_ref()
            .map(|d| doc_comment_lines(d))
            .unwrap_or_default();
          let deprecated = variant_schema.deprecated.unwrap_or(false);

          // Use the schema name as variant name
          let mut variant_name = rust_type_name.clone();
          if seen_names.contains(&variant_name) {
            variant_name = format!("{}{}", variant_name, i);
          }
          seen_names.insert(variant_name.clone());

          variants.push(VariantDef {
            name: variant_name,
            docs,
            content: VariantContent::Tuple(vec![type_ref]),
            serde_attrs: vec![],
            deprecated,
          });
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

        // Determine variant content - inline objects or primitives
        let content = if !variant_schema.properties.is_empty() {
          // Inline object - create struct variant
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

    // Strip common prefix/suffix from variant names to satisfy clippy::enum_variant_names
    let original_names: Vec<String> = variants.iter().map(|v| v.name.clone()).collect();
    let stripped_names = Self::strip_common_affixes(&original_names);

    // Update variant names with stripped versions
    for (variant, stripped_name) in variants.iter_mut().zip(stripped_names.iter()) {
      variant.name = stripped_name.clone();
    }

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
      derives: vec![
        "Debug".into(),
        "Clone".into(),
        "Serialize".into(),
        "Deserialize".into(),
        "Default".into(),
      ],
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
        "Default".into(),
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
        "Default".into(),
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

  /// Split a PascalCase name into words
  fn split_pascal_case(name: &str) -> Vec<String> {
    let mut words = Vec::new();
    let mut current_word = String::new();

    for (i, ch) in name.chars().enumerate() {
      if ch.is_uppercase() && i > 0 && !current_word.is_empty() {
        words.push(current_word.clone());
        current_word.clear();
      }
      current_word.push(ch);
    }

    if !current_word.is_empty() {
      words.push(current_word);
    }

    words
  }

  /// Strip common prefix/suffix from enum variant names to satisfy clippy::enum_variant_names
  /// Only strips if there are at least 3 variants with the common prefix/suffix
  fn strip_common_affixes(variant_names: &[String]) -> Vec<String> {
    if variant_names.len() < 3 {
      return variant_names.to_vec();
    }

    // Split all names into words
    let split_names: Vec<Vec<String>> = variant_names.iter().map(|n| Self::split_pascal_case(n)).collect();

    // Find common prefix words
    let mut common_prefix_len = 0;
    if let Some(first) = split_names.first() {
      'prefix: for (i, word) in first.iter().enumerate() {
        for name_words in &split_names[1..] {
          if name_words.len() <= i || &name_words[i] != word {
            break 'prefix;
          }
        }
        common_prefix_len = i + 1;
      }
    }

    // Find common suffix words
    let mut common_suffix_len = 0;
    if let Some(first) = split_names.first() {
      'suffix: for i in 1..=first.len() {
        let word = &first[first.len() - i];
        for name_words in &split_names[1..] {
          if name_words.len() < i || &name_words[name_words.len() - i] != word {
            break 'suffix;
          }
        }
        common_suffix_len = i;
      }
    }

    // Build stripped names
    let mut stripped_names = Vec::new();
    for words in &split_names {
      let start = common_prefix_len;
      let end = words.len().saturating_sub(common_suffix_len);

      if start >= end {
        // Stripping would leave empty name - keep original
        stripped_names.push(words.join(""));
      } else {
        stripped_names.push(words[start..end].join(""));
      }
    }

    // Check for conflicts - if any stripped name is empty or duplicated, return original
    let mut seen = BTreeSet::new();
    for name in &stripped_names {
      if name.is_empty() || !seen.insert(name) {
        return variant_names.to_vec();
      }
    }

    stripped_names
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
      derives: vec![
        "Debug".into(),
        "Clone".into(),
        "Serialize".into(),
        "Deserialize".into(),
        "Default".into(),
      ],
      serde_attrs: vec![],
    }))
  }

  /// Convert an object schema to a Rust struct
  /// Returns the struct and any inline types that were generated
  pub(crate) fn convert_struct(&self, name: &str, schema: &ObjectSchema) -> anyhow::Result<(RustType, Vec<RustType>)> {
    let is_discriminated = self.is_discriminated_base_type(schema);

    let struct_name = if is_discriminated {
      format!("{}Fields", to_rust_type_name(name))
    } else {
      to_rust_type_name(name)
    };

    let discriminator_field_to_exclude = if is_discriminated {
      schema.discriminator.as_ref().map(|d| d.property_name.as_str())
    } else {
      None
    };

    let (mut fields, inline_types) = if is_discriminated {
      self.convert_fields_with_inline_types_and_exclusions(&struct_name, schema, discriminator_field_to_exclude)?
    } else {
      self.convert_fields_with_inline_types(&struct_name, schema)?
    };

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
              serde_attrs: vec!["flatten".to_string()],
              validation_attrs: vec![],
              regex_validation: None,
              default_value: None,
              read_only: false,
              write_only: false,
              deprecated: false,
              multiple_of: None,
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

    // ALWAYS derive Default for all structs (using better_default)
    derives.push("Default".into());

    let struct_type = RustType::Struct(StructDef {
      name: struct_name,
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

  pub(crate) fn extract_validation_pattern<'s>(&self, prop_name: &str, schema: &'s ObjectSchema) -> Option<&'s String> {
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
  pub(crate) fn extract_validation_attrs(
    &self,
    _prop_name: &str,
    is_required: bool,
    schema: &ObjectSchema,
  ) -> Vec<String> {
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
  pub(crate) fn extract_default_value(&self, schema: &ObjectSchema) -> Option<serde_json::Value> {
    schema.default.clone()
  }

  /// Resolves a property type with special handling for inline anyOf unions
  /// Returns the TypeRef and any generated inline enum types
  fn resolve_property_type_with_inline_enums(
    &self,
    parent_name: &str,
    prop_name: &str,
    prop_schema_ref: &ObjectOrReference<ObjectSchema>,
  ) -> anyhow::Result<(TypeRef, Vec<RustType>)> {
    match prop_schema_ref {
      ObjectOrReference::Ref { ref_path, .. } => {
        if let Some(ref_name) = SchemaGraph::extract_ref_name(ref_path) {
          Ok((TypeRef::new(to_rust_type_name(&ref_name)), vec![]))
        } else if let Ok(prop_schema) = prop_schema_ref.resolve(self.graph.spec()) {
          Ok((self.schema_to_type_ref(&prop_schema)?, vec![]))
        } else {
          Ok((TypeRef::new("serde_json::Value"), vec![]))
        }
      }
      _ => {
        let Ok(prop_schema) = prop_schema_ref.resolve(self.graph.spec()) else {
          return Ok((TypeRef::new("serde_json::Value"), vec![]));
        };

        // Check if this has oneOf or anyOf
        let has_one_of = !prop_schema.one_of.is_empty();
        let has_any_of = !prop_schema.any_of.is_empty();

        if !has_one_of && !has_any_of {
          return Ok((self.schema_to_type_ref(&prop_schema)?, vec![]));
        }

        // Use oneOf if present, otherwise anyOf
        let variants = if has_one_of {
          &prop_schema.one_of
        } else {
          &prop_schema.any_of
        };

        let has_nullable_or_generic = variants.iter().any(|v| {
          v.resolve(self.graph.spec())
            .ok()
            .map(|s| Self::is_nullable_or_generic(&s))
            .unwrap_or(false)
        });

        if has_nullable_or_generic && variants.len() == 2 {
          for variant_ref in variants {
            if let Some(ref_name) = Self::try_extract_ref_name(variant_ref) {
              let mut type_ref = TypeRef::new(to_rust_type_name(&ref_name));
              // Apply Box wrapping if this schema is part of a cycle
              if self.graph.is_cyclic(&ref_name) {
                type_ref = type_ref.with_boxed();
              }
              return Ok((type_ref.with_option(), vec![]));
            }

            if let Ok(resolved) = variant_ref.resolve(self.graph.spec())
              && !Self::is_nullable_or_generic(&resolved)
            {
              return Ok((self.schema_to_type_ref(&resolved)?.with_option(), vec![]));
            }
          }
          return Ok((self.schema_to_type_ref(&prop_schema)?.with_option(), vec![]));
        }

        // Check if this union matches an existing schema
        if let Some(matching_schema) = self.find_matching_union_schema(variants) {
          let mut type_ref = TypeRef::new(to_rust_type_name(&matching_schema));
          if self.graph.is_cyclic(&matching_schema) {
            type_ref = type_ref.with_boxed();
          }
          return Ok((type_ref, vec![]));
        }

        let should_generate_inline_enum = prop_schema.title.is_none()
          || prop_schema
            .title
            .as_ref()
            .map(|t| self.graph.get_schema(t).is_none())
            .unwrap_or(true);

        if should_generate_inline_enum {
          let enum_name = format!("{}.{}", parent_name, prop_name);
          let enum_types = if has_one_of {
            self.convert_one_of_enum(&enum_name, &prop_schema)?
          } else {
            self.convert_any_of_enum(&enum_name, &prop_schema)?
          };
          let type_name = if let Some(RustType::Enum(enum_def)) = enum_types.last() {
            enum_def.name.clone()
          } else {
            to_rust_type_name(&enum_name)
          };
          Ok((TypeRef::new(&type_name), enum_types))
        } else {
          Ok((self.schema_to_type_ref(&prop_schema)?, vec![]))
        }
      }
    }
  }

  /// Converts a property schema reference to a TypeRef, handling both $ref and inline schemas
  fn resolve_property_type(&self, prop_schema_ref: &ObjectOrReference<ObjectSchema>) -> anyhow::Result<TypeRef> {
    match prop_schema_ref {
      ObjectOrReference::Ref { ref_path, .. } => {
        if let Some(ref_name) = SchemaGraph::extract_ref_name(ref_path) {
          let mut type_ref = TypeRef::new(to_rust_type_name(&ref_name));
          // Apply Box wrapping if this schema is part of a cycle
          if self.graph.is_cyclic(&ref_name) {
            type_ref = type_ref.with_boxed();
          }
          Ok(type_ref)
        } else if let Ok(prop_schema) = prop_schema_ref.resolve(self.graph.spec()) {
          self.schema_to_type_ref(&prop_schema)
        } else {
          Ok(TypeRef::new("serde_json::Value"))
        }
      }
      _ => {
        if let Ok(prop_schema) = prop_schema_ref.resolve(self.graph.spec()) {
          self.schema_to_type_ref(&prop_schema)
        } else {
          Ok(TypeRef::new("serde_json::Value"))
        }
      }
    }
  }

  /// Extracts metadata (docs, validation, flags) from a resolved property schema
  fn extract_field_metadata(
    &self,
    prop_name: &str,
    is_required: bool,
    prop_schema_ref: &ObjectOrReference<ObjectSchema>,
  ) -> FieldMetadata {
    if let Ok(prop_schema) = prop_schema_ref.resolve(self.graph.spec()) {
      let docs = prop_schema
        .description
        .as_ref()
        .map(|d| doc_comment_lines(d))
        .unwrap_or_default();
      let validation_attrs = self.extract_validation_attrs(prop_name, is_required, &prop_schema);
      let regex_validation = self.extract_validation_pattern(prop_name, &prop_schema).cloned();
      let default_value = self.extract_default_value(&prop_schema);
      let read_only = prop_schema.read_only.unwrap_or(false);
      let write_only = prop_schema.write_only.unwrap_or(false);
      let deprecated = prop_schema.deprecated.unwrap_or(false);
      let multiple_of = prop_schema.multiple_of.clone();

      FieldMetadata {
        docs,
        validation_attrs,
        regex_validation,
        default_value,
        read_only,
        write_only,
        deprecated,
        multiple_of,
      }
    } else {
      FieldMetadata {
        docs: vec![],
        validation_attrs: vec![],
        regex_validation: None,
        default_value: None,
        read_only: false,
        write_only: false,
        deprecated: false,
        multiple_of: None,
      }
    }
  }

  /// Builds serde attributes for a field (rename, skip_serializing_if)
  fn build_serde_attrs(prop_name: &str, is_optional: bool, is_nullable: bool) -> Vec<String> {
    let mut serde_attrs = vec![];

    let rust_field_name = to_rust_field_name(prop_name);
    if rust_field_name != prop_name {
      serde_attrs.push(format!("rename = \"{}\"", prop_name));
    }

    if is_optional || is_nullable {
      serde_attrs.push("skip_serializing_if = \"Option::is_none\"".to_string());
    }

    serde_attrs
  }

  /// Filters out regex validation for types that don't support it
  fn filter_regex_validation(rust_type: &TypeRef, regex: Option<String>) -> Option<String> {
    match rust_type.base_type.as_str() {
      "chrono::DateTime<chrono::Utc>" | "chrono::NaiveDate" | "chrono::NaiveTime" | "uuid::Uuid" => None,
      _ => regex,
    }
  }

  /// Wraps a TypeRef with Option if needed (avoids double-wrapping)
  fn apply_optionality(rust_type: TypeRef, is_optional: bool) -> TypeRef {
    let is_nullable = rust_type.nullable;
    if is_optional && !is_nullable {
      rust_type.with_option()
    } else {
      rust_type
    }
  }

  /// Deduplicates field names that collide after conversion to snake_case.
  /// Strategy:
  /// - If duplicates exist where some are deprecated and some are not, remove deprecated ones
  /// - Otherwise, append numeric suffixes (_2, _3, etc.) to later occurrences
  fn deduplicate_field_names(fields: &mut Vec<FieldDef>) {
    let mut name_groups: HashMap<String, Vec<usize>> = HashMap::new();

    for (idx, field) in fields.iter().enumerate() {
      name_groups.entry(field.name.clone()).or_default().push(idx);
    }

    let mut indices_to_remove = HashSet::<usize>::new();

    for (name, indices) in name_groups {
      if indices.len() <= 1 {
        // Skip if there's no collision.
        continue;
      }

      let (deprecated_indices, non_deprecated_indices): (Vec<_>, Vec<_>) =
        indices.iter().partition(|&&idx| fields[idx].deprecated);

      if !deprecated_indices.is_empty() && !non_deprecated_indices.is_empty() {
        indices_to_remove.extend(&deprecated_indices);
      } else {
        for (i, &idx) in indices.iter().enumerate().skip(1) {
          fields[idx].name = format!("{}_{}", name, i + 1);
        }
      }
    }

    if !indices_to_remove.is_empty() {
      *fields = fields
        .iter()
        .enumerate()
        .filter_map(|(idx, field)| {
          if indices_to_remove.contains(&idx) {
            None
          } else {
            Some(field.clone())
          }
        })
        .collect();
    }
  }

  /// Builds a FieldDef from all the constituent parts
  fn build_field_def(
    prop_name: &str,
    rust_type: TypeRef,
    serde_attrs: Vec<String>,
    metadata: FieldMetadata,
    regex_validation: Option<String>,
  ) -> FieldDef {
    FieldDef {
      name: to_rust_field_name(prop_name),
      docs: metadata.docs,
      rust_type,
      serde_attrs,
      validation_attrs: metadata.validation_attrs,
      regex_validation,
      default_value: metadata.default_value,
      read_only: metadata.read_only,
      write_only: metadata.write_only,
      deprecated: metadata.deprecated,
      multiple_of: metadata.multiple_of,
    }
  }

  /// Converts schema properties to struct fields, optionally excluding specified fields
  fn convert_fields_with_exclusions(
    &self,
    schema: &ObjectSchema,
    exclude_field: Option<&str>,
  ) -> anyhow::Result<Vec<FieldDef>> {
    let mut fields = Vec::new();
    let mut properties: Vec<_> = schema.properties.iter().collect();
    properties.sort_by(|(a, _), (b, _)| a.cmp(b));

    for (prop_name, prop_schema_ref) in properties {
      if let Some(exclude) = exclude_field
        && prop_name == exclude
      {
        continue;
      }

      let rust_type = self.resolve_property_type(prop_schema_ref)?;
      let is_required = schema.required.contains(prop_name);
      let is_optional = !is_required;

      let serde_attrs = Self::build_serde_attrs(prop_name, is_optional, rust_type.nullable);
      let metadata = self.extract_field_metadata(prop_name, is_required, prop_schema_ref);
      let regex_validation = Self::filter_regex_validation(&rust_type, metadata.regex_validation.clone());
      let final_type = Self::apply_optionality(rust_type, is_optional);

      fields.push(Self::build_field_def(
        prop_name,
        final_type,
        serde_attrs,
        metadata,
        regex_validation,
      ));
    }

    // Deduplicate field names that collide after conversion to snake_case
    Self::deduplicate_field_names(&mut fields);

    Ok(fields)
  }

  /// Converts schema properties to struct fields with inline enum generation, optionally excluding specified fields
  fn convert_fields_with_inline_types_and_exclusions(
    &self,
    parent_name: &str,
    schema: &ObjectSchema,
    exclude_field: Option<&str>,
  ) -> anyhow::Result<(Vec<FieldDef>, Vec<RustType>)> {
    let mut fields = Vec::new();
    let mut inline_types = Vec::new();

    let mut properties: Vec<_> = schema.properties.iter().collect();
    properties.sort_by(|(a, _), (b, _)| a.cmp(b));

    for (prop_name, prop_schema_ref) in properties {
      if let Some(exclude) = exclude_field
        && prop_name == exclude
      {
        continue;
      }

      let (rust_type, generated_types) =
        self.resolve_property_type_with_inline_enums(parent_name, prop_name, prop_schema_ref)?;
      inline_types.extend(generated_types);

      let is_required = schema.required.contains(prop_name);
      let is_optional = !is_required;

      let serde_attrs = Self::build_serde_attrs(prop_name, is_optional, rust_type.nullable);
      let metadata = self.extract_field_metadata(prop_name, is_required, prop_schema_ref);
      let regex_validation = Self::filter_regex_validation(&rust_type, metadata.regex_validation.clone());
      let final_type = Self::apply_optionality(rust_type, is_optional);

      fields.push(Self::build_field_def(
        prop_name,
        final_type,
        serde_attrs,
        metadata,
        regex_validation,
      ));
    }

    // Deduplicate field names that collide after conversion to snake_case
    Self::deduplicate_field_names(&mut fields);

    Ok((fields, inline_types))
  }

  /// Convert schema properties to struct fields (convenience wrapper)
  fn convert_fields(&self, schema: &ObjectSchema) -> anyhow::Result<Vec<FieldDef>> {
    self.convert_fields_with_exclusions(schema, None)
  }

  /// Converts schema properties to struct fields with inline enum generation for anyOf unions
  fn convert_fields_with_inline_types(
    &self,
    parent_name: &str,
    schema: &ObjectSchema,
  ) -> anyhow::Result<(Vec<FieldDef>, Vec<RustType>)> {
    let mut fields = Vec::new();
    let mut inline_types = Vec::new();

    let mut properties: Vec<_> = schema.properties.iter().collect();
    properties.sort_by(|(a, _), (b, _)| a.cmp(b));

    for (prop_name, prop_schema_ref) in properties {
      let (rust_type, generated_types) =
        self.resolve_property_type_with_inline_enums(parent_name, prop_name, prop_schema_ref)?;
      inline_types.extend(generated_types);

      let is_required = schema.required.contains(prop_name);
      let is_optional = !is_required;

      let serde_attrs = Self::build_serde_attrs(prop_name, is_optional, rust_type.nullable);
      let metadata = self.extract_field_metadata(prop_name, is_required, prop_schema_ref);
      let regex_validation = Self::filter_regex_validation(&rust_type, metadata.regex_validation.clone());
      let final_type = Self::apply_optionality(rust_type, is_optional);

      fields.push(Self::build_field_def(
        prop_name,
        final_type,
        serde_attrs,
        metadata,
        regex_validation,
      ));
    }

    // Deduplicate field names that collide after conversion to snake_case
    Self::deduplicate_field_names(&mut fields);

    Ok((fields, inline_types))
  }

  /// Maps OpenAPI string format values to their corresponding Rust types
  fn map_string_format(format: Option<&String>) -> &'static str {
    format
      .map(|f| match f.as_str() {
        "date" => "chrono::NaiveDate",
        "date-time" => "chrono::DateTime<chrono::Utc>",
        "time" => "chrono::NaiveTime",
        "binary" => "Vec<u8>",
        "byte" => "String",
        "uuid" => "uuid::Uuid",
        _ => "String",
      })
      .unwrap_or("String")
  }

  /// Attempts to extract a schema name from a $ref path
  fn try_extract_ref_name(obj_ref: &ObjectOrReference<ObjectSchema>) -> Option<String> {
    match obj_ref {
      ObjectOrReference::Ref { ref_path, .. } => SchemaGraph::extract_ref_name(ref_path),
      _ => None,
    }
  }

  /// Extracts the first $ref from a schema's oneOf array, if present
  fn try_extract_first_oneof_ref(&self, schema: &ObjectSchema) -> Option<String> {
    schema.one_of.iter().find_map(Self::try_extract_ref_name)
  }

  /// Extracts all $ref names from a list of variants (oneOf/anyOf)
  fn extract_all_variant_refs(variants: &[ObjectOrReference<ObjectSchema>]) -> BTreeSet<String> {
    variants.iter().filter_map(Self::try_extract_ref_name).collect()
  }

  /// Finds an existing schema that has the same oneOf/anyOf variants
  /// Returns the schema name if a match is found
  fn find_matching_union_schema(&self, variants: &[ObjectOrReference<ObjectSchema>]) -> Option<String> {
    let variant_refs = Self::extract_all_variant_refs(variants);

    // Need at least 2 variants to be considered a union worth matching
    if variant_refs.len() < 2 {
      return None;
    }

    // Search through all schemas to find one with matching variants
    for schema_name in self.graph.schema_names() {
      if let Some(schema) = self.graph.get_schema(schema_name) {
        // Check oneOf variants
        if !schema.one_of.is_empty() {
          let schema_refs = Self::extract_all_variant_refs(&schema.one_of);
          if schema_refs == variant_refs {
            return Some(schema_name.clone());
          }
        }

        // Check anyOf variants
        if !schema.any_of.is_empty() {
          let schema_refs = Self::extract_all_variant_refs(&schema.any_of);
          if schema_refs == variant_refs {
            return Some(schema_name.clone());
          }
        }
      }
    }

    None
  }

  /// Checks if a schema represents a null type
  fn is_null_schema(schema: &ObjectSchema) -> bool {
    schema.schema_type == Some(SchemaTypeSet::Single(SchemaType::Null))
  }

  /// Returns true if the schema is nullable or contains null as one of its types
  fn is_nullable_or_generic(schema: &ObjectSchema) -> bool {
    // Check if it's explicitly nullable using the oas3 helper
    if schema.is_nullable() == Some(true) {
      return true;
    }
    // Check if it's a generic type like ["null", "object"] which is essentially a wildcard
    if let Some(SchemaTypeSet::Multiple(ref types)) = schema.schema_type {
      types.contains(&SchemaType::Null)
    } else {
      false
    }
  }

  /// Converts OpenAPI array schema items to a Rust TypeRef
  /// Returns the array element type without the Vec wrapper
  fn convert_array_items(&self, schema: &ObjectSchema) -> anyhow::Result<TypeRef> {
    let Some(ref items_box) = schema.items else {
      return Ok(TypeRef::new("serde_json::Value"));
    };

    let Schema::Object(items_ref) = items_box.as_ref() else {
      return Ok(TypeRef::new("serde_json::Value"));
    };

    if let Some(ref_name) = Self::try_extract_ref_name(items_ref) {
      return Ok(TypeRef::new(to_rust_type_name(&ref_name)));
    }

    let items_schema = items_ref.resolve(self.graph.spec())?;

    if let Some(ref_name) = self.try_extract_first_oneof_ref(&items_schema) {
      return Ok(TypeRef::new(to_rust_type_name(&ref_name)));
    }

    self.schema_to_type_ref(&items_schema)
  }

  /// Finds the non-null variant in a two-element union of [T, null]
  /// Returns None if not a nullable pattern
  fn find_non_null_variant<'b>(
    &self,
    variants: &'b [ObjectOrReference<ObjectSchema>],
  ) -> Option<&'b ObjectOrReference<ObjectSchema>> {
    if variants.len() != 2 {
      return None;
    }

    let has_null = variants.iter().any(|v| {
      v.resolve(self.graph.spec())
        .ok()
        .map(|s| Self::is_null_schema(&s))
        .unwrap_or(false)
    });

    if !has_null {
      return None;
    }

    variants.iter().find(|v| {
      v.resolve(self.graph.spec())
        .ok()
        .map(|s| !Self::is_null_schema(&s))
        .unwrap_or(false)
    })
  }

  /// Attempts to resolve a schema by its title property
  /// Returns a TypeRef with Box wrapper if the type is cyclic
  fn try_resolve_by_title(&self, schema: &ObjectSchema) -> Option<TypeRef> {
    let title = schema.title.as_ref()?;

    if self.graph.get_schema(title).is_none() || schema.properties.is_empty() {
      return None;
    }

    let mut type_ref = TypeRef::new(to_rust_type_name(title));
    if self.graph.is_cyclic(title) {
      type_ref = type_ref.with_boxed();
    }

    Some(type_ref)
  }

  /// Attempts to convert oneOf/anyOf union variants to a TypeRef
  /// Handles nullable patterns and type extraction from unions
  fn try_convert_union_to_type_ref(&self, variants: &[ObjectOrReference<ObjectSchema>]) -> Option<TypeRef> {
    // First, check if this inline union matches an existing schema definition
    if let Some(matching_schema) = self.find_matching_union_schema(variants) {
      let mut type_ref = TypeRef::new(to_rust_type_name(&matching_schema));
      // Apply Box wrapping if this schema is part of a cycle
      if self.graph.is_cyclic(&matching_schema) {
        type_ref = type_ref.with_boxed();
      }
      return Some(type_ref);
    }

    if let Some(non_null_variant) = self.find_non_null_variant(variants) {
      if let Some(ref_name) = Self::try_extract_ref_name(non_null_variant) {
        return Some(TypeRef::new(to_rust_type_name(&ref_name)).with_option());
      }

      if let Ok(resolved) = non_null_variant.resolve(self.graph.spec())
        && let Ok(inner_type) = self.schema_to_type_ref(&resolved)
      {
        return Some(inner_type.with_option());
      }
    }

    let mut fallback_type: Option<TypeRef> = None;

    for variant_ref in variants {
      if let Some(ref_name) = Self::try_extract_ref_name(variant_ref) {
        return Some(TypeRef::new(to_rust_type_name(&ref_name)));
      }

      let Ok(resolved) = variant_ref.resolve(self.graph.spec()) else {
        continue;
      };

      if Self::is_null_schema(&resolved) {
        continue;
      }

      if resolved.schema_type == Some(SchemaTypeSet::Single(SchemaType::Array))
        && let Ok(item_type) = self.convert_array_items(&resolved)
      {
        let unique_items = resolved.unique_items.unwrap_or(false);
        return Some(
          TypeRef::new(item_type.to_rust_type())
            .with_vec()
            .with_unique_items(unique_items),
        );
      }

      if resolved.schema_type == Some(SchemaTypeSet::Single(SchemaType::String)) && fallback_type.is_none() {
        fallback_type = Some(TypeRef::new("String"));
        continue;
      }

      if let Some(ref_name) = self.try_extract_first_oneof_ref(&resolved) {
        return Some(TypeRef::new(to_rust_type_name(&ref_name)));
      }

      if let Some(ref variant_title) = resolved.title
        && self.graph.get_schema(variant_title).is_some()
      {
        return Some(TypeRef::new(to_rust_type_name(variant_title)));
      }
    }

    fallback_type
  }

  /// Maps a single primitive SchemaType to a Rust TypeRef
  fn map_single_primitive_type(&self, schema_type: &SchemaType, schema: &ObjectSchema) -> anyhow::Result<TypeRef> {
    match schema_type {
      SchemaType::String => Ok(TypeRef::new(Self::map_string_format(schema.format.as_ref()))),
      SchemaType::Number => Ok(TypeRef::new("f64")),
      SchemaType::Integer => Ok(TypeRef::new("i64")),
      SchemaType::Boolean => Ok(TypeRef::new("bool")),
      SchemaType::Array => {
        let item_type = self.convert_array_items(schema)?;
        let unique_items = schema.unique_items.unwrap_or(false);
        Ok(
          TypeRef::new(item_type.to_rust_type())
            .with_vec()
            .with_unique_items(unique_items),
        )
      }
      SchemaType::Object => Ok(TypeRef::new("serde_json::Value")),
      SchemaType::Null => Ok(TypeRef::new("()").with_option()),
    }
  }

  /// Converts nullable primitive types from SchemaTypeSet::Multiple
  /// Detects [T, null] patterns and returns Option<T>
  fn convert_nullable_primitive(&self, types: &[SchemaType], schema: &ObjectSchema) -> anyhow::Result<TypeRef> {
    let type_vec: Vec<_> = types.iter().collect();

    if type_vec.len() != 2 {
      return Ok(TypeRef::new("serde_json::Value"));
    }

    let has_null = type_vec.iter().any(|t| matches!(t, SchemaType::Null));
    if !has_null {
      return Ok(TypeRef::new("serde_json::Value"));
    }

    let Some(non_null_type) = type_vec.iter().find(|t| !matches!(t, SchemaType::Null)) else {
      return Ok(TypeRef::new("serde_json::Value"));
    };

    let type_ref = self.map_single_primitive_type(non_null_type, schema)?;
    Ok(type_ref.with_option())
  }

  /// Converts an OpenAPI schema to a Rust TypeRef
  ///
  /// This is the main entry point for type conversion. It handles:
  /// - Title-based schema references (with cycle detection)
  /// - Union types (oneOf/anyOf) including nullable patterns
  /// - Primitive types (string, number, integer, boolean, array, object, null)
  /// - Nullable primitives using SchemaTypeSet::Multiple
  pub(crate) fn schema_to_type_ref(&self, schema: &ObjectSchema) -> anyhow::Result<TypeRef> {
    if let Some(ref schema_type) = schema.schema_type {
      if matches!(schema_type, SchemaTypeSet::Single(SchemaType::Object))
        && let Some(type_ref) = self.try_resolve_by_title(schema)
      {
        return Ok(type_ref);
      }
    } else if let Some(type_ref) = self.try_resolve_by_title(schema) {
      return Ok(type_ref);
    }

    if !schema.one_of.is_empty() || !schema.any_of.is_empty() {
      let variants = if !schema.one_of.is_empty() {
        &schema.one_of
      } else {
        &schema.any_of
      };

      if let Some(type_ref) = self.try_convert_union_to_type_ref(variants) {
        return Ok(type_ref);
      }
    }

    if let Some(ref schema_type) = schema.schema_type {
      return match schema_type {
        SchemaTypeSet::Single(typ) => self.map_single_primitive_type(typ, schema),
        SchemaTypeSet::Multiple(types) => self.convert_nullable_primitive(types, schema),
      };
    }

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

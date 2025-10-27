use std::{
  cmp::Reverse,
  collections::{BTreeMap, BTreeSet, HashMap, HashSet},
};

use oas3::spec::{ObjectOrReference, ObjectSchema, Schema, SchemaType, SchemaTypeSet};
use regex::Regex;
use serde_json::Number;

use super::{
  ast::{EnumDef, FieldDef, RustType, StructDef, TypeAliasDef, TypeRef, VariantContent, VariantDef},
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

#[derive(Copy, Clone)]
enum InlinePolicy {
  None,
  InlineUnions, // generate inline enums for oneOf/anyOf in properties
}

#[derive(Copy, Clone, PartialEq, Eq)]
enum UnionKind {
  OneOf,
  AnyOf,
}

/// Converter that transforms OpenAPI schemas into Rust AST structures
pub(crate) struct SchemaConverter<'a> {
  graph: &'a SchemaGraph,
}

impl<'a> SchemaConverter<'a> {
  pub(crate) fn new(graph: &'a SchemaGraph) -> Self {
    Self { graph }
  }

  /// Convenience for doc lines
  fn docs(desc: Option<&String>) -> Vec<String> {
    desc.map(|d| doc_comment_lines(d)).unwrap_or_default()
  }

  /// Derives for struct, respecting read/write-only directions
  fn derives_for_struct(all_read_only: bool, all_write_only: bool) -> Vec<String> {
    let mut derives = vec!["Debug".into(), "Clone".into(), "PartialEq".into()];
    if !all_read_only {
      derives.push("Serialize".into());
    }
    if !all_write_only {
      derives.push("Deserialize".into());
    }
    derives.push("validator::Validate".into());
    derives.push("oas3_gen_support::Default".into());
    derives
  }

  /// Derives for enum, optionally include Eq
  fn derives_for_enum() -> Vec<String> {
    let derives = vec![
      "Debug".into(),
      "Clone".into(),
      "PartialEq".into(),
      "Serialize".into(),
      "Deserialize".into(),
      "oas3_gen_support::Default".into(),
    ];
    derives
  }

  /// Check if a schema is a discriminated base type (has discriminator with mappings and properties)
  fn is_discriminated_base_type(&self, schema: &ObjectSchema) -> bool {
    schema
      .discriminator
      .as_ref()
      .and_then(|d| d.mapping.as_ref().map(|m| !m.is_empty()))
      .unwrap_or(false)
      && !schema.properties.is_empty()
  }

  /// Compute the inheritance depth of a schema (0 for schemas with no allOf)
  fn compute_inheritance_depth(&self, schema_name: &str, memo: &mut HashMap<String, usize>) -> usize {
    if let Some(&depth) = memo.get(schema_name) {
      return depth;
    }
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
        SchemaGraph::extract_ref_name(ref_path).map(|schema_name| (disc_value.clone(), schema_name))
      })
      .collect();

    let mut depth_memo = HashMap::new();
    children.sort_by_key(|(_, schema_name)| Reverse(self.compute_inheritance_depth(schema_name, &mut depth_memo)));
    children
  }

  /// Determine container-level attributes for a struct based on its fields
  pub(crate) fn container_outer_attrs(fields: &[FieldDef]) -> Vec<String> {
    if fields.iter().any(|field| field.rust_type.nullable) {
      vec!["oas3_gen_support::skip_serializing_none".into()]
    } else {
      Vec::new()
    }
  }

  /// Finds the discriminator field name for a schema that appears in another schema's discriminator mapping.
  fn find_discriminator_mapping_value(&self, schema_name: &str) -> Option<(String, String)> {
    for candidate_name in self.graph.schema_names() {
      let Some(candidate_schema) = self.graph.get_schema(candidate_name) else {
        continue;
      };
      let Some(discriminator) = candidate_schema.discriminator.as_ref() else {
        continue;
      };
      let Some(mapping) = discriminator.mapping.as_ref() else {
        continue;
      };

      for (disc_value, ref_path) in mapping {
        if SchemaGraph::extract_ref_name(ref_path)
          .as_deref()
          .is_some_and(|mapped| mapped == schema_name)
        {
          return Some((discriminator.property_name.clone(), disc_value.clone()));
        }
      }
    }

    None
  }

  /// Convert a child schema that extends a discriminated parent
  fn convert_discriminated_child(
    &self,
    name: &str,
    schema: &ObjectSchema,
    _parent_name: &str,
    parent_schema: &ObjectSchema,
  ) -> anyhow::Result<Vec<RustType>> {
    let struct_name = to_rust_type_name(name);

    let Some(_discriminator_prop_name) = parent_schema.discriminator.as_ref().map(|d| d.property_name.clone()) else {
      return Err(anyhow::anyhow!("Parent schema is not discriminated"));
    };

    let mut merged_properties = BTreeMap::new();
    let mut merged_required = Vec::new();
    let mut merged_discriminator = parent_schema.discriminator.clone();

    self.collect_all_of_properties(
      schema,
      &mut merged_properties,
      &mut merged_required,
      &mut merged_discriminator,
    )?;

    let mut merged_schema = schema.clone();
    merged_schema.properties = merged_properties;
    merged_schema.required = merged_required;
    merged_schema.discriminator = merged_discriminator;
    merged_schema.all_of.clear();
    if merged_schema.additional_properties.is_none() {
      merged_schema.additional_properties = parent_schema.additional_properties.clone();
    }

    let (mut child_fields, inline_types) = self.convert_fields_core(
      &struct_name,
      &merged_schema,
      InlinePolicy::InlineUnions,
      None,
      Some(name),
    )?;

    let mut serde_attrs = Vec::new();

    if let Some(ref additional) = merged_schema.additional_properties {
      match additional {
        Schema::Boolean(bool_schema) => {
          if !bool_schema.0 {
            serde_attrs.push("deny_unknown_fields".to_string());
          }
        }
        Schema::Object(schema_ref) => {
          if let Ok(additional_schema) = schema_ref.resolve(self.graph.spec()) {
            let value_type = self.schema_to_type_ref(&additional_schema)?;
            let map_type = TypeRef::new(format!(
              "std::collections::HashMap<String, {}>",
              value_type.to_rust_type()
            ));
            child_fields.push(FieldDef {
              name: "additional_properties".to_string(),
              docs: vec!["/// Additional properties not defined in the schema".to_string()],
              rust_type: map_type,
              serde_attrs: vec!["flatten".to_string()],
              extra_attrs: vec![],
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

    let fields = child_fields;

    let all_read_only = !fields.is_empty() && fields.iter().all(|f| f.read_only);
    let all_write_only = !fields.is_empty() && fields.iter().all(|f| f.write_only);
    let derives = Self::derives_for_struct(all_read_only, all_write_only);

    let outer_attrs = Self::container_outer_attrs(&fields);

    let struct_type = RustType::Struct(StructDef {
      name: struct_name,
      docs: Self::docs(schema.description.as_ref()),
      fields,
      derives,
      serde_attrs,
      outer_attrs,
      methods: vec![],
    });

    let mut all_types = vec![struct_type];
    all_types.extend(inline_types);
    Ok(all_types)
  }

  /// Create a discriminated enum for schemas with discriminator mappings
  fn create_discriminated_enum(
    &self,
    base_name: &str,
    schema: &ObjectSchema,
    base_struct_name: &str,
  ) -> anyhow::Result<RustType> {
    use crate::generator::ast::{DiscriminatedEnumDef, DiscriminatedVariant};

    let children = self.extract_discriminator_children(schema);
    let enum_name = to_rust_type_name(base_name);

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
      let variant_name = if variant_name.is_empty() {
        child_type_name.clone()
      } else {
        variant_name
      };

      variants.push(DiscriminatedVariant {
        discriminator_value: disc_value,
        variant_name,
        type_name: format!("Box<{}>", child_type_name), // always boxed to avoid recursion
      });
    }

    // Add fallback variant (base type)
    let base_variant_name = to_rust_type_name(base_name.split('.').next_back().unwrap_or(base_name));
    let fallback = Some(DiscriminatedVariant {
      discriminator_value: "".into(),
      variant_name: base_variant_name,
      type_name: format!("Box<{}>", base_struct_name),
    });

    Ok(RustType::DiscriminatedEnum(DiscriminatedEnumDef {
      name: enum_name,
      docs: Self::docs(schema.description.as_ref()),
      discriminator_field,
      variants,
      fallback,
    }))
  }

  /// Convert a schema to Rust type definitions
  pub(crate) fn convert_schema(&self, name: &str, schema: &ObjectSchema) -> anyhow::Result<Vec<RustType>> {
    if !schema.all_of.is_empty() {
      return self.convert_all_of_schema(name, schema);
    }

    if !schema.one_of.is_empty() {
      return self.convert_union_enum(name, schema, UnionKind::OneOf);
    }

    if !schema.any_of.is_empty() {
      return self.convert_union_enum(name, schema, UnionKind::AnyOf);
    }

    if !schema.enum_values.is_empty() {
      return Ok(vec![self.convert_simple_enum(name, schema, &schema.enum_values)?]);
    }

    if !schema.properties.is_empty() {
      let is_discriminated = self.is_discriminated_base_type(schema);
      let (main_type, mut inline_types) = self.convert_struct(name, schema)?;
      let mut all_types = Vec::new();
      if is_discriminated {
        let base_struct_name = match &main_type {
          RustType::Struct(def) => def.name.clone(),
          _ => format!("{}Base", to_rust_type_name(name)),
        };
        let discriminated_enum = self.create_discriminated_enum(name, schema, &base_struct_name)?;
        all_types.push(discriminated_enum);
      }
      all_types.push(main_type);
      all_types.append(&mut inline_types);
      return Ok(all_types);
    }

    let alias = RustType::TypeAlias(TypeAliasDef {
      name: to_rust_type_name(name),
      docs: Self::docs(schema.description.as_ref()),
      target: TypeRef::new("serde_json::Value"),
    });

    Ok(vec![alias])
  }

  /// Recursively collect all properties, required fields, and discriminators from a schema's allOf chain
  fn collect_all_of_properties(
    &self,
    schema: &ObjectSchema,
    properties: &mut BTreeMap<String, ObjectOrReference<ObjectSchema>>,
    required: &mut Vec<String>,
    discriminator: &mut Option<oas3::spec::Discriminator>,
  ) -> anyhow::Result<()> {
    for all_of_ref in &schema.all_of {
      if let Ok(all_of_schema) = all_of_ref.resolve(self.graph.spec()) {
        self.collect_all_of_properties(&all_of_schema, properties, required, discriminator)?;
      }
    }

    for (prop_name, prop_ref) in &schema.properties {
      properties.insert(prop_name.clone(), prop_ref.clone());
    }

    for req in &schema.required {
      if !required.contains(req) {
        required.push(req.clone());
      }
    }

    if schema.discriminator.is_some() {
      *discriminator = schema.discriminator.clone();
    }

    Ok(())
  }

  /// Convert an allOf schema by merging all schemas into one struct
  fn convert_all_of_schema(&self, name: &str, schema: &ObjectSchema) -> anyhow::Result<Vec<RustType>> {
    // Detect discriminated parent in allOf refs
    let discriminated_parent = schema.all_of.iter().find_map(|all_of_ref| {
      let ObjectOrReference::Ref { ref_path, .. } = all_of_ref else {
        return None;
      };
      let parent_name = SchemaGraph::extract_ref_name(ref_path)?;

      let parent_ref = self.graph.spec().components.as_ref()?.schemas.get(&parent_name)?;
      let parent_schema = parent_ref.resolve(self.graph.spec()).ok()?;

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

      if self.is_discriminated_base_type(&merged_parent) {
        Some((parent_name, merged_parent))
      } else {
        None
      }
    });

    if let Some((parent_name, parent_schema)) = discriminated_parent {
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

    let mut merged_schema = schema.clone();
    merged_schema.properties = merged_properties;
    merged_schema.required = merged_required;
    merged_schema.discriminator = merged_discriminator.clone();

    let is_discriminated = self.is_discriminated_base_type(&merged_schema);
    let (main_type, mut inline_types) = self.convert_struct(name, &merged_schema)?;

    let mut all_types = Vec::new();
    if is_discriminated {
      let base_struct_name = match &main_type {
        RustType::Struct(def) => def.name.clone(),
        _ => format!("{}Base", to_rust_type_name(name)),
      };
      let discriminated_enum = self.create_discriminated_enum(name, &merged_schema, &base_struct_name)?;
      all_types.push(discriminated_enum);
    }
    all_types.push(main_type);

    all_types.append(&mut inline_types);
    Ok(all_types)
  }

  /// Unified converter for oneOf and anyOf enums (keeps anyOf special-case for forward-compatible string enums)
  fn convert_union_enum(&self, name: &str, schema: &ObjectSchema, kind: UnionKind) -> anyhow::Result<Vec<RustType>> {
    // anyOf special-case: catch-all string enums (const values + freeform string)
    if kind == UnionKind::AnyOf {
      let has_freeform_string = schema.any_of.iter().any(|s| {
        if let Ok(resolved) = s.resolve(self.graph.spec()) {
          resolved.const_value.is_none() && resolved.schema_type == Some(SchemaTypeSet::Single(SchemaType::String))
        } else {
          false
        }
      });

      let mut seen_values = HashSet::new();
      let mut known_values = Vec::new();

      for variant in &schema.any_of {
        let Ok(resolved) = variant.resolve(self.graph.spec()) else {
          continue;
        };

        if let Some(const_value) = resolved.const_value.as_ref() {
          if let Some(str_val) = const_value.as_str()
            && seen_values.insert(str_val.to_string())
          {
            known_values.push((
              serde_json::Value::String(str_val.to_string()),
              resolved.description.clone(),
              resolved.deprecated.unwrap_or(false),
            ));
          }
          continue;
        }

        if resolved.schema_type == Some(SchemaTypeSet::Single(SchemaType::String)) && !resolved.enum_values.is_empty() {
          for enum_value in &resolved.enum_values {
            if let Some(str_val) = enum_value.as_str()
              && seen_values.insert(str_val.to_string())
            {
              known_values.push((
                serde_json::Value::String(str_val.to_string()),
                resolved.description.clone(),
                resolved.deprecated.unwrap_or(false),
              ));
            }
          }
        }
      }

      if has_freeform_string && !known_values.is_empty() {
        return self.convert_string_enum_with_catchall(name, schema, &known_values);
      }
    }

    let variants_src = match kind {
      UnionKind::OneOf => &schema.one_of,
      UnionKind::AnyOf => &schema.any_of,
    };

    let discriminator_prop = if kind == UnionKind::OneOf {
      schema.discriminator.as_ref().map(|d| d.property_name.as_str())
    } else {
      None
    };

    let discriminator_map: BTreeMap<String, String> = if kind == UnionKind::OneOf {
      schema
        .discriminator
        .as_ref()
        .and_then(|d| d.mapping.as_ref())
        .map(|mapping| {
          mapping
            .iter()
            .filter_map(|(val, ref_path)| SchemaGraph::extract_ref_name(ref_path).map(|name| (name, val.clone())))
            .collect()
        })
        .unwrap_or_default()
    } else {
      BTreeMap::new()
    };

    let mut inline_types = Vec::new();
    let mut variants = Vec::new();
    let mut seen_names = BTreeSet::new();

    for (i, variant_schema_ref) in variants_src.iter().enumerate() {
      // capture $ref name before resolving
      let ref_schema_name = if let ObjectOrReference::Ref { ref_path, .. } = variant_schema_ref {
        SchemaGraph::extract_ref_name(ref_path)
      } else {
        None
      };

      let Ok(resolved) = variant_schema_ref.resolve(self.graph.spec()) else {
        continue;
      };

      // Skip null - handled as Option at field sites, not variant
      if resolved.schema_type == Some(SchemaTypeSet::Single(SchemaType::Null)) {
        continue;
      }

      // If this was a ref into components, make a tuple variant of that type (box if cyclic)
      if let Some(ref schema_name) = ref_schema_name {
        let rust_type_name = to_rust_type_name(schema_name);
        let mut type_ref = TypeRef::new(&rust_type_name);
        if self.graph.is_cyclic(schema_name) {
          type_ref = type_ref.with_boxed();
        }

        let docs = Self::docs(resolved.description.as_ref());
        let deprecated = resolved.deprecated.unwrap_or(false);

        let mut variant_name = rust_type_name.clone();
        if !seen_names.insert(variant_name.clone()) {
          variant_name = format!("{}{}", variant_name, i);
          seen_names.insert(variant_name.clone());
        }

        let mut serde_attrs = Vec::new();
        if discriminator_prop.is_some()
          && let Some(disc_value) = discriminator_map.get(schema_name)
        {
          serde_attrs.push(format!("rename = \"{}\"", disc_value));
          // note: in internally tagged enums, serde rename at variant level maps discriminator value
        }

        variants.push(VariantDef {
          name: variant_name,
          docs,
          content: VariantContent::Tuple(vec![type_ref]),
          serde_attrs,
          deprecated,
        });
        continue;
      }

      // Inline variant - naming + content
      let mut variant_name = if let Some(ref title) = resolved.title {
        to_rust_type_name(title)
      } else {
        self.infer_variant_name(&resolved, i)
      };

      if !seen_names.insert(variant_name.clone()) {
        variant_name = format!("{}{}", variant_name, i);
        seen_names.insert(variant_name.clone());
      }

      let docs = Self::docs(resolved.description.as_ref());
      let deprecated = resolved.deprecated.unwrap_or(false);

      let (content, mut generated_types) = if let Some(disc_prop) = discriminator_prop {
        // internally tagged wants struct-like variants
        if !resolved.properties.is_empty() {
          self
            .convert_fields_core(name, &resolved, InlinePolicy::InlineUnions, Some(disc_prop), None)
            .map_or_else(
              |_e| Ok::<_, anyhow::Error>((VariantContent::Struct(vec![]), vec![])),
              |(fields, tys)| Ok((VariantContent::Struct(fields), tys)),
            )?
        } else {
          let field = FieldDef {
            name: "value".to_string(),
            rust_type: self.schema_to_type_ref(&resolved)?,
            ..Default::default()
          };
          (VariantContent::Struct(vec![field]), vec![])
        }
      } else if !resolved.properties.is_empty() {
        let fields = self.convert_fields(&resolved)?;
        (VariantContent::Struct(fields), vec![])
      } else {
        let type_ref = self.schema_to_type_ref(&resolved)?;
        (VariantContent::Tuple(vec![type_ref]), vec![])
      };

      inline_types.append(&mut generated_types);

      variants.push(VariantDef {
        name: variant_name,
        docs,
        content,
        serde_attrs: vec![],
        deprecated,
      });
    }

    // Strip common prefix/suffix for clippy::enum_variant_names
    let original_names: Vec<String> = variants.iter().map(|v| v.name.clone()).collect();
    let stripped_names = Self::strip_common_affixes(&original_names);
    for (variant, stripped_name) in variants.iter_mut().zip(stripped_names.iter()) {
      variant.name = stripped_name.clone();
    }

    // For untagged (anyOf) fix self-referential struct fields by adding Box to fields of the same enum type
    if kind == UnionKind::AnyOf {
      let enum_name = to_rust_type_name(name);
      for variant in &mut variants {
        if let VariantContent::Struct(ref mut fields) = variant.content {
          for field in fields {
            if field.rust_type.base_type == enum_name && !field.rust_type.boxed {
              field.rust_type = field.rust_type.clone().with_boxed();
            }
          }
        }
      }
    }

    let (serde_attrs, derives) = if kind == UnionKind::AnyOf {
      (vec!["untagged".into()], Self::derives_for_enum())
    } else {
      (vec![], Self::derives_for_enum())
    };

    let main_enum = RustType::Enum(EnumDef {
      name: to_rust_type_name(name),
      docs: Self::docs(schema.description.as_ref()),
      variants,
      discriminator: schema.discriminator.as_ref().map(|d| d.property_name.clone()),
      derives,
      serde_attrs,
      outer_attrs: vec![], // no outer attrs needed for enums
    });

    // Preserve convert_one_of_enum behavior: place generated inline types first, then enum
    let mut out = inline_types;
    out.push(main_enum);
    Ok(out)
  }

  /// Convert a string enum with const values + a catch-all for unknown strings
  fn convert_string_enum_with_catchall(
    &self,
    name: &str,
    schema: &ObjectSchema,
    const_values: &[(serde_json::Value, Option<String>, bool)],
  ) -> anyhow::Result<Vec<RustType>> {
    let base_name = to_rust_type_name(name);
    let known_name = format!("{}Known", base_name);

    // Inner enum with known values
    let mut known_variants = Vec::new();
    let mut seen_names = BTreeSet::new();

    for (i, (value, description, deprecated)) in const_values.iter().enumerate() {
      if let Some(str_val) = value.as_str() {
        let mut variant_name = to_rust_type_name(str_val);
        if !seen_names.insert(variant_name.clone()) {
          variant_name = format!("{}{}", variant_name, i);
          seen_names.insert(variant_name.clone());
        }
        let docs = Self::docs(description.as_ref());
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
      derives: Self::derives_for_enum(),
      serde_attrs: vec![],
      outer_attrs: vec![],
    });

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
      docs: Self::docs(schema.description.as_ref()),
      variants: outer_variants,
      discriminator: None,
      derives: Self::derives_for_enum(),
      serde_attrs: vec!["untagged".into()],
      outer_attrs: vec![],
    });

    Ok(vec![inner_enum, outer_enum])
  }

  /// Infer a variant name from the schema type
  fn infer_variant_name(&self, schema: &ObjectSchema, index: usize) -> String {
    if !schema.enum_values.is_empty() {
      return "Enum".to_string();
    }
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
      format!("Variant{}", index)
    }
  }

  /// Split a PascalCase name into words
  fn split_pascal_case(name: &str) -> Vec<String> {
    let mut words = Vec::new();
    let mut current_word = String::new();

    for (i, ch) in name.chars().enumerate() {
      if ch.is_uppercase() && i > 0 && !current_word.is_empty() {
        words.push(std::mem::take(&mut current_word));
      }
      current_word.push(ch);
    }
    if !current_word.is_empty() {
      words.push(current_word);
    }
    words
  }

  /// Strip common prefix/suffix from enum variant names
  fn strip_common_affixes(variant_names: &[String]) -> Vec<String> {
    if variant_names.len() < 3 {
      return variant_names.to_vec();
    }

    let split_names: Vec<Vec<String>> = variant_names.iter().map(|n| Self::split_pascal_case(n)).collect();

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

    let mut stripped_names = Vec::new();
    for words in &split_names {
      let start = common_prefix_len;
      let end = words.len().saturating_sub(common_suffix_len);
      if start >= end {
        stripped_names.push(words.join(""));
      } else {
        stripped_names.push(words[start..end].join(""));
      }
    }

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
        if !seen_names.insert(variant_name.clone()) {
          variant_name = format!("{}{}", variant_name, i);
          seen_names.insert(variant_name.clone());
        }
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
      docs: Self::docs(schema.description.as_ref()),
      variants,
      discriminator: None,
      derives: Self::derives_for_enum(),
      serde_attrs: vec![],
      outer_attrs: vec![],
    }))
  }

  /// Convert an object schema to a Rust struct
  pub(crate) fn convert_struct(&self, name: &str, schema: &ObjectSchema) -> anyhow::Result<(RustType, Vec<RustType>)> {
    let is_discriminated = self.is_discriminated_base_type(schema);

    let struct_name_base = to_rust_type_name(name);
    let struct_name = if is_discriminated {
      format!("{}Base", struct_name_base)
    } else {
      struct_name_base.clone()
    };

    let (mut fields, inline_types) =
      self.convert_fields_core(&struct_name, schema, InlinePolicy::InlineUnions, None, Some(name))?;

    // Serde container attributes
    let mut serde_attrs = vec![];

    // additionalProperties handling
    if let Some(ref additional) = schema.additional_properties {
      match additional {
        Schema::Boolean(bool_schema) => {
          if !bool_schema.0 {
            // Important: deny_unknown_fields is incompatible with flatten. We only add it here
            // because this branch does not add any flatten fields.
            serde_attrs.push("deny_unknown_fields".to_string());
          }
        }
        Schema::Object(schema_ref) => {
          if let Ok(additional_schema) = schema_ref.resolve(self.graph.spec()) {
            let value_type = self.schema_to_type_ref(&additional_schema)?;
            let map_type = TypeRef::new(format!(
              "std::collections::HashMap<String, {}>",
              value_type.to_rust_type()
            ));
            // Flatten map of additional properties
            fields.push(FieldDef {
              name: "additional_properties".to_string(),
              docs: vec!["/// Additional properties not defined in the schema".to_string()],
              rust_type: map_type,
              serde_attrs: vec!["flatten".to_string()],
              extra_attrs: vec![],
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

    // serde(default) at struct level if useful (unchanged policy)
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

    let all_read_only = !fields.is_empty() && fields.iter().all(|f| f.read_only);
    let all_write_only = !fields.is_empty() && fields.iter().all(|f| f.write_only);

    let derives = Self::derives_for_struct(all_read_only, all_write_only);
    let outer_attrs = Self::container_outer_attrs(&fields);

    let struct_type = RustType::Struct(StructDef {
      name: struct_name,
      docs: Self::docs(schema.description.as_ref()),
      fields,
      derives,
      serde_attrs,
      outer_attrs,
      methods: vec![],
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
      // Keep stable formatting, ensure trailing .0 for integers
      let s = num.to_string();
      if s.contains('.') { s } else { format!("{}.0", s) }
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

        if let Some(exclusive_min) = schema
          .exclusive_minimum
          .as_ref()
          .map(|v| format!("exclusive_min = {}", Self::render_number(is_float, v)))
        {
          parts.push(exclusive_min);
        }
        if let Some(exclusive_max) = schema
          .exclusive_maximum
          .as_ref()
          .map(|v| format!("exclusive_max = {}", Self::render_number(is_float, v)))
        {
          parts.push(exclusive_max);
        }
        if let Some(min) = schema
          .minimum
          .as_ref()
          .map(|v| format!("min = {}", Self::render_number(is_float, v)))
        {
          parts.push(min);
        }
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
            attrs.push("length(min = 1)".to_string());
          }
        }
      }

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

  pub(crate) fn extract_default_value(&self, schema: &ObjectSchema) -> Option<serde_json::Value> {
    if let Some(default) = schema.default.clone() {
      return Some(default);
    }

    if let Some(const_value) = schema.const_value.clone() {
      return Some(const_value);
    }

    if schema.enum_values.len() == 1 {
      return schema.enum_values.first().cloned();
    }

    None
  }

  /// Resolves a property type with special handling for inline anyOf/oneOf unions
  fn resolve_property_type_with_inline_enums(
    &self,
    parent_name: &str,
    prop_name: &str,
    prop_schema_ref: &ObjectOrReference<ObjectSchema>,
  ) -> anyhow::Result<(TypeRef, Vec<RustType>)> {
    match prop_schema_ref {
      ObjectOrReference::Ref { ref_path, .. } => {
        if let Some(ref_name) = SchemaGraph::extract_ref_name(ref_path) {
          let mut type_ref = TypeRef::new(to_rust_type_name(&ref_name));
          if self.graph.is_cyclic(&ref_name) {
            type_ref = type_ref.with_boxed();
          }
          Ok((type_ref, vec![]))
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

        let has_one_of = !prop_schema.one_of.is_empty();
        let has_any_of = !prop_schema.any_of.is_empty();

        if !has_one_of && !has_any_of {
          return Ok((self.schema_to_type_ref(&prop_schema)?, vec![]));
        }

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

        if let Some(matching_schema) = self.find_matching_union_schema(variants) {
          let mut type_ref = TypeRef::new(to_rust_type_name(&matching_schema));
          if self.graph.is_cyclic(&matching_schema) {
            type_ref = type_ref.with_boxed();
          }
          return Ok((type_ref, vec![]));
        }

        let should_generate_inline_enum = prop_schema
          .title
          .as_ref()
          .is_none_or(|t| self.graph.get_schema(t).is_none());

        if should_generate_inline_enum {
          let enum_name = format!("{}.{}", parent_name, prop_name);
          let enum_types = if has_one_of {
            self.convert_union_enum(&enum_name, &prop_schema, UnionKind::OneOf)?
          } else {
            self.convert_union_enum(&enum_name, &prop_schema, UnionKind::AnyOf)?
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

  /// Converts a property schema reference to a TypeRef (no inline enums)
  fn resolve_property_type(&self, prop_schema_ref: &ObjectOrReference<ObjectSchema>) -> anyhow::Result<TypeRef> {
    match prop_schema_ref {
      ObjectOrReference::Ref { ref_path, .. } => {
        if let Some(ref_name) = SchemaGraph::extract_ref_name(ref_path) {
          let mut type_ref = TypeRef::new(to_rust_type_name(&ref_name));
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
      let docs = Self::docs(prop_schema.description.as_ref());
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

  /// Builds serde attributes for a field (rename only; skip_serializing_if is handled at container-level by serde_with)
  fn build_serde_attrs(prop_name: &str) -> Vec<String> {
    let mut serde_attrs = vec![];
    let rust_field_name = to_rust_field_name(prop_name);
    if rust_field_name != prop_name {
      serde_attrs.push(format!("rename = \"{}\"", prop_name));
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
  fn deduplicate_field_names(fields: &mut Vec<FieldDef>) {
    let mut name_groups: HashMap<String, Vec<usize>> = HashMap::new();
    for (idx, field) in fields.iter().enumerate() {
      name_groups.entry(field.name.clone()).or_default().push(idx);
    }

    let mut indices_to_remove = HashSet::<usize>::new();

    for (name, indices) in name_groups {
      if indices.len() <= 1 {
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
    extra_attrs: Vec<String>,
  ) -> FieldDef {
    FieldDef {
      name: to_rust_field_name(prop_name),
      docs: metadata.docs,
      rust_type,
      serde_attrs,
      extra_attrs,
      validation_attrs: metadata.validation_attrs,
      regex_validation,
      default_value: metadata.default_value,
      read_only: metadata.read_only,
      write_only: metadata.write_only,
      deprecated: metadata.deprecated,
      multiple_of: metadata.multiple_of,
    }
  }

  /// Single, policy-driven field converter (replaces the 3 previous variants)
  fn convert_fields_core(
    &self,
    parent_name: &str,
    schema: &ObjectSchema,
    policy: InlinePolicy,
    exclude_field: Option<&str>,
    schema_name: Option<&str>,
  ) -> anyhow::Result<(Vec<FieldDef>, Vec<RustType>)> {
    let mut fields = Vec::new();
    let mut inline_types = Vec::new();

    // required membership O(1)
    let required_set: HashSet<&str> = schema.required.iter().map(|s| s.as_str()).collect();

    // schema.properties is a BTreeMap -> iteration order is already sorted
    for (prop_name, prop_schema_ref) in &schema.properties {
      if exclude_field.is_some() && exclude_field == Some(prop_name.as_str()) {
        continue;
      }

      let (rust_type, generated_types) = match policy {
        InlinePolicy::None => (self.resolve_property_type(prop_schema_ref)?, Vec::new()),
        InlinePolicy::InlineUnions => {
          self.resolve_property_type_with_inline_enums(parent_name, prop_name, prop_schema_ref)?
        }
      };
      inline_types.extend(generated_types);

      let is_required = required_set.contains(prop_name.as_str());
      let final_type = Self::apply_optionality(rust_type, !is_required);

      let mut metadata = self.extract_field_metadata(prop_name, is_required, prop_schema_ref);
      let mut serde_attrs = Self::build_serde_attrs(prop_name);
      let mut extra_attrs = Vec::new();

      let mut regex_validation = Self::filter_regex_validation(&final_type, metadata.regex_validation.clone());

      if self.graph.discriminator_fields().contains(prop_name) {
        metadata.docs.clear();
        metadata.validation_attrs.clear();
        regex_validation = None;
        extra_attrs.push("#[doc(hidden)]".to_string());

        if metadata.default_value.is_none()
          && let Some(schema_name) = schema_name
          && let Some((disc_prop, disc_value)) = self.find_discriminator_mapping_value(schema_name)
          && disc_prop == *prop_name
        {
          metadata.default_value = Some(serde_json::Value::String(disc_value));
        }

        if !serde_attrs.iter().any(|attr| attr == "default") {
          serde_attrs.push("default".to_string());
        }

        if metadata.default_value.is_none() && final_type.base_type == "String" && !final_type.is_array {
          metadata.default_value = Some(serde_json::Value::String(String::new()));
        }

        if metadata
          .default_value
          .as_ref()
          .is_none_or(|value| value.as_str().is_some_and(|s| s.is_empty()))
        {
          serde_attrs.push("skip".to_string());
        }
      }

      fields.push(Self::build_field_def(
        prop_name,
        final_type,
        serde_attrs,
        metadata,
        regex_validation,
        extra_attrs,
      ));
    }

    Self::deduplicate_field_names(&mut fields);
    Ok((fields, inline_types))
  }

  /// Convert schema properties to struct fields (convenience wrapper)
  fn convert_fields(&self, schema: &ObjectSchema) -> anyhow::Result<Vec<FieldDef>> {
    self
      .convert_fields_core("<inline>", schema, InlinePolicy::None, None, None)
      .map(|(f, _)| f)
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
  fn find_matching_union_schema(&self, variants: &[ObjectOrReference<ObjectSchema>]) -> Option<String> {
    let variant_refs = Self::extract_all_variant_refs(variants);
    if variant_refs.len() < 2 {
      return None;
    }

    for schema_name in self.graph.schema_names() {
      if let Some(schema) = self.graph.get_schema(schema_name) {
        if !schema.one_of.is_empty() {
          let schema_refs = Self::extract_all_variant_refs(&schema.one_of);
          if schema_refs == variant_refs {
            return Some(schema_name.clone());
          }
        }
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
    if schema.is_nullable() == Some(true) {
      return true;
    }
    if let Some(SchemaTypeSet::Multiple(ref types)) = schema.schema_type {
      types.contains(&SchemaType::Null)
    } else {
      false
    }
  }

  /// Converts OpenAPI array schema items to a Rust TypeRef (returns element type, caller adds Vec)
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
  fn try_convert_union_to_type_ref(&self, variants: &[ObjectOrReference<ObjectSchema>]) -> Option<TypeRef> {
    if let Some(matching_schema) = self.find_matching_union_schema(variants) {
      let mut type_ref = TypeRef::new(to_rust_type_name(&matching_schema));
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

  /// Converts nullable primitive types from SchemaTypeSet::Multiple -> Option<T>
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

  /// Converts OpenAPI schema to TypeRef
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

  use oas3::spec::{BooleanSchema, Discriminator, Spec};
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

  #[test]
  fn test_discriminated_base_struct_renamed_and_enum_references_it() {
    let mut entity_schema = ObjectSchema {
      schema_type: Some(SchemaTypeSet::Single(SchemaType::Object)),
      additional_properties: Some(Schema::Boolean(BooleanSchema(false))),
      ..Default::default()
    };

    entity_schema.properties.insert(
      "id".to_string(),
      ObjectOrReference::Object(ObjectSchema {
        schema_type: Some(SchemaTypeSet::Single(SchemaType::String)),
        ..Default::default()
      }),
    );
    entity_schema.properties.insert(
      "@odata.type".to_string(),
      ObjectOrReference::Object(ObjectSchema {
        schema_type: Some(SchemaTypeSet::Single(SchemaType::String)),
        ..Default::default()
      }),
    );
    entity_schema.required.push("id".to_string());

    let mut mapping = BTreeMap::new();
    mapping.insert(
      "#microsoft.graph.user".to_string(),
      "#/components/schemas/User".to_string(),
    );
    entity_schema.discriminator = Some(Discriminator {
      property_name: "@odata.type".to_string(),
      mapping: Some(mapping),
    });

    let mut user_inline = ObjectSchema {
      schema_type: Some(SchemaTypeSet::Single(SchemaType::Object)),
      ..Default::default()
    };
    user_inline.properties.insert(
      "@odata.type".to_string(),
      ObjectOrReference::Object(ObjectSchema {
        schema_type: Some(SchemaTypeSet::Single(SchemaType::String)),
        default: Some(json!("#microsoft.graph.user")),
        ..Default::default()
      }),
    );

    let mut user_schema = ObjectSchema {
      schema_type: Some(SchemaTypeSet::Single(SchemaType::Object)),
      ..Default::default()
    };
    user_schema.all_of.push(ObjectOrReference::Ref {
      ref_path: "#/components/schemas/Entity".to_string(),
      summary: None,
      description: None,
    });
    user_schema.all_of.push(ObjectOrReference::Object(user_inline));

    let mut schemas = BTreeMap::new();
    schemas.insert("Entity".to_string(), entity_schema);
    schemas.insert("User".to_string(), user_schema);

    let spec = create_test_spec(schemas);
    let mut graph = SchemaGraph::new(spec).unwrap();
    graph.build_dependencies();
    graph.detect_cycles();
    let converter = SchemaConverter::new(&graph);

    let result = converter
      .convert_schema("Entity", graph.get_schema("Entity").unwrap())
      .unwrap();

    assert!(
      result.iter().any(|ty| matches!(ty, RustType::DiscriminatedEnum(_))),
      "Entity should generate a discriminated enum"
    );
    assert!(
      result.iter().any(|ty| matches!(ty, RustType::Struct(_))),
      "Entity should also generate a backing struct"
    );

    let enum_def = result
      .iter()
      .find_map(|ty| match ty {
        RustType::DiscriminatedEnum(def) => Some(def),
        _ => None,
      })
      .expect("Discriminated enum should exist");
    assert_eq!(enum_def.name, "Entity");
    let fallback = enum_def
      .fallback
      .as_ref()
      .expect("Fallback variant should be generated");
    assert_eq!(fallback.type_name, "Box<EntityBase>");

    let struct_def = result
      .iter()
      .find_map(|ty| match ty {
        RustType::Struct(def) => Some(def),
        _ => None,
      })
      .expect("Backing struct should be present");
    assert_eq!(struct_def.name, "EntityBase");
    assert!(
      struct_def.serde_attrs.iter().any(|attr| attr == "deny_unknown_fields"),
      "Backing struct should inherit deny_unknown_fields"
    );
  }

  #[test]
  fn test_discriminated_child_inlines_parent_fields_and_boxes_cycles() {
    let mut entity_schema = ObjectSchema {
      schema_type: Some(SchemaTypeSet::Single(SchemaType::Object)),
      additional_properties: Some(Schema::Boolean(BooleanSchema(false))),
      ..Default::default()
    };

    entity_schema.properties.insert(
      "id".to_string(),
      ObjectOrReference::Object(ObjectSchema {
        schema_type: Some(SchemaTypeSet::Single(SchemaType::String)),
        ..Default::default()
      }),
    );
    entity_schema.properties.insert(
      "@odata.type".to_string(),
      ObjectOrReference::Object(ObjectSchema {
        schema_type: Some(SchemaTypeSet::Single(SchemaType::String)),
        ..Default::default()
      }),
    );
    entity_schema.properties.insert(
      "manager".to_string(),
      ObjectOrReference::Ref {
        ref_path: "#/components/schemas/Entity".to_string(),
        summary: None,
        description: None,
      },
    );
    entity_schema.required.push("id".to_string());

    let mut mapping = BTreeMap::new();
    mapping.insert(
      "#microsoft.graph.user".to_string(),
      "#/components/schemas/User".to_string(),
    );
    entity_schema.discriminator = Some(Discriminator {
      property_name: "@odata.type".to_string(),
      mapping: Some(mapping),
    });

    let mut user_inline = ObjectSchema {
      schema_type: Some(SchemaTypeSet::Single(SchemaType::Object)),
      ..Default::default()
    };
    user_inline.properties.insert(
      "@odata.type".to_string(),
      ObjectOrReference::Object(ObjectSchema {
        schema_type: Some(SchemaTypeSet::Single(SchemaType::String)),
        default: Some(json!("#microsoft.graph.user")),
        ..Default::default()
      }),
    );
    user_inline.properties.insert(
      "jobTitle".to_string(),
      ObjectOrReference::Object(ObjectSchema {
        schema_type: Some(SchemaTypeSet::Single(SchemaType::String)),
        ..Default::default()
      }),
    );

    let mut user_schema = ObjectSchema {
      schema_type: Some(SchemaTypeSet::Single(SchemaType::Object)),
      ..Default::default()
    };
    user_schema.all_of.push(ObjectOrReference::Ref {
      ref_path: "#/components/schemas/Entity".to_string(),
      summary: None,
      description: None,
    });
    user_schema.all_of.push(ObjectOrReference::Object(user_inline));
    user_schema.description = Some("MS Graph user entity".to_string());

    let mut schemas = BTreeMap::new();
    schemas.insert("Entity".to_string(), entity_schema);
    schemas.insert("User".to_string(), user_schema);

    let spec = create_test_spec(schemas);
    let mut graph = SchemaGraph::new(spec).unwrap();
    graph.build_dependencies();
    graph.detect_cycles();
    let converter = SchemaConverter::new(&graph);

    let result = converter
      .convert_schema("User", graph.get_schema("User").unwrap())
      .unwrap();

    let user_struct = result
      .iter()
      .find_map(|ty| match ty {
        RustType::Struct(def) if def.name == "User" => Some(def),
        _ => None,
      })
      .expect("User struct should be generated");

    assert!(
      user_struct
        .fields
        .iter()
        .all(|field| field.name != "__inherited_properties"),
      "Child struct should inline parent fields instead of flattening parent struct"
    );
    assert!(
      user_struct
        .fields
        .iter()
        .all(|field| field.serde_attrs.iter().all(|attr| attr != "flatten")),
      "No field should use serde(flatten) for inherited data"
    );
    assert!(
      user_struct.fields.iter().any(|field| field.name == "id"),
      "Parent fields should be present on the child struct"
    );
    assert!(
      user_struct.fields.iter().any(|field| field.name == "job_title"),
      "Child-specific fields should still be generated"
    );

    let manager_field = user_struct
      .fields
      .iter()
      .find(|field| field.name == "manager")
      .expect("manager field should be generated");
    assert_eq!(manager_field.rust_type.to_rust_type(), "Option<Box<Entity>>");
    assert!(manager_field.rust_type.boxed, "Cycles should be boxed");
    assert!(
      manager_field.rust_type.nullable,
      "Optional inherited field should remain optional"
    );

    let discriminator_field = user_struct
      .fields
      .iter()
      .find(|field| field.name == "odata_type")
      .expect("Discriminator field should be present to retain default value");
    assert_eq!(
      discriminator_field.default_value.as_ref(),
      Some(&json!("#microsoft.graph.user")),
      "Child discriminator should retain its default value"
    );
    assert!(
      discriminator_field
        .extra_attrs
        .iter()
        .any(|attr| attr == "#[doc(hidden)]"),
      "Discriminator field should be hidden from documentation"
    );
    assert!(
      discriminator_field.serde_attrs.iter().any(|attr| attr == "default"),
      "Discriminator field should request serde to use the default value when missing"
    );
    assert!(
      discriminator_field
        .serde_attrs
        .iter()
        .all(|attr| attr != "skip" && attr != "skip_serializing"),
      "Discriminator field should be serialized with its default value"
    );

    assert!(
      user_struct.serde_attrs.iter().any(|attr| attr == "deny_unknown_fields"),
      "deny_unknown_fields should carry over from parent additionalProperties=false"
    );
  }

  #[test]
  fn test_discriminator_field_removed_from_child_struct() {
    let mut cat_schema = ObjectSchema {
      schema_type: Some(SchemaTypeSet::Single(SchemaType::Object)),
      ..Default::default()
    };
    cat_schema.properties.insert(
      "type".to_string(),
      ObjectOrReference::Object(ObjectSchema {
        schema_type: Some(SchemaTypeSet::Single(SchemaType::String)),
        const_value: Some(json!("cat")),
        default: Some(json!("cat")),
        enum_values: vec![json!("cat")],
        ..Default::default()
      }),
    );
    cat_schema.properties.insert(
      "meows".to_string(),
      ObjectOrReference::Object(ObjectSchema {
        schema_type: Some(SchemaTypeSet::Single(SchemaType::Boolean)),
        ..Default::default()
      }),
    );
    cat_schema.required.push("type".to_string());
    cat_schema.required.push("meows".to_string());

    let mut dog_schema = ObjectSchema {
      schema_type: Some(SchemaTypeSet::Single(SchemaType::Object)),
      ..Default::default()
    };
    dog_schema.properties.insert(
      "type".to_string(),
      ObjectOrReference::Object(ObjectSchema {
        schema_type: Some(SchemaTypeSet::Single(SchemaType::String)),
        const_value: Some(json!("dog")),
        default: Some(json!("dog")),
        enum_values: vec![json!("dog")],
        ..Default::default()
      }),
    );
    dog_schema.properties.insert(
      "barks".to_string(),
      ObjectOrReference::Object(ObjectSchema {
        schema_type: Some(SchemaTypeSet::Single(SchemaType::Boolean)),
        ..Default::default()
      }),
    );
    dog_schema.required.push("type".to_string());
    dog_schema.required.push("barks".to_string());

    let mut mapping = BTreeMap::new();
    mapping.insert("cat".to_string(), "#/components/schemas/Cat".to_string());
    mapping.insert("dog".to_string(), "#/components/schemas/Dog".to_string());

    let mut pet_schema = ObjectSchema {
      schema_type: Some(SchemaTypeSet::Single(SchemaType::Object)),
      discriminator: Some(Discriminator {
        property_name: "type".to_string(),
        mapping: Some(mapping),
      }),
      ..Default::default()
    };
    pet_schema.one_of.push(ObjectOrReference::Ref {
      ref_path: "#/components/schemas/Cat".to_string(),
      summary: None,
      description: None,
    });
    pet_schema.one_of.push(ObjectOrReference::Ref {
      ref_path: "#/components/schemas/Dog".to_string(),
      summary: None,
      description: None,
    });

    let mut schemas = BTreeMap::new();
    schemas.insert("Pet".to_string(), pet_schema);
    schemas.insert("Cat".to_string(), cat_schema);
    schemas.insert("Dog".to_string(), dog_schema);

    let spec = create_test_spec(schemas);
    let graph = SchemaGraph::new(spec).unwrap();
    let converter = SchemaConverter::new(&graph);

    let cat_struct = converter
      .convert_schema("Cat", graph.get_schema("Cat").unwrap())
      .unwrap()
      .into_iter()
      .find_map(|ty| match ty {
        RustType::Struct(def) => Some(def),
        _ => None,
      })
      .expect("Cat struct should be generated");

    let discrim_field = cat_struct
      .fields
      .iter()
      .find(|field| field.name == "r#type")
      .expect("Discriminator property should be retained on child struct");
    assert_eq!(
      discrim_field.default_value.as_ref(),
      Some(&json!("cat")),
      "Child discriminator should preserve the schema default"
    );
    assert!(
      discrim_field.serde_attrs.iter().any(|attr| attr == "default"),
      "Discriminator should request serde to use its default when absent"
    );
    assert!(
      discrim_field
        .serde_attrs
        .iter()
        .all(|attr| attr != "skip" && attr != "skip_serializing"),
      "Discriminator should serialize with the default value"
    );
    assert!(
      cat_struct.fields.iter().any(|field| field.name == "meows"),
      "Non-discriminator fields must still be generated"
    );
  }

  #[test]
  fn test_skip_serializing_none_only_added_when_options_present() {
    let mut optional_schema = ObjectSchema {
      schema_type: Some(SchemaTypeSet::Single(SchemaType::Object)),
      ..Default::default()
    };
    optional_schema.properties.insert(
      "value".to_string(),
      ObjectOrReference::Object(ObjectSchema {
        schema_type: Some(SchemaTypeSet::Single(SchemaType::String)),
        ..Default::default()
      }),
    );

    let mut schemas = BTreeMap::new();
    schemas.insert("OptionalStruct".to_string(), optional_schema);

    let spec = create_test_spec(schemas);
    let graph = SchemaGraph::new(spec).unwrap();
    let converter = SchemaConverter::new(&graph);

    let (optional_type, _) = converter
      .convert_struct("OptionalStruct", graph.get_schema("OptionalStruct").unwrap())
      .unwrap();

    let optional_struct = match optional_type {
      RustType::Struct(def) => def,
      _ => panic!("Expected struct for OptionalStruct"),
    };

    assert!(
      optional_struct
        .outer_attrs
        .iter()
        .any(|attr| attr.contains("oas3_gen_support::skip_serializing_none")),
      "Optional fields should trigger oas3_gen_support::skip_serializing_none"
    );

    let mut required_schema = ObjectSchema {
      schema_type: Some(SchemaTypeSet::Single(SchemaType::Object)),
      ..Default::default()
    };
    required_schema.properties.insert(
      "value".to_string(),
      ObjectOrReference::Object(ObjectSchema {
        schema_type: Some(SchemaTypeSet::Single(SchemaType::String)),
        ..Default::default()
      }),
    );
    required_schema.required.push("value".to_string());

    let mut schemas = BTreeMap::new();
    schemas.insert("RequiredStruct".to_string(), required_schema);

    let spec = create_test_spec(schemas);
    let graph = SchemaGraph::new(spec).unwrap();
    let converter = SchemaConverter::new(&graph);

    let (required_type, _) = converter
      .convert_struct("RequiredStruct", graph.get_schema("RequiredStruct").unwrap())
      .unwrap();

    let required_struct = match required_type {
      RustType::Struct(def) => def,
      _ => panic!("Expected struct for RequiredStruct"),
    };

    assert!(
      required_struct.outer_attrs.is_empty(),
      "Required-only fields should not add oas3_gen_support::skip_serializing_none"
    );
  }

  #[test]
  fn test_const_values_become_field_defaults() {
    let mut schema = ObjectSchema {
      schema_type: Some(SchemaTypeSet::Single(SchemaType::Object)),
      additional_properties: Some(Schema::Boolean(BooleanSchema(false))),
      ..Default::default()
    };

    schema.properties.insert(
      "kind".to_string(),
      ObjectOrReference::Object(ObjectSchema {
        schema_type: Some(SchemaTypeSet::Single(SchemaType::String)),
        const_value: Some(json!("plain")),
        enum_values: vec![json!("plain")],
        ..Default::default()
      }),
    );
    schema.properties.insert(
      "value".to_string(),
      ObjectOrReference::Object(ObjectSchema {
        schema_type: Some(SchemaTypeSet::Single(SchemaType::String)),
        ..Default::default()
      }),
    );
    schema.required.push("kind".to_string());
    schema.required.push("value".to_string());

    let mut schemas = BTreeMap::new();
    schemas.insert("ConstStruct".to_string(), schema);

    let spec = create_test_spec(schemas);
    let graph = SchemaGraph::new(spec).unwrap();
    let converter = SchemaConverter::new(&graph);

    let (rust_type, _) = converter
      .convert_struct("ConstStruct", graph.get_schema("ConstStruct").unwrap())
      .unwrap();

    let struct_def = match rust_type {
      RustType::Struct(def) => def,
      _ => panic!("Expected struct"),
    };

    let kind_field = struct_def
      .fields
      .iter()
      .find(|f| f.name == "kind")
      .expect("kind field should exist");

    assert_eq!(
      kind_field.default_value.as_ref(),
      Some(&json!("plain")),
      "const string should produce a default value"
    );

    assert!(
      kind_field.serde_attrs.iter().any(|attr| attr == r#"rename = "kind""#) || kind_field.serde_attrs.is_empty(),
      "Sanity check attribute state left untouched"
    );
  }

  #[test]
  fn test_discriminator_mapping_sets_default_value() {
    let mut parent_schema = ObjectSchema {
      schema_type: Some(SchemaTypeSet::Single(SchemaType::Object)),
      ..Default::default()
    };
    parent_schema.properties.insert(
      "kind".to_string(),
      ObjectOrReference::Object(ObjectSchema {
        schema_type: Some(SchemaTypeSet::Single(SchemaType::String)),
        ..Default::default()
      }),
    );
    parent_schema.required.push("kind".to_string());

    let mut mapping = BTreeMap::new();
    mapping.insert("base#kind".to_string(), "#/components/schemas/Base".to_string());
    parent_schema.discriminator = Some(Discriminator {
      property_name: "kind".to_string(),
      mapping: Some(mapping),
    });

    let mut base_inline = ObjectSchema {
      schema_type: Some(SchemaTypeSet::Single(SchemaType::Object)),
      ..Default::default()
    };
    base_inline.properties.insert(
      "kind".to_string(),
      ObjectOrReference::Object(ObjectSchema {
        schema_type: Some(SchemaTypeSet::Single(SchemaType::String)),
        ..Default::default()
      }),
    );

    let mut base_schema = ObjectSchema {
      schema_type: Some(SchemaTypeSet::Single(SchemaType::Object)),
      ..Default::default()
    };
    base_schema.all_of.push(ObjectOrReference::Ref {
      ref_path: "#/components/schemas/Parent".to_string(),
      summary: None,
      description: None,
    });
    base_schema.all_of.push(ObjectOrReference::Object(base_inline));

    let mut schemas = BTreeMap::new();
    schemas.insert("Parent".to_string(), parent_schema);
    schemas.insert("Base".to_string(), base_schema);

    let spec = create_test_spec(schemas);
    let mut graph = SchemaGraph::new(spec).unwrap();
    graph.build_dependencies();
    graph.detect_cycles();
    let converter = SchemaConverter::new(&graph);

    let result = converter
      .convert_schema("Base", graph.get_schema("Base").unwrap())
      .unwrap();

    let struct_def = result
      .iter()
      .find_map(|ty| match ty {
        RustType::Struct(def) if def.name == "Base" => Some(def),
        _ => None,
      })
      .expect("Base struct should exist");

    let kind_field = struct_def
      .fields
      .iter()
      .find(|field| field.name == "kind")
      .expect("kind field should exist");

    assert_eq!(
      kind_field.default_value.as_ref(),
      Some(&json!("base#kind")),
      "Discriminator should use mapping value when schema default is absent"
    );
    assert!(
      kind_field.serde_attrs.iter().any(|attr| attr == "default"),
      "Field should request serde to inject the default when missing"
    );
    assert!(
      kind_field
        .serde_attrs
        .iter()
        .all(|attr| attr != "skip" && attr != "skip_serializing"),
      "Field should serialize with the default discriminator value"
    );
  }

  #[test]
  fn test_empty_schema_produces_type_alias() {
    let mut schemas = BTreeMap::new();
    schemas.insert("Empty".to_string(), ObjectSchema::default());

    let spec = create_test_spec(schemas);
    let graph = SchemaGraph::new(spec).unwrap();
    let converter = SchemaConverter::new(&graph);

    let generated = converter
      .convert_schema("Empty", graph.get_schema("Empty").unwrap())
      .unwrap();

    assert_eq!(generated.len(), 1);
    match &generated[0] {
      RustType::TypeAlias(alias) => {
        assert_eq!(alias.name, "Empty");
        assert_eq!(alias.target.to_rust_type(), "serde_json::Value");
      }
      other => panic!("Expected type alias for empty schema, got {:?}", other),
    }
  }
}

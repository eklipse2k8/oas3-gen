use std::{
  cmp::Reverse,
  collections::{BTreeMap, BTreeSet, HashMap, HashSet},
  future::Future,
  pin::Pin,
  sync::LazyLock,
};

use num_format::{CustomFormat, Grouping, ToFormattedString as _};
use oas3::spec::{ObjectOrReference, ObjectSchema, Schema, SchemaType, SchemaTypeSet};
use regex::Regex;
use serde_json::Number;

use super::{
  ast::{EnumDef, FieldDef, RustPrimitive, RustType, StructDef, TypeAliasDef, TypeRef, VariantContent, VariantDef},
  schema_graph::SchemaGraph,
  utils::doc_comment_lines,
};
use crate::{
  generator::ast::{DiscriminatedEnumDef, DiscriminatedVariant, StructKind},
  reserved::{to_rust_field_name, to_rust_type_name},
};

/// Field metadata extracted from an `OpenAPI` schema property
#[derive(Clone)]
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
  InlineUnions,
}

#[derive(Copy, Clone, PartialEq, Eq)]
enum UnionKind {
  OneOf,
  AnyOf,
}

static UNDERSCORE_FORMAT: LazyLock<CustomFormat> = LazyLock::new(|| {
  CustomFormat::builder()
    .grouping(Grouping::Standard)
    .separator("_")
    .build()
    .expect("formatter failed to build.")
});

/// Type alias for boxed async field conversion result
type FieldConversionFuture<'a> =
  Pin<Box<dyn Future<Output = anyhow::Result<(Vec<FieldDef>, Vec<RustType>)>> + Send + 'a>>;

/// Converter that transforms `OpenAPI` schemas into Rust AST structures
pub(crate) struct SchemaConverter<'a> {
  graph: &'a SchemaGraph,
}

impl<'a> SchemaConverter<'a> {
  pub(crate) fn new(graph: &'a SchemaGraph) -> Self {
    Self { graph }
  }

  /// Converts an optional schema description into formatted documentation lines.
  ///
  /// # Parameters
  /// - `desc`: Optional description text pulled from the schema.
  ///
  async fn docs(desc: Option<&String>) -> Vec<String> {
    if let Some(d) = desc {
      doc_comment_lines(d).await
    } else {
      vec![]
    }
  }

  /// Builds derive attribute list for structs while respecting read/write-only flags.
  ///
  /// # Parameters
  /// - `all_read_only`: Indicates if every field is marked read-only.
  /// - `all_write_only`: Indicates if every field is marked write-only.
  ///
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

  /// Builds derive attribute list for enums.
  ///
  /// # Parameters
  /// None.
  ///
  #[inline]
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

  /// Determines whether a schema is a discriminated base type (has discriminator mappings and properties).
  ///
  /// # Parameters
  /// - `schema`: The `ObjectSchema` under inspection.
  ///
  fn is_discriminated_base_type(schema: &ObjectSchema) -> bool {
    schema
      .discriminator
      .as_ref()
      .and_then(|d| d.mapping.as_ref().map(|m| !m.is_empty()))
      .unwrap_or(false)
      && !schema.properties.is_empty()
  }

  /// Computes the inheritance depth of a schema (0 when no `allOf` chain exists).
  ///
  /// # Parameters
  /// - `schema_name`: The registry key for the schema being measured.
  /// - `memo`: Cache used to avoid repeated traversal when computing depths.
  ///
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
          ObjectOrReference::Object(_) => None,
        })
        .map(|parent_name| self.compute_inheritance_depth(&parent_name, memo))
        .max()
        .unwrap_or(0)
        + 1
    };

    memo.insert(schema_name.to_string(), depth);
    depth
  }

  /// Extracts child schemas from a discriminator mapping sorted by inheritance depth.
  ///
  /// # Parameters
  /// - `schema`: The discriminated base `ObjectSchema` supplying the mapping.
  ///
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

  /// Determines container-level attributes for a struct based on its fields.
  ///
  /// # Parameters
  /// - `fields`: Slice of generated `FieldDef` values for the struct.
  ///
  pub(crate) fn container_outer_attrs(fields: &[FieldDef]) -> Vec<String> {
    if fields.iter().any(|field| field.rust_type.nullable) {
      vec!["oas3_gen_support::skip_serializing_none".into()]
    } else {
      Vec::new()
    }
  }

  /// Finds the discriminator mapping entry that points at the provided schema.
  ///
  /// # Parameters
  /// - `schema_name`: The child schema name being searched for in mappings.
  ///
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

  /// Merges a child schema with its parent's discriminator context and collected allOf properties.
  ///
  /// # Parameters
  /// - `child_schema`: The concrete child `ObjectSchema` being materialized.
  /// - `parent_schema`: The discriminated parent `ObjectSchema` supplying inherited properties.
  ///
  fn merge_child_schema_with_parent(
    &self,
    child_schema: &ObjectSchema,
    parent_schema: &ObjectSchema,
  ) -> anyhow::Result<ObjectSchema> {
    let mut merged_properties = BTreeMap::new();
    let mut merged_required = Vec::new();
    let mut merged_discriminator = parent_schema.discriminator.clone();

    self.collect_all_of_properties(
      child_schema,
      &mut merged_properties,
      &mut merged_required,
      &mut merged_discriminator,
    )?;

    let mut merged_schema = child_schema.clone();
    merged_schema.properties = merged_properties;
    merged_schema.required = merged_required;
    merged_schema.discriminator = merged_discriminator;
    merged_schema.all_of.clear();

    if merged_schema.additional_properties.is_none() {
      merged_schema
        .additional_properties
        .clone_from(&parent_schema.additional_properties);
    }

    Ok(merged_schema)
  }

  /// Prepares serde attributes and a synthesized `FieldDef` for `additionalProperties` handling.
  ///
  /// # Parameters
  /// - `schema`: The schema whose `additional_properties` should be inspected.
  ///
  fn prepare_additional_properties(&self, schema: &ObjectSchema) -> anyhow::Result<(Vec<String>, Option<FieldDef>)> {
    let mut serde_attrs = Vec::new();
    let mut additional_field = None;

    if let Some(ref additional) = schema.additional_properties {
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
            additional_field = Some(FieldDef {
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

    Ok((serde_attrs, additional_field))
  }

  /// Converts a child schema that extends a discriminated parent into Rust types.
  ///
  /// # Parameters
  /// - `name`: The OpenAPI schema key for the child.
  /// - `schema`: The child `ObjectSchema` instance.
  /// - `parent_schema`: The resolved discriminated parent `ObjectSchema`.
  ///
  async fn convert_discriminated_child(
    &self,
    name: &str,
    schema: &ObjectSchema,
    parent_schema: &ObjectSchema,
  ) -> anyhow::Result<Vec<RustType>> {
    if parent_schema.discriminator.is_none() {
      return Err(anyhow::anyhow!("Parent schema is not discriminated"));
    }

    let struct_name = to_rust_type_name(name);
    let merged_schema = self.merge_child_schema_with_parent(schema, parent_schema)?;

    let (mut fields, mut inline_types) = self
      .convert_fields_core(
        &struct_name,
        &merged_schema,
        InlinePolicy::InlineUnions,
        None,
        Some(name),
      )
      .await?;

    let (serde_attrs, additional_field) = self.prepare_additional_properties(&merged_schema)?;
    if let Some(field) = additional_field {
      fields.push(field);
    }

    let all_read_only = !fields.is_empty() && fields.iter().all(|f| f.read_only);
    let all_write_only = !fields.is_empty() && fields.iter().all(|f| f.write_only);
    let outer_attrs = Self::container_outer_attrs(&fields);

    let mut all_types = vec![RustType::Struct(StructDef {
      name: struct_name,
      docs: Self::docs(schema.description.as_ref()).await,
      fields,
      derives: Self::derives_for_struct(all_read_only, all_write_only),
      serde_attrs,
      outer_attrs,
      methods: vec![],
      kind: StructKind::Schema,
    })];

    all_types.append(&mut inline_types);
    Ok(all_types)
  }

  /// Creates a discriminated enum for schemas with discriminator mappings.
  ///
  /// # Parameters
  /// - `base_name`: The schema name used to derive Rust identifiers.
  /// - `schema`: The discriminated base `ObjectSchema` definition.
  /// - `base_struct_name`: The generated struct name backing the base variant.
  ///
  async fn create_discriminated_enum(
    &self,
    base_name: &str,
    schema: &ObjectSchema,
    base_struct_name: &str,
  ) -> anyhow::Result<RustType> {
    let Some(discriminator_field) = schema.discriminator.as_ref().map(|d| &d.property_name) else {
      anyhow::bail!("no discriminator field")
    };

    let children = self.extract_discriminator_children(schema);
    let enum_name = to_rust_type_name(base_name);

    let mut variants = Vec::new();

    for (disc_value, child_schema_name) in children {
      let child_type_name = to_rust_type_name(&child_schema_name);

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
        type_name: format!("Box<{child_type_name}>"),
      });
    }

    let base_variant_name = to_rust_type_name(base_name.split('.').next_back().unwrap_or(base_name));
    let fallback = Some(DiscriminatedVariant {
      discriminator_value: String::new(),
      variant_name: base_variant_name,
      type_name: format!("Box<{base_struct_name}>"),
    });

    Ok(RustType::DiscriminatedEnum(DiscriminatedEnumDef {
      name: enum_name,
      docs: Self::docs(schema.description.as_ref()).await,
      discriminator_field: discriminator_field.clone(),
      variants,
      fallback,
    }))
  }

  /// Combines the primary struct type with any inline types and optional discriminated enum.
  ///
  /// # Parameters
  /// - `name`: The schema name currently being materialized.
  /// - `schema`: The source `ObjectSchema` for discriminator inspection.
  /// - `main_type`: The main `RustType` produced for the schema.
  /// - `inline_types`: Any inline helper types generated alongside the main type.
  ///
  async fn finalize_struct_types(
    &self,
    name: &str,
    schema: &ObjectSchema,
    main_type: RustType,
    mut inline_types: Vec<RustType>,
  ) -> anyhow::Result<Vec<RustType>> {
    let mut all_types = Vec::new();

    if Self::is_discriminated_base_type(schema) {
      let base_struct_name = match &main_type {
        RustType::Struct(def) => def.name.clone(),
        _ => format!("{}Base", to_rust_type_name(name)),
      };
      let discriminated_enum = self.create_discriminated_enum(name, schema, &base_struct_name).await?;
      all_types.push(discriminated_enum);
    }

    all_types.push(main_type);
    all_types.append(&mut inline_types);
    Ok(all_types)
  }

  /// Converts an object schema into the full set of Rust type definitions.
  ///
  /// # Parameters
  /// - `name`: The OpenAPI schema key being converted.
  /// - `schema`: The resolved `ObjectSchema` definition.
  ///
  pub(crate) async fn convert_schema(&self, name: &str, schema: &ObjectSchema) -> anyhow::Result<Vec<RustType>> {
    if !schema.all_of.is_empty() {
      return self.convert_all_of_schema(name, schema).await;
    }

    if !schema.one_of.is_empty() {
      return self.convert_union_enum(name, schema, UnionKind::OneOf).await;
    }

    if !schema.any_of.is_empty() {
      return self.convert_union_enum(name, schema, UnionKind::AnyOf).await;
    }

    if !schema.enum_values.is_empty() {
      return Ok(vec![self.convert_simple_enum(name, schema, &schema.enum_values).await?]);
    }

    if !schema.properties.is_empty() {
      let (main_type, inline_types) = self.convert_struct(name, schema, None).await?;
      return self.finalize_struct_types(name, schema, main_type, inline_types).await;
    }

    let alias = RustType::TypeAlias(TypeAliasDef {
      name: to_rust_type_name(name),
      docs: Self::docs(schema.description.as_ref()).await,
      target: TypeRef::new("serde_json::Value"),
    });

    Ok(vec![alias])
  }

  /// Recursively collects properties, required fields, and discriminators from an `allOf` chain.
  ///
  /// # Parameters
  /// - `schema`: The schema whose inheritance chain is being traversed.
  /// - `properties`: Destination map receiving merged properties.
  /// - `required`: Destination vector accumulating required field names.
  /// - `discriminator`: Destination for the discovered discriminator, if any.
  ///
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
      discriminator.clone_from(&schema.discriminator);
    }

    Ok(())
  }

  /// Detects and returns a discriminated parent schema referenced via allOf, if present.
  ///
  /// # Parameters
  /// - `schema`: The derived `ObjectSchema` potentially extending a discriminated parent.
  ///
  fn detect_discriminated_parent(&self, schema: &ObjectSchema) -> Option<(String, ObjectSchema)> {
    schema.all_of.iter().find_map(|all_of_ref| {
      let ObjectOrReference::Ref { ref_path, .. } = all_of_ref else {
        return None;
      };

      let parent_name = SchemaGraph::extract_ref_name(ref_path)?;
      let parent_ref = self.graph.spec().components.as_ref()?.schemas.get(&parent_name)?;
      let parent_schema = parent_ref.resolve(self.graph.spec()).ok()?;

      let merged_parent = if parent_schema.all_of.is_empty() {
        parent_schema.clone()
      } else {
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
      };

      if Self::is_discriminated_base_type(&merged_parent) {
        Some((parent_name, merged_parent))
      } else {
        None
      }
    })
  }

  /// Merges allOf properties, required lists, and discriminator information into a single schema.
  ///
  /// # Parameters
  /// - `schema`: The composite `ObjectSchema` containing allOf references.
  ///
  fn merge_all_of_schema(&self, schema: &ObjectSchema) -> anyhow::Result<ObjectSchema> {
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
    merged_schema.discriminator.clone_from(&merged_discriminator);

    Ok(merged_schema)
  }

  /// Converts an `allOf` composition into flattened Rust type definitions.
  ///
  /// # Parameters
  /// - `name`: The schema key representing the composite definition.
  /// - `schema`: The composite `ObjectSchema` with `allOf` references.
  ///
  async fn convert_all_of_schema(&self, name: &str, schema: &ObjectSchema) -> anyhow::Result<Vec<RustType>> {
    if let Some((_, parent_schema)) = self.detect_discriminated_parent(schema) {
      return self.convert_discriminated_child(name, schema, &parent_schema).await;
    }

    let merged_schema = self.merge_all_of_schema(schema)?;
    let (main_type, inline_types) = self.convert_struct(name, &merged_schema, None).await?;

    self
      .finalize_struct_types(name, &merged_schema, main_type, inline_types)
      .await
  }

  /// Converts `oneOf`/`anyOf` schemas into Rust enums, handling discriminator nuances.
  ///
  /// # Parameters
  /// - `name`: The schema name used to derive Rust identifiers.
  /// - `schema`: The union `ObjectSchema` definition.
  /// - `kind`: Indicates whether the union originated from `oneOf` or `anyOf`.
  ///
  #[allow(clippy::too_many_lines)]
  async fn convert_union_enum(
    &self,
    name: &str,
    schema: &ObjectSchema,
    kind: UnionKind,
  ) -> anyhow::Result<Vec<RustType>> {
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
        return self
          .convert_string_enum_with_catchall(name, schema, &known_values)
          .await;
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

        let docs = Self::docs(resolved.description.as_ref()).await;
        let deprecated = resolved.deprecated.unwrap_or(false);

        let mut variant_name = rust_type_name.clone();
        if !seen_names.insert(variant_name.clone()) {
          variant_name = format!("{variant_name}{i}");
          seen_names.insert(variant_name.clone());
        }

        let mut serde_attrs = Vec::new();
        if discriminator_prop.is_some()
          && let Some(disc_value) = discriminator_map.get(schema_name)
        {
          serde_attrs.push(format!("rename = \"{disc_value}\""));
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

      let mut variant_name = if let Some(ref title) = resolved.title {
        to_rust_type_name(title)
      } else {
        Self::infer_variant_name(&resolved, i)
      };

      if !seen_names.insert(variant_name.clone()) {
        variant_name = format!("{variant_name}{i}");
        seen_names.insert(variant_name.clone());
      }

      let docs = Self::docs(resolved.description.as_ref()).await;
      let deprecated = resolved.deprecated.unwrap_or(false);

      let (content, mut generated_types) = if let Some(disc_prop) = discriminator_prop {
        if resolved.properties.is_empty() {
          let field = FieldDef {
            name: "value".to_string(),
            rust_type: self.schema_to_type_ref(&resolved)?,
            ..Default::default()
          };
          (VariantContent::Struct(vec![field]), vec![])
        } else {
          match self
            .convert_fields_core(name, &resolved, InlinePolicy::InlineUnions, Some(disc_prop), None)
            .await
          {
            Ok((fields, tys)) => (VariantContent::Struct(fields), tys),
            Err(_e) => (VariantContent::Struct(vec![]), vec![]),
          }
        }
      } else if !resolved.properties.is_empty() {
        let fields = self.convert_fields(&resolved).await?;
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

    let original_names: Vec<String> = variants.iter().map(|v| v.name.clone()).collect();
    let stripped_names = Self::strip_common_affixes(&original_names);
    for (variant, stripped_name) in variants.iter_mut().zip(stripped_names.iter()) {
      variant.name.clone_from(stripped_name);
    }

    if kind == UnionKind::AnyOf {
      let enum_name = to_rust_type_name(name);
      for variant in &mut variants {
        if let VariantContent::Struct(ref mut fields) = variant.content {
          for field in fields {
            if field.rust_type.base_type.to_string() == enum_name && !field.rust_type.boxed {
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
      docs: Self::docs(schema.description.as_ref()).await,
      variants,
      discriminator: schema.discriminator.as_ref().map(|d| d.property_name.clone()),
      derives,
      serde_attrs,
      outer_attrs: vec![],
    });

    let mut out = inline_types;
    out.push(main_enum);
    Ok(out)
  }

  /// Generates a two-level enum for string unions with known constants plus an "other" arm.
  ///
  /// # Parameters
  /// - `name`: The schema name used to derive Rust identifiers.
  /// - `schema`: The original `ObjectSchema` containing the union.
  /// - `const_values`: Ordered list of known constant values with descriptions and deprecation flags.
  ///
  async fn convert_string_enum_with_catchall(
    &self,
    name: &str,
    schema: &ObjectSchema,
    const_values: &[(serde_json::Value, Option<String>, bool)],
  ) -> anyhow::Result<Vec<RustType>> {
    let base_name = to_rust_type_name(name);
    let known_name = format!("{base_name}Known");

    let mut known_variants = Vec::new();
    let mut seen_names = BTreeSet::new();

    for (i, (value, description, deprecated)) in const_values.iter().enumerate() {
      if let Some(str_val) = value.as_str() {
        let mut variant_name = to_rust_type_name(str_val);
        if !seen_names.insert(variant_name.clone()) {
          variant_name = format!("{variant_name}{i}");
          seen_names.insert(variant_name.clone());
        }
        let docs = Self::docs(description.as_ref()).await;
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
      docs: Self::docs(schema.description.as_ref()).await,
      variants: outer_variants,
      discriminator: None,
      derives: Self::derives_for_enum(),
      serde_attrs: vec!["untagged".into()],
      outer_attrs: vec![],
    });

    Ok(vec![inner_enum, outer_enum])
  }

  /// Infers a default variant name from a schema's structure when no title is present.
  ///
  /// # Parameters
  /// - `schema`: The `ObjectSchema` describing the variant.
  /// - `index`: Fallback index used when no specific hint is available.
  ///
  fn infer_variant_name(schema: &ObjectSchema, index: usize) -> String {
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
      format!("Variant{index}")
    }
  }

  /// Splits a `PascalCase` identifier into constituent words.
  ///
  /// # Parameters
  /// - `name`: The identifier to split.
  ///
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

  /// Removes common prefixes and suffixes from a set of variant names to shorten them.
  ///
  /// # Parameters
  /// - `variant_names`: The original variant names to process.
  ///
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

  /// Converts a simple string enum into a Rust enum definition.
  ///
  /// # Parameters
  /// - `name`: The schema identifier for the enum.
  /// - `schema`: The source `ObjectSchema` for description metadata.
  /// - `enum_values`: The list of literal enum values.
  ///
  async fn convert_simple_enum(
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
          variant_name = format!("{variant_name}{i}");
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
      docs: Self::docs(schema.description.as_ref()).await,
      variants,
      discriminator: None,
      derives: Self::derives_for_enum(),
      serde_attrs: vec![],
      outer_attrs: vec![],
    }))
  }

  /// Converts an object schema definition into a Rust struct plus inline helper types.
  ///
  /// # Parameters
  /// - `name`: The schema key used to derive the struct name.
  /// - `schema`: The resolved `ObjectSchema` for the struct.
  /// - `kind`: Optional override for the struct's `StructKind` classification.
  ///
  pub(crate) async fn convert_struct(
    &self,
    name: &str,
    schema: &ObjectSchema,
    kind: Option<StructKind>,
  ) -> anyhow::Result<(RustType, Vec<RustType>)> {
    let is_discriminated = Self::is_discriminated_base_type(schema);

    let struct_name_base = to_rust_type_name(name);
    let struct_name = if is_discriminated {
      format!("{struct_name_base}Base")
    } else {
      struct_name_base.clone()
    };

    let (mut fields, inline_types) = self
      .convert_fields_core(&struct_name, schema, InlinePolicy::InlineUnions, None, Some(name))
      .await?;

    let (mut serde_attrs, additional_field) = self.prepare_additional_properties(schema)?;
    if let Some(field) = additional_field {
      fields.push(field);
    }

    let all_fields_defaultable = fields.iter().all(|f| {
      f.default_value.is_some()
        || f.rust_type.nullable
        || f.rust_type.is_array
        || matches!(
          &f.rust_type.base_type,
          RustPrimitive::String
            | RustPrimitive::Bool
            | RustPrimitive::I8
            | RustPrimitive::I16
            | RustPrimitive::I32
            | RustPrimitive::I64
            | RustPrimitive::I128
            | RustPrimitive::Isize
            | RustPrimitive::U8
            | RustPrimitive::U16
            | RustPrimitive::U32
            | RustPrimitive::U64
            | RustPrimitive::U128
            | RustPrimitive::Usize
            | RustPrimitive::F32
            | RustPrimitive::F64
            | RustPrimitive::Value
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
      docs: Self::docs(schema.description.as_ref()).await,
      fields,
      derives,
      serde_attrs,
      outer_attrs,
      methods: vec![],
      kind: kind.unwrap_or(StructKind::Schema),
    });

    Ok((struct_type, inline_types))
  }

  /// Extracts a regex validation pattern from a string schema if the pattern is valid.
  ///
  /// # Parameters
  /// - `prop_name`: Name of the property being processed (used for diagnostics).
  /// - `schema`: The `ObjectSchema` supplying potential pattern metadata.
  ///
  pub(crate) fn extract_validation_pattern<'s>(prop_name: &str, schema: &'s ObjectSchema) -> Option<&'s String> {
    match (schema.schema_type.as_ref(), schema.pattern.as_ref()) {
      (Some(SchemaTypeSet::Single(SchemaType::String)), Some(pattern)) => {
        let is_non_string_format = schema.format.as_ref().is_some_and(|f| {
          matches!(
            f.as_str(),
            "date" | "date-time" | "duration" | "time" | "binary" | "byte" | "uuid"
          )
        });

        if is_non_string_format {
          return None;
        }

        if Regex::new(pattern).is_ok() {
          Some(pattern)
        } else {
          eprintln!("Warning: Invalid regex pattern '{pattern}' for property '{prop_name}'");
          None
        }
      }
      _ => None,
    }
  }

  /// Renders a `serde_json::Number` into Rust literal form with optional float formatting.
  ///
  /// # Parameters
  /// - `is_float`: When true, forces decimal formatting suitable for floats.
  /// - `num`: The number to render.
  ///
  fn render_number(primitive: &RustPrimitive, num: &Number) -> String {
    if primitive.is_float() {
      let s = num.to_string();
      if s.contains('.') { s } else { format!("{s}.0") }
    } else {
      match primitive {
        RustPrimitive::I8 => {
          if let Some(value) = num.as_i64() {
            if value <= i8::MIN as i64 {
              return "i8::MIN".to_string();
            } else if value >= i8::MAX as i64 {
              return "i8::MAX".to_string();
            }
            format!("{}i8", value.to_formatted_string(&*UNDERSCORE_FORMAT))
          } else {
            num.to_string()
          }
        }
        RustPrimitive::I16 => {
          if let Some(value) = num.as_i64() {
            if value <= i16::MIN as i64 {
              return "i16::MIN".to_string();
            } else if value >= i16::MAX as i64 {
              return "i16::MAX".to_string();
            }
            format!("{}i16", value.to_formatted_string(&*UNDERSCORE_FORMAT))
          } else {
            num.to_string()
          }
        }
        RustPrimitive::I32 => {
          if let Some(value) = num.as_i64() {
            if value <= i32::MIN as i64 {
              return "i32::MIN".to_string();
            } else if value >= i32::MAX as i64 {
              return "i32::MAX".to_string();
            }
            format!("{}i32", value.to_formatted_string(&*UNDERSCORE_FORMAT))
          } else {
            num.to_string()
          }
        }
        RustPrimitive::I64 => {
          if let Some(value) = num.as_i64() {
            format!("{}i64", value.to_formatted_string(&*UNDERSCORE_FORMAT))
          } else {
            num.to_string()
          }
        }
        RustPrimitive::U8 => {
          if let Some(value) = num.as_u64() {
            if value >= u8::MAX as u64 {
              return "u8::MAX".to_string();
            }
            format!("{}u8", value.to_formatted_string(&*UNDERSCORE_FORMAT))
          } else {
            num.to_string()
          }
        }
        RustPrimitive::U16 => {
          if let Some(value) = num.as_u64() {
            if value >= u16::MAX as u64 {
              return "u16::MAX".to_string();
            }
            format!("{}u16", value.to_formatted_string(&*UNDERSCORE_FORMAT))
          } else {
            num.to_string()
          }
        }
        RustPrimitive::U32 => {
          if let Some(value) = num.as_u64() {
            if value >= u32::MAX as u64 {
              return "u32::MAX".to_string();
            }
            format!("{}u32", value.to_formatted_string(&*UNDERSCORE_FORMAT))
          } else {
            num.to_string()
          }
        }
        RustPrimitive::U64 => {
          if let Some(value) = num.as_u64() {
            format!("{}u64", value.to_formatted_string(&*UNDERSCORE_FORMAT))
          } else {
            num.to_string()
          }
        }
        _ => num.to_string(),
      }
    }
  }

  /// Extracts validation attributes from an `OpenAPI` schema for use with `validator` macros.
  ///
  /// # Parameters
  /// - `_prop_name`: The property name (unused, retained for compatibility).
  /// - `is_required`: Indicates whether the property is required.
  /// - `schema`: The `ObjectSchema` describing the property.
  ///
  pub(crate) fn extract_validation_attrs(_prop_name: &str, is_required: bool, schema: &ObjectSchema) -> Vec<String> {
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
        SchemaTypeSet::Single(SchemaType::Number | SchemaType::Integer)
      ) {
        let format = if matches!(schema_type, SchemaTypeSet::Single(SchemaType::Number)) {
          Self::format_to_primitive(schema.format.as_ref()).unwrap_or(RustPrimitive::F64)
        } else {
          Self::format_to_primitive(schema.format.as_ref()).unwrap_or(RustPrimitive::I64)
        };

        let mut parts = Vec::<String>::new();

        if let Some(exclusive_min) = schema
          .exclusive_minimum
          .as_ref()
          .map(|v| format!("exclusive_min = {}", Self::render_number(&format, v)))
        {
          parts.push(exclusive_min);
        }
        if let Some(exclusive_max) = schema
          .exclusive_maximum
          .as_ref()
          .map(|v| format!("exclusive_max = {}", Self::render_number(&format, v)))
        {
          parts.push(exclusive_max);
        }
        if let Some(min) = schema
          .minimum
          .as_ref()
          .map(|v| format!("min = {}", Self::render_number(&format, v)))
        {
          parts.push(min);
        }
        if let Some(max) = schema
          .maximum
          .as_ref()
          .map(|v| format!("max = {}", Self::render_number(&format, v)))
        {
          parts.push(max);
        }

        if !parts.is_empty() {
          attrs.push(format!("range({})", parts.join(", ")));
        }
      }

      if matches!(schema_type, SchemaTypeSet::Single(SchemaType::String)) && schema.enum_values.is_empty() {
        let is_non_string_format = schema.format.as_ref().is_some_and(|f| {
          matches!(
            f.as_str(),
            "date" | "date-time" | "duration" | "time" | "binary" | "byte" | "uuid"
          )
        });

        if !is_non_string_format {
          let min_length = schema.min_length.map(|l| l.to_formatted_string(&*UNDERSCORE_FORMAT));
          let max_length = schema.max_length.map(|l| l.to_formatted_string(&*UNDERSCORE_FORMAT));

          if let (Some(min), Some(max)) = (&min_length, &max_length) {
            attrs.push(format!("length(min = {min}, max = {max})"));
          } else if let Some(min) = &min_length {
            attrs.push(format!("length(min = {min})"));
          } else if let Some(max) = &max_length {
            attrs.push(format!("length(max = {max})"));
          } else if is_required {
            attrs.push("length(min = 1)".to_string());
          }
        }
      }

      if matches!(schema_type, SchemaTypeSet::Single(SchemaType::Array)) {
        let min_length = schema.min_items.map(|l| l.to_formatted_string(&*UNDERSCORE_FORMAT));
        let max_length = schema.max_items.map(|l| l.to_formatted_string(&*UNDERSCORE_FORMAT));

        if let (Some(min), Some(max)) = (&min_length, &max_length) {
          attrs.push(format!("length(min = {min}, max = {max})"));
        } else if let Some(min) = &min_length {
          attrs.push(format!("length(min = {min})"));
        } else if let Some(max) = &max_length {
          attrs.push(format!("length(max = {max})"));
        }
      }
    }

    attrs
  }

  /// Extracts a default value from a schema using `default`, `const`, or single-value enums.
  ///
  /// # Parameters
  /// - `schema`: The `ObjectSchema` from which to extract a default.
  ///
  pub(crate) fn extract_default_value(schema: &ObjectSchema) -> Option<serde_json::Value> {
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

  /// Attempts to coerce a two-variant union containing a nullable element into an optional type.
  ///
  /// # Parameters
  /// - `variants`: The list of `ObjectOrReference` union variants.
  /// - `prop_schema`: The resolved property schema containing the union.
  ///
  fn try_build_nullable_union(
    &self,
    variants: &[ObjectOrReference<ObjectSchema>],
    prop_schema: &ObjectSchema,
  ) -> anyhow::Result<Option<TypeRef>> {
    let has_nullable_or_generic = variants.iter().any(|variant| {
      variant
        .resolve(self.graph.spec())
        .ok()
        .is_some_and(|schema| Self::is_nullable_or_generic(&schema))
    });

    if !(has_nullable_or_generic && variants.len() == 2) {
      return Ok(None);
    }

    for variant_ref in variants {
      if let Some(ref_name) = Self::try_extract_ref_name(variant_ref) {
        let mut type_ref = TypeRef::new(to_rust_type_name(&ref_name));
        if self.graph.is_cyclic(&ref_name) {
          type_ref = type_ref.with_boxed();
        }
        return Ok(Some(type_ref.with_option()));
      }

      if let Ok(resolved) = variant_ref.resolve(self.graph.spec())
        && !Self::is_nullable_or_generic(&resolved)
      {
        return Ok(Some(self.schema_to_type_ref(&resolved)?.with_option()));
      }
    }

    Ok(Some(self.schema_to_type_ref(prop_schema)?.with_option()))
  }

  /// Converts inline union schemas into either references, generated enums, or optional types.
  ///
  /// # Parameters
  /// - `parent_name`: The containing schema name, used to derive inline enum names.
  /// - `prop_name`: The property key currently being processed.
  /// - `prop_schema`: The resolved `ObjectSchema` for the property.
  /// - `uses_one_of`: Indicates whether the union originated from a `oneOf` (otherwise `anyOf`).
  ///
  async fn convert_inline_union_type(
    &self,
    parent_name: &str,
    prop_name: &str,
    prop_schema: &ObjectSchema,
    uses_one_of: bool,
  ) -> anyhow::Result<(TypeRef, Vec<RustType>)> {
    let variants = if uses_one_of {
      &prop_schema.one_of
    } else {
      &prop_schema.any_of
    };

    if let Some(type_ref) = self.try_build_nullable_union(variants, prop_schema)? {
      return Ok((type_ref, vec![]));
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
      let enum_name = format!("{parent_name}.{prop_name}");
      let enum_types = if uses_one_of {
        self
          .convert_union_enum(&enum_name, prop_schema, UnionKind::OneOf)
          .await?
      } else {
        self
          .convert_union_enum(&enum_name, prop_schema, UnionKind::AnyOf)
          .await?
      };
      let type_name = if let Some(RustType::Enum(enum_def)) = enum_types.last() {
        enum_def.name.clone()
      } else {
        to_rust_type_name(&enum_name)
      };
      Ok((TypeRef::new(&type_name), enum_types))
    } else {
      Ok((self.schema_to_type_ref(prop_schema)?, vec![]))
    }
  }

  /// Resolves a property type with special handling for inline `anyOf`/`oneOf` unions.
  ///
  /// # Parameters
  /// - `parent_name`: Name of the owning schema (used for inline enum naming).
  /// - `prop_name`: The property key being converted.
  /// - `prop_schema_ref`: The schema reference describing the property.
  ///
  async fn resolve_property_type_with_inline_enums(
    &self,
    parent_name: &str,
    prop_name: &str,
    prop_schema_ref: &ObjectOrReference<ObjectSchema>,
  ) -> anyhow::Result<(TypeRef, Vec<RustType>)> {
    if let ObjectOrReference::Ref { ref_path, .. } = prop_schema_ref {
      if let Some(ref_name) = SchemaGraph::extract_ref_name(ref_path) {
        let mut type_ref = TypeRef::new(to_rust_type_name(&ref_name));
        if self.graph.is_cyclic(&ref_name) {
          type_ref = type_ref.with_boxed();
        }
        return Ok((type_ref, vec![]));
      }

      if let Ok(prop_schema) = prop_schema_ref.resolve(self.graph.spec()) {
        return Ok((self.schema_to_type_ref(&prop_schema)?, vec![]));
      }

      return Ok((TypeRef::new("serde_json::Value"), vec![]));
    }

    let Ok(prop_schema) = prop_schema_ref.resolve(self.graph.spec()) else {
      return Ok((TypeRef::new("serde_json::Value"), vec![]));
    };

    let has_one_of = !prop_schema.one_of.is_empty();
    let has_any_of = !prop_schema.any_of.is_empty();

    if !has_one_of && !has_any_of {
      return Ok((self.schema_to_type_ref(&prop_schema)?, vec![]));
    }

    self
      .convert_inline_union_type(parent_name, prop_name, &prop_schema, has_one_of)
      .await
  }

  /// Converts a property schema reference into a `TypeRef` without generating inline unions.
  ///
  /// # Parameters
  /// - `prop_schema_ref`: The schema reference describing the property.
  ///
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
      ObjectOrReference::Object(_) => {
        if let Ok(prop_schema) = prop_schema_ref.resolve(self.graph.spec()) {
          self.schema_to_type_ref(&prop_schema)
        } else {
          Ok(TypeRef::new("serde_json::Value"))
        }
      }
    }
  }

  /// Extracts metadata (docs, validation, flags) from a resolved property schema.
  ///
  /// # Parameters
  /// - `prop_name`: The property name used for diagnostics.
  /// - `is_required`: Indicates whether the property is required.
  /// - `prop_schema_ref`: The schema reference being resolved.
  ///
  async fn extract_field_metadata(
    &self,
    prop_name: &str,
    is_required: bool,
    prop_schema_ref: &ObjectOrReference<ObjectSchema>,
  ) -> FieldMetadata {
    if let Ok(prop_schema) = prop_schema_ref.resolve(self.graph.spec()) {
      let docs = Self::docs(prop_schema.description.as_ref()).await;
      let validation_attrs = SchemaConverter::extract_validation_attrs(prop_name, is_required, &prop_schema);
      let regex_validation = Self::extract_validation_pattern(prop_name, &prop_schema).cloned();
      let default_value = Self::extract_default_value(&prop_schema);
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

  /// Builds serde attributes for a field (only handles renames; container-level skips live elsewhere).
  ///
  /// # Parameters
  /// - `prop_name`: The original property name from the schema.
  ///
  fn serde_renamed_if_needed(prop_name: &str) -> Vec<String> {
    let mut serde_attrs = vec![];
    let rust_field_name = to_rust_field_name(prop_name);
    if rust_field_name != prop_name {
      serde_attrs.push(format!("rename = \"{prop_name}\""));
    }
    serde_attrs
  }

  /// Filters out regex validation for types that do not support pattern enforcement.
  ///
  /// # Parameters
  /// - `rust_type`: The resolved type reference for the field.
  /// - `regex`: The candidate regex string to apply.
  ///
  fn filter_regex_validation(rust_type: &TypeRef, regex: Option<String>) -> Option<String> {
    match &rust_type.base_type {
      RustPrimitive::DateTime | RustPrimitive::Date | RustPrimitive::Time | RustPrimitive::Uuid => None,
      _ => regex,
    }
  }

  /// Wraps a `TypeRef` with `Option` when the field is optional and not already nullable.
  ///
  /// # Parameters
  /// - `rust_type`: The base type reference.
  /// - `is_optional`: Indicates whether the field is optional.
  ///
  fn apply_optionality(rust_type: TypeRef, is_optional: bool) -> TypeRef {
    let is_nullable = rust_type.nullable;
    if is_optional && !is_nullable {
      rust_type.with_option()
    } else {
      rust_type
    }
  }

  /// Deduplicates field names that collide after conversion to `snake_case`.
  ///
  /// # Parameters
  /// - `fields`: Mutable list of field definitions subject to deduplication.
  ///
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

  /// Builds a `FieldDef` from the gathered metadata, serde attributes, and type information.
  ///
  /// # Parameters
  /// - `prop_name`: Original property name used for identifier generation.
  /// - `rust_type`: The final `TypeRef` assigned to the field.
  /// - `serde_attrs`: Serde attribute list for the field.
  /// - `metadata`: Aggregated metadata describing docs, defaults, and flags.
  /// - `regex_validation`: Optional regex validation string.
  /// - `extra_attrs`: Extra Rust attributes to apply to the field.
  ///
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

  /// Resolves a field's type based on the inlining policy.
  ///
  /// # Parameters
  /// - `parent_name`: The containing struct name used for inline enum naming.
  /// - `prop_name`: The property key being processed.
  /// - `prop_schema_ref`: Schema reference describing the property.
  /// - `policy`: Controls whether inline unions are generated.
  ///
  async fn resolve_field_type(
    &self,
    parent_name: &str,
    prop_name: &str,
    prop_schema_ref: &ObjectOrReference<ObjectSchema>,
    policy: InlinePolicy,
  ) -> anyhow::Result<(TypeRef, Vec<RustType>)> {
    match policy {
      InlinePolicy::None => Ok((self.resolve_property_type(prop_schema_ref)?, Vec::new())),
      InlinePolicy::InlineUnions => {
        self
          .resolve_property_type_with_inline_enums(parent_name, prop_name, prop_schema_ref)
          .await
      }
    }
  }

  /// Applies discriminator-specific attributes and defaults to a field.
  ///
  /// # Parameters
  /// - `prop_name`: The property name being evaluated.
  /// - `schema_name`: Optional schema name used to locate discriminator mappings.
  /// - `metadata`: Field metadata prior to discriminator adjustments.
  /// - `serde_attrs`: Existing serde attributes for the field.
  /// - `final_type`: The final resolved type for the field.
  ///
  fn apply_discriminator_attributes(
    &self,
    prop_name: &str,
    schema_name: Option<&str>,
    metadata: FieldMetadata,
    serde_attrs: Vec<String>,
    final_type: &TypeRef,
  ) -> (FieldMetadata, Vec<String>, Vec<String>, Option<String>) {
    if !self.graph.discriminator_fields().contains(prop_name) {
      let regex = metadata.regex_validation.clone();
      return (metadata, serde_attrs, Vec::new(), regex);
    }

    let mut metadata = metadata;
    let mut serde_attrs = serde_attrs;

    metadata.docs.clear();
    metadata.validation_attrs.clear();
    let regex_validation = None;
    let extra_attrs = vec!["#[doc(hidden)]".to_string()];

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

    if metadata.default_value.is_none() && final_type.base_type == RustPrimitive::String && !final_type.is_array {
      metadata.default_value = Some(serde_json::Value::String(String::new()));
    }

    if metadata
      .default_value
      .as_ref()
      .is_none_or(|value| value.as_str().is_some_and(str::is_empty))
    {
      serde_attrs.push("skip".to_string());
    }

    (metadata, serde_attrs, extra_attrs, regex_validation)
  }

  /// Processes a single property into a `FieldDef` and collects any inline helper types.
  ///
  /// # Parameters
  /// - `parent_name`: The containing struct name for naming inline enums.
  /// - `prop_name`: The property key being processed.
  /// - `prop_schema_ref`: The schema reference for the property.
  /// - `is_required`: Indicates whether the property is required.
  /// - `policy`: Controls union inlining.
  /// - `schema_name`: Optional schema name used for discriminator handling.
  ///
  async fn process_single_field(
    &self,
    parent_name: &str,
    prop_name: &str,
    prop_schema_ref: &ObjectOrReference<ObjectSchema>,
    is_required: bool,
    policy: InlinePolicy,
    schema_name: Option<&str>,
  ) -> anyhow::Result<(FieldDef, Vec<RustType>)> {
    let (rust_type, generated_types) = self
      .resolve_field_type(parent_name, prop_name, prop_schema_ref, policy)
      .await?;

    let final_type = Self::apply_optionality(rust_type, !is_required);

    let metadata = self
      .extract_field_metadata(prop_name, is_required, prop_schema_ref)
      .await;
    let serde_attrs = Self::serde_renamed_if_needed(prop_name);

    let (metadata, serde_attrs, extra_attrs, regex_validation) =
      self.apply_discriminator_attributes(prop_name, schema_name, metadata, serde_attrs, &final_type);

    let regex_validation =
      regex_validation.or_else(|| Self::filter_regex_validation(&final_type, metadata.regex_validation.clone()));
    let field = Self::build_field_def(
      prop_name,
      final_type,
      serde_attrs,
      metadata,
      regex_validation,
      extra_attrs,
    );

    Ok((field, generated_types))
  }

  /// Single, policy-driven field converter used by struct generation.
  ///
  /// # Parameters
  /// - `parent_name`: Name of the struct receiving the fields.
  /// - `schema`: The source schema providing properties.
  /// - `policy`: Controls whether inline unions are generated.
  /// - `exclude_field`: Optional property to skip (e.g., discriminator field).
  /// - `schema_name`: Optional schema identifier for discriminator lookups.
  ///
  fn convert_fields_core<'b>(
    &'b self,
    parent_name: &'b str,
    schema: &'b ObjectSchema,
    policy: InlinePolicy,
    exclude_field: Option<&'b str>,
    schema_name: Option<&'b str>,
  ) -> FieldConversionFuture<'b> {
    Box::pin(async move {
      let required_set: HashSet<&str> = schema.required.iter().map(String::as_str).collect();

      let mut fields = Vec::new();
      let mut inline_types = Vec::new();

      for (prop_name, prop_schema_ref) in &schema.properties {
        if exclude_field == Some(prop_name.as_str()) {
          continue;
        }

        let is_required = required_set.contains(prop_name.as_str());
        let (field, generated_types) = self
          .process_single_field(
            parent_name,
            prop_name,
            prop_schema_ref,
            is_required,
            policy,
            schema_name,
          )
          .await?;

        fields.push(field);
        inline_types.extend(generated_types);
      }

      Self::deduplicate_field_names(&mut fields);
      Ok((fields, inline_types))
    })
  }

  /// Converts schema properties to struct fields without generating inline helper types.
  ///
  /// # Parameters
  /// - `schema`: The schema whose properties are being converted.
  ///
  async fn convert_fields(&self, schema: &ObjectSchema) -> anyhow::Result<Vec<FieldDef>> {
    self
      .convert_fields_core("<inline>", schema, InlinePolicy::None, None, None)
      .await
      .map(|(f, _)| f)
  }

  /// Maps `OpenAPI` format values to their corresponding Rust primitive.
  /// defined in https://spec.openapis.org/registry/format
  ///
  /// # Parameters
  /// - `format`: Optional format string declared on the schema.
  ///
  fn format_to_primitive(format: Option<&String>) -> Option<RustPrimitive> {
    match format?.as_str() {
      "int8" => Some(RustPrimitive::I8),
      "int16" => Some(RustPrimitive::I16),
      "int32" => Some(RustPrimitive::I32),
      "int64" => Some(RustPrimitive::I64),
      "uint8" => Some(RustPrimitive::U8),
      "uint16" => Some(RustPrimitive::U16),
      "uint32" => Some(RustPrimitive::U32),
      "uint64" => Some(RustPrimitive::U64),
      "float" => Some(RustPrimitive::F32),
      "double" => Some(RustPrimitive::F64),
      "date" => Some(RustPrimitive::Date),
      "date-time" => Some(RustPrimitive::DateTime),
      "time" => Some(RustPrimitive::Time),
      "duration" => Some(RustPrimitive::Duration),
      "byte" | "binary" => Some(RustPrimitive::Bytes),
      "uuid" => Some(RustPrimitive::Uuid),
      _ => None,
    }
  }

  /// Attempts to extract a schema name from a `$ref` path.
  ///
  /// # Parameters
  /// - `obj_ref`: The object-or-reference pointing at the schema.
  ///
  fn try_extract_ref_name(obj_ref: &ObjectOrReference<ObjectSchema>) -> Option<String> {
    match obj_ref {
      ObjectOrReference::Ref { ref_path, .. } => SchemaGraph::extract_ref_name(ref_path),
      ObjectOrReference::Object(_) => None,
    }
  }

  /// Extracts the first `$ref` from a schema's `oneOf` array, if present.
  ///
  /// # Parameters
  /// - `schema`: The schema containing the `oneOf` array.
  ///
  fn try_extract_first_oneof_ref(schema: &ObjectSchema) -> Option<String> {
    schema.one_of.iter().find_map(Self::try_extract_ref_name)
  }

  /// Extracts all `$ref` names from a list of union variants.
  ///
  /// # Parameters
  /// - `variants`: The list of union variants to scan.
  ///
  fn extract_all_variant_refs(variants: &[ObjectOrReference<ObjectSchema>]) -> BTreeSet<String> {
    variants.iter().filter_map(Self::try_extract_ref_name).collect()
  }

  /// Finds an existing schema that has the same `oneOf`/`anyOf` variant references.
  ///
  /// # Parameters
  /// - `variants`: The set of variants defining the inline union.
  ///
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

  /// Checks if a schema represents a null type.
  ///
  /// # Parameters
  /// - `schema`: The schema under inspection.
  ///
  fn is_null_schema(schema: &ObjectSchema) -> bool {
    schema.schema_type == Some(SchemaTypeSet::Single(SchemaType::Null))
  }

  /// Returns true if the schema is nullable or contains null as one of its types.
  ///
  /// # Parameters
  /// - `schema`: The schema under inspection.
  ///
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

  /// Converts `OpenAPI` array schema items to a Rust `TypeRef` representing the element type.
  ///
  /// # Parameters
  /// - `schema`: The array schema whose items should be converted.
  ///
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

    if let Some(ref_name) = Self::try_extract_first_oneof_ref(&items_schema) {
      return Ok(TypeRef::new(to_rust_type_name(&ref_name)));
    }

    self.schema_to_type_ref(&items_schema)
  }

  /// Finds the non-null variant in a two-element union of `[T, null]`.
  ///
  /// # Parameters
  /// - `variants`: The collection of union variants to inspect.
  ///
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
        .is_some_and(|s| Self::is_null_schema(&s))
    });

    if !has_null {
      return None;
    }

    variants.iter().find(|v| {
      v.resolve(self.graph.spec())
        .ok()
        .is_some_and(|s| !Self::is_null_schema(&s))
    })
  }

  /// Attempts to resolve a schema by its `title` property.
  ///
  /// # Parameters
  /// - `schema`: The schema whose title should be inspected.
  ///
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

  /// Attempts to convert `oneOf`/`anyOf` union variants to a reusable `TypeRef`.
  ///
  /// # Parameters
  /// - `variants`: The union variants to analyze.
  ///
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

      if let Some(ref_name) = Self::try_extract_first_oneof_ref(&resolved) {
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

  /// Maps a single primitive `SchemaType` to a Rust `TypeRef`.
  ///
  /// # Parameters
  /// - `schema_type`: The primitive schema type to map.
  /// - `schema`: The enclosing schema providing additional metadata.
  ///
  #[allow(clippy::trivially_copy_pass_by_ref)]
  fn map_single_primitive_type(&self, schema_type: &SchemaType, schema: &ObjectSchema) -> anyhow::Result<TypeRef> {
    match schema_type {
      SchemaType::String => {
        let primitive = Self::format_to_primitive(schema.format.as_ref()).unwrap_or(RustPrimitive::String);
        Ok(TypeRef::new(primitive))
      }
      SchemaType::Number => {
        let primitive = Self::format_to_primitive(schema.format.as_ref()).unwrap_or(RustPrimitive::F64);
        Ok(TypeRef::new(primitive))
      }
      SchemaType::Integer => {
        let primitive = Self::format_to_primitive(schema.format.as_ref()).unwrap_or(RustPrimitive::I64);
        Ok(TypeRef::new(primitive))
      }
      SchemaType::Boolean => Ok(TypeRef::new(RustPrimitive::Bool)),
      SchemaType::Array => {
        let item_type = self.convert_array_items(schema)?;
        let unique_items = schema.unique_items.unwrap_or(false);
        Ok(
          TypeRef::new(item_type.to_rust_type())
            .with_vec()
            .with_unique_items(unique_items),
        )
      }
      SchemaType::Object => Ok(TypeRef::new(RustPrimitive::Value)),
      SchemaType::Null => Ok(TypeRef::new(RustPrimitive::Unit).with_option()),
    }
  }

  /// Converts nullable primitive types from `SchemaTypeSet::Multiple` into `Option<T>`.
  ///
  /// # Parameters
  /// - `types`: The collection of primitive schema types in the union.
  /// - `schema`: The schema providing additional metadata (e.g., formats).
  ///
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

  /// Converts an arbitrary schema into a `TypeRef`, generating fallbacks when needed.
  ///
  /// # Parameters
  /// - `schema`: The schema to convert.
  ///
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
      let variants = if schema.one_of.is_empty() {
        &schema.any_of
      } else {
        &schema.one_of
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

  #[tokio::test]
  async fn test_discriminated_union_uses_struct_variants() {
    let mut one_of_schema = ObjectSchema::default();

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

    let string_variant = ObjectSchema {
      schema_type: Some(SchemaTypeSet::Single(SchemaType::String)),
      title: Some("StringVariant".to_string()),
      ..Default::default()
    };

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
      .await
      .unwrap();

    assert_eq!(result.len(), 1, "Should generate exactly one type");

    if let RustType::Enum(enum_def) = &result[0] {
      assert_eq!(enum_def.name, "TestUnion");
      assert!(enum_def.discriminator.is_some(), "Should have discriminator");

      for variant in &enum_def.variants {
        match &variant.content {
          VariantContent::Struct(fields) => {
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

  #[tokio::test]
  async fn test_catch_all_enum_generates_two_level_structure() {
    let mut any_of_schema = ObjectSchema::default();

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
      .await
      .unwrap();

    assert_eq!(
      result.len(),
      2,
      "Should generate TWO types (inner Known enum + outer untagged wrapper)"
    );

    if let RustType::Enum(inner_enum) = &result[0] {
      assert_eq!(inner_enum.name, "CatchAllEnumKnown");
      assert_eq!(inner_enum.variants.len(), 2, "Should have 2 known variants");
      assert!(inner_enum.serde_attrs.is_empty(), "Inner enum should not be untagged");

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

    if let RustType::Enum(outer_enum) = &result[1] {
      assert_eq!(outer_enum.name, "CatchAllEnum");
      assert_eq!(outer_enum.variants.len(), 2, "Should have 2 variants (Known + Other)");
      assert!(
        outer_enum.serde_attrs.contains(&"untagged".to_string()),
        "Outer enum should be untagged"
      );

      let known_variant = outer_enum.variants.iter().find(|v| v.name == "Known").unwrap();
      match &known_variant.content {
        VariantContent::Tuple(types) => {
          assert_eq!(types.len(), 1);
          assert_eq!(
            types[0].base_type,
            RustPrimitive::Custom("CatchAllEnumKnown".to_string())
          );
        }
        _ => panic!("Known variant should be a tuple variant"),
      }

      let other_variant = outer_enum.variants.iter().find(|v| v.name == "Other").unwrap();
      match &other_variant.content {
        VariantContent::Tuple(types) => {
          assert_eq!(types.len(), 1);
          assert_eq!(types[0].base_type, RustPrimitive::String);
        }
        _ => panic!("Other variant should be a tuple variant"),
      }
    } else {
      panic!("Second type should be outer wrapper enum, got {:?}", result[1]);
    }
  }

  #[tokio::test]
  async fn test_simple_string_enum() {
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
      .await
      .unwrap();

    assert_eq!(result.len(), 1, "Should generate exactly one enum");

    if let RustType::Enum(enum_def) = &result[0] {
      assert_eq!(enum_def.name, "SimpleEnum");
      assert_eq!(enum_def.variants.len(), 3);
      assert!(enum_def.discriminator.is_none());
      assert!(enum_def.serde_attrs.is_empty(), "Simple enum should not be untagged");

      for variant in &enum_def.variants {
        assert!(matches!(variant.content, VariantContent::Unit));
        assert!(variant.serde_attrs.iter().any(|a| a.starts_with("rename")));
      }
    } else {
      panic!("Expected enum, got {:?}", result[0]);
    }
  }

  #[tokio::test]
  async fn test_nullable_pattern_detection() {
    let mut any_of_schema = ObjectSchema::default();

    any_of_schema.any_of.push(ObjectOrReference::Object(ObjectSchema {
      schema_type: Some(SchemaTypeSet::Single(SchemaType::String)),
      ..Default::default()
    }));

    any_of_schema.any_of.push(ObjectOrReference::Object(ObjectSchema {
      schema_type: Some(SchemaTypeSet::Single(SchemaType::Null)),
      ..Default::default()
    }));

    let mut schemas = BTreeMap::new();
    schemas.insert("NullableString".to_string(), any_of_schema);

    let spec = create_test_spec(schemas);
    let graph = SchemaGraph::new(spec).unwrap();
    let converter = SchemaConverter::new(&graph);

    let type_ref = converter
      .schema_to_type_ref(graph.get_schema("NullableString").unwrap())
      .unwrap();

    assert_eq!(type_ref.base_type, RustPrimitive::String);
    assert!(
      type_ref.nullable,
      "Should detect nullable pattern and set nullable=true"
    );
    assert_eq!(type_ref.to_rust_type(), "Option<String>");
  }

  #[tokio::test]
  async fn test_untagged_any_of_enum() {
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
      .await
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

  #[tokio::test]
  async fn test_discriminated_base_struct_renamed_and_enum_references_it() {
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
      .await
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

  #[tokio::test]
  async fn test_discriminated_child_inlines_parent_fields_and_boxes_cycles() {
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
      .await
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

  #[tokio::test]
  async fn test_discriminator_field_removed_from_child_struct() {
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
      .await
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

  #[tokio::test]
  async fn test_skip_serializing_none_only_added_when_options_present() {
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
      .convert_struct("OptionalStruct", graph.get_schema("OptionalStruct").unwrap(), None)
      .await
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
      .convert_struct("RequiredStruct", graph.get_schema("RequiredStruct").unwrap(), None)
      .await
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

  #[tokio::test]
  async fn test_const_values_become_field_defaults() {
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
      .convert_struct("ConstStruct", graph.get_schema("ConstStruct").unwrap(), None)
      .await
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

  #[tokio::test]
  async fn test_discriminator_mapping_sets_default_value() {
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
      .await
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

  #[tokio::test]
  async fn test_empty_schema_produces_type_alias() {
    let mut schemas = BTreeMap::new();
    schemas.insert("Empty".to_string(), ObjectSchema::default());

    let spec = create_test_spec(schemas);
    let graph = SchemaGraph::new(spec).unwrap();
    let converter = SchemaConverter::new(&graph);

    let generated = converter
      .convert_schema("Empty", graph.get_schema("Empty").unwrap())
      .await
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

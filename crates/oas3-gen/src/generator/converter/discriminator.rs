use std::{
  cmp::Reverse,
  collections::{BTreeSet, HashMap},
  sync::Arc,
};

use oas3::spec::{ObjectOrReference, ObjectSchema};
use string_cache::DefaultAtom;

use super::{SchemaExt, metadata::FieldMetadata};
use crate::generator::{
  ast::{DiscriminatedEnumDefBuilder, DiscriminatedVariant, RustType, SerdeAttribute, TypeRef},
  converter::metadata,
  naming::identifiers::to_rust_type_name,
  schema_registry::{ReferenceExtractor, SchemaRegistry},
};

pub(crate) struct DiscriminatorInfo {
  pub value: Option<DefaultAtom>,
  pub is_base: bool,
  pub has_enum: bool,
}

pub(crate) struct DiscriminatorAttributesResult {
  pub metadata: FieldMetadata,
  pub serde_attrs: Vec<SerdeAttribute>,
  pub doc_hidden: bool,
}

pub(crate) fn get_discriminator_info(
  prop_name: &str,
  parent_schema: &ObjectSchema,
  prop_schema: &ObjectSchema,
  discriminator_mapping: Option<&(String, String)>,
) -> Option<DiscriminatorInfo> {
  let is_child_discriminator = discriminator_mapping
    .as_ref()
    .is_some_and(|(prop, _)| prop == prop_name);

  let is_base_discriminator = parent_schema
    .discriminator
    .as_ref()
    .is_some_and(|d| d.property_name == prop_name);

  let has_enum = !prop_schema.enum_values.is_empty();

  if is_child_discriminator {
    let (_, value) = discriminator_mapping?;
    Some(DiscriminatorInfo {
      value: Some(DefaultAtom::from(value.as_str())),
      is_base: false,
      has_enum,
    })
  } else if is_base_discriminator {
    Some(DiscriminatorInfo {
      value: None,
      is_base: true,
      has_enum,
    })
  } else {
    None
  }
}

pub(crate) fn apply_discriminator_attributes(
  mut metadata: FieldMetadata,
  mut serde_attrs: Vec<SerdeAttribute>,
  final_type: &TypeRef,
  discriminator_info: Option<&DiscriminatorInfo>,
) -> DiscriminatorAttributesResult {
  let should_hide = discriminator_info
    .as_ref()
    .is_some_and(|d| !d.has_enum && (d.value.is_some() || d.is_base));

  if !should_hide {
    return DiscriminatorAttributesResult {
      metadata,
      serde_attrs,
      doc_hidden: false,
    };
  }

  let disc_info = discriminator_info.expect("checked above");

  metadata.docs.clear();
  metadata.validation_attrs.clear();

  if disc_info.value.is_some() {
    metadata.default_value = Some(serde_json::Value::String(disc_info.value.as_ref().unwrap().to_string()));
    serde_attrs.push(SerdeAttribute::SkipDeserializing);
    serde_attrs.push(SerdeAttribute::Default);
  } else {
    serde_attrs.clear();
    serde_attrs.push(SerdeAttribute::Skip);
    if final_type.is_string_like() {
      metadata.default_value = Some(serde_json::Value::String(String::new()));
    }
  }

  DiscriminatorAttributesResult {
    metadata,
    serde_attrs,
    doc_hidden: true,
  }
}

pub(crate) struct DiscriminatorHandler<'a> {
  graph: &'a Arc<SchemaRegistry>,
  reachable_schemas: Option<&'a Arc<BTreeSet<String>>>,
}

impl<'a> DiscriminatorHandler<'a> {
  pub(crate) fn new(graph: &'a Arc<SchemaRegistry>, reachable_schemas: Option<&'a Arc<BTreeSet<String>>>) -> Self {
    Self {
      graph,
      reachable_schemas,
    }
  }

  pub(crate) fn detect_discriminated_parent(
    &self,
    schema: &ObjectSchema,
    merged_schema_cache: &mut HashMap<String, ObjectSchema>,
    merge_fn: impl Fn(&str, &ObjectSchema, &mut HashMap<String, ObjectSchema>) -> anyhow::Result<ObjectSchema>,
  ) -> Option<ObjectSchema> {
    if schema.all_of.is_empty() {
      return None;
    }

    schema.all_of.iter().find_map(|all_of_ref| {
      let ObjectOrReference::Ref { ref_path, .. } = all_of_ref else {
        return None;
      };
      let parent_name = SchemaRegistry::extract_ref_name(ref_path)?;
      let parent_schema = self.graph.get_schema(&parent_name)?;

      parent_schema.discriminator.as_ref()?;

      let merged_parent = merge_fn(&parent_name, parent_schema, merged_schema_cache).ok()?;
      if merged_parent.is_discriminated_base_type() {
        Some(merged_parent)
      } else {
        None
      }
    })
  }

  pub(crate) fn create_discriminated_enum(
    &self,
    base_name: &str,
    schema: &ObjectSchema,
    base_struct_name: &str,
  ) -> anyhow::Result<RustType> {
    let Some(discriminator_field) = schema.discriminator.as_ref().map(|d| &d.property_name) else {
      anyhow::bail!("Failed to find discriminator property for schema '{base_name}'");
    };

    let children = self.extract_discriminator_children(schema);
    let enum_name = to_rust_type_name(base_name);

    let variants: Vec<_> = children
      .into_iter()
      .map(|(disc_values, child_schema_name)| {
        let child_type_name = to_rust_type_name(&child_schema_name);
        let variant_name = child_type_name
          .strip_prefix(&enum_name)
          .filter(|s| !s.is_empty())
          .map(|s| {
            let mut chars = s.chars();
            match chars.next() {
              None => String::new(),
              Some(first) => first.to_uppercase().chain(chars).collect(),
            }
          })
          .unwrap_or(child_type_name.clone());

        DiscriminatedVariant {
          discriminator_values: disc_values,
          variant_name,
          type_name: TypeRef::new(child_type_name).with_boxed(),
        }
      })
      .collect();

    let base_variant_name = to_rust_type_name(base_name.split('.').next_back().unwrap_or(base_name));
    let fallback = Some(DiscriminatedVariant {
      discriminator_values: vec![],
      variant_name: base_variant_name,
      type_name: TypeRef::new(base_struct_name).with_boxed(),
    });

    Ok(RustType::DiscriminatedEnum(
      DiscriminatedEnumDefBuilder::default()
        .name(enum_name)
        .docs(metadata::extract_docs(schema.description.as_ref()))
        .discriminator_field(discriminator_field.clone())
        .variants(variants)
        .fallback(fallback)
        .build()?,
    ))
  }

  /// Extracts child schemas from a discriminator mapping.
  ///
  /// Returns `(discriminator_values, schema_name)` pairs grouped by schema name.
  /// Multiple discriminator values that map to the same schema are collected together.
  /// Results are sorted by inheritance depth (deepest first).
  pub(crate) fn extract_discriminator_children(&self, schema: &ObjectSchema) -> Vec<(Vec<String>, String)> {
    use std::collections::BTreeMap;

    let Some(mapping) = schema.discriminator.as_ref().and_then(|d| d.mapping.as_ref()) else {
      return vec![];
    };

    let mut schema_to_disc_values: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for (disc_value, ref_path) in mapping {
      let Some(schema_name) = SchemaRegistry::extract_ref_name(ref_path) else {
        continue;
      };

      if let Some(filter) = self.reachable_schemas
        && !filter.contains(&schema_name)
      {
        continue;
      }

      schema_to_disc_values
        .entry(schema_name)
        .or_default()
        .push(disc_value.clone());
    }

    let mut children: Vec<_> = schema_to_disc_values
      .into_iter()
      .map(|(name, values)| (values, name))
      .collect();

    let mut depth_memo = HashMap::new();
    children.sort_by_key(|(_, name)| Reverse(compute_inheritance_depth(self.graph, name, &mut depth_memo)));
    children
  }
}

pub(crate) fn compute_inheritance_depth(
  graph: &SchemaRegistry,
  schema_name: &str,
  memo: &mut HashMap<String, usize>,
) -> usize {
  if let Some(&depth) = memo.get(schema_name) {
    return depth;
  }
  let Some(schema) = graph.get_schema(schema_name) else {
    return 0;
  };

  let depth = if schema.all_of.is_empty() {
    0
  } else {
    schema
      .all_of
      .iter()
      .filter_map(ReferenceExtractor::extract_ref_name_from_obj_ref)
      .map(|parent| compute_inheritance_depth(graph, &parent, memo))
      .max()
      .unwrap_or(0)
      + 1
  };

  memo.insert(schema_name.to_string(), depth);
  depth
}

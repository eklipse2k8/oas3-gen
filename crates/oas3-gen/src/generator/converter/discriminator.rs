use std::{
  cmp::Reverse,
  collections::{BTreeMap, BTreeSet, HashMap},
  sync::Arc,
};

use oas3::spec::{ObjectOrReference, ObjectSchema};
use string_cache::DefaultAtom;

use super::{SchemaExt, metadata::FieldMetadata};
use crate::generator::{
  ast::{
    DiscriminatedEnumDef, DiscriminatedEnumDefBuilder, DiscriminatedVariant, EnumMethod, EnumToken, RustType,
    SerdeAttribute, TypeRef, VariantDef,
  },
  converter::metadata,
  naming::identifiers::to_rust_type_name,
  schema_registry::SchemaRegistry,
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

  let has_enum = prop_schema.has_enum_values();

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
    if !schema.has_all_of() {
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
    let Some(mapping) = schema.discriminator.as_ref().and_then(|d| d.mapping.as_ref()) else {
      return vec![];
    };

    let mut schema_to_disc_values: HashMap<String, Vec<String>> = HashMap::with_capacity(mapping.len());
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

    children.sort_by_cached_key(|(_, name)| Reverse(self.graph.get_inheritance_depth(name)));
    children
  }
}

pub(crate) fn ref_path_to_type_name(ref_path: &str) -> Option<String> {
  SchemaRegistry::extract_ref_name(ref_path).map(|name| to_rust_type_name(&name))
}

pub(crate) fn try_build_discriminated_enum_from_variants(
  name: &str,
  schema: &ObjectSchema,
  variants: &[VariantDef],
  methods: Vec<EnumMethod>,
) -> Option<RustType> {
  let discriminator = schema.discriminator.as_ref()?;
  let mapping = discriminator.mapping.as_ref()?;

  if !all_variants_match_mapping(variants, mapping) {
    return None;
  }

  let disc_variants = build_discriminated_variants_from_mapping(variants, mapping);
  Some(RustType::DiscriminatedEnum(DiscriminatedEnumDef {
    name: EnumToken::from_raw(name),
    docs: metadata::extract_docs(schema.description.as_ref()),
    discriminator_field: discriminator.property_name.clone(),
    variants: disc_variants,
    methods,
    ..Default::default()
  }))
}

fn all_variants_match_mapping(variants: &[VariantDef], mapping: &BTreeMap<String, String>) -> bool {
  if variants.is_empty() || mapping.is_empty() {
    return false;
  }

  let variant_types: BTreeSet<String> = variants.iter().filter_map(VariantDef::unboxed_type_name).collect();

  mapping
    .values()
    .filter_map(|ref_path| ref_path_to_type_name(ref_path))
    .all(|type_name| variant_types.contains(&type_name))
}

fn build_discriminated_variants_from_mapping(
  variants: &[VariantDef],
  mapping: &BTreeMap<String, String>,
) -> Vec<DiscriminatedVariant> {
  let mut type_to_disc_values: BTreeMap<String, Vec<String>> = BTreeMap::new();
  for (disc_value, ref_path) in mapping {
    if let Some(expected_type) = ref_path_to_type_name(ref_path) {
      type_to_disc_values
        .entry(expected_type)
        .or_default()
        .push(disc_value.clone());
    }
  }

  type_to_disc_values
    .into_iter()
    .filter_map(|(expected_type, disc_values)| {
      let variant = variants
        .iter()
        .find(|v| v.unboxed_type_name().is_some_and(|name| name == expected_type))?;
      let type_ref = variant.single_wrapped_type()?;

      Some(DiscriminatedVariant {
        discriminator_values: disc_values,
        variant_name: variant.name.to_string(),
        type_name: type_ref.clone(),
      })
    })
    .collect()
}

use std::collections::{BTreeSet, HashSet};

use oas3::spec::{ObjectSchema, SchemaType, SchemaTypeSet};

use super::{cache::SharedSchemaCache, metadata};
use crate::generator::{
  ast::{EnumDef, RustType, SerdeAttribute, TypeRef, VariantContent, VariantDef, default_enum_derives},
  naming::{identifiers::to_rust_type_name, inference as naming},
  schema_registry::SchemaRegistry,
};

/// Optimizes anyOf unions containing string enums and a freeform string.
///
/// Detects patterns like `anyOf: [const "foo", const "bar", type: string]`
/// and generates a two-variant enum: Known(KnownEnum) | Other(String).
/// This provides type safety for known values while accepting unknown ones.
pub(crate) struct StringEnumOptimizer<'a> {
  graph: &'a SchemaRegistry,
  case_insensitive: bool,
}

impl<'a> StringEnumOptimizer<'a> {
  pub(crate) fn new(graph: &'a SchemaRegistry, case_insensitive: bool) -> Self {
    Self {
      graph,
      case_insensitive,
    }
  }

  pub(crate) fn try_convert(
    &self,
    name: &str,
    schema: &ObjectSchema,
    cache: Option<&mut SharedSchemaCache>,
  ) -> Option<Vec<RustType>> {
    if !self.has_freeform_string(schema) {
      return None;
    }

    let known_values = self.collect_known_values(schema);
    if known_values.is_empty() {
      return None;
    }

    Some(self.generate_optimized_types(name, schema, &known_values, cache))
  }

  fn has_freeform_string(&self, schema: &ObjectSchema) -> bool {
    schema.any_of.iter().any(|s| {
      s.resolve(self.graph.spec()).ok().is_some_and(|resolved| {
        resolved.const_value.is_none()
          && resolved.enum_values.is_empty()
          && resolved.schema_type == Some(SchemaTypeSet::Single(SchemaType::String))
      })
    })
  }

  fn collect_known_values(&self, schema: &ObjectSchema) -> Vec<(String, Option<String>, bool)> {
    let mut seen_values = HashSet::new();
    let mut known_values = vec![];

    for variant in &schema.any_of {
      let Ok(resolved) = variant.resolve(self.graph.spec()) else {
        continue;
      };

      if let Some(const_val) = resolved.const_value.as_ref().and_then(|v| v.as_str()) {
        if seen_values.insert(const_val.to_string()) {
          known_values.push((
            const_val.to_string(),
            resolved.description.clone(),
            resolved.deprecated.unwrap_or(false),
          ));
        }
        continue;
      }

      if resolved.schema_type == Some(SchemaTypeSet::Single(SchemaType::String)) {
        for enum_value in &resolved.enum_values {
          if let Some(str_val) = enum_value.as_str()
            && seen_values.insert(str_val.to_string())
          {
            known_values.push((
              str_val.to_string(),
              resolved.description.clone(),
              resolved.deprecated.unwrap_or(false),
            ));
          }
        }
      }
    }
    known_values
  }

  fn generate_optimized_types(
    &self,
    name: &str,
    schema: &ObjectSchema,
    known_values: &[(String, Option<String>, bool)],
    cache: Option<&mut SharedSchemaCache>,
  ) -> Vec<RustType> {
    let base_name = to_rust_type_name(name);

    let mut cache_key_values: Vec<String> = known_values.iter().map(|(v, _, _)| v.clone()).collect();
    cache_key_values.sort();

    let (known_enum_name, inner_enum_type) =
      self.resolve_cached_enum(&base_name, known_values, cache_key_values, cache);

    let outer_enum = Self::build_outer_enum(&base_name, &known_enum_name, schema);

    let mut types = vec![];
    if let Some(ie) = inner_enum_type {
      types.push(ie);
    }
    types.push(outer_enum);
    types
  }

  fn resolve_cached_enum(
    &self,
    base_name: &str,
    known_values: &[(String, Option<String>, bool)],
    cache_key: Vec<String>,
    cache: Option<&mut SharedSchemaCache>,
  ) -> (String, Option<RustType>) {
    if let Some(c) = cache {
      if let Some(existing) = c.get_enum_name(&cache_key) {
        let name = existing.clone();
        if c.is_enum_generated(&cache_key) {
          (name, None)
        } else {
          let def = self.build_known_enum(&name, known_values);
          c.register_enum(cache_key, name.clone());
          c.mark_name_used(name.clone());
          (name, Some(def))
        }
      } else {
        let name = format!("{base_name}Known");
        let def = self.build_known_enum(&name, known_values);
        c.register_enum(cache_key, name.clone());
        c.mark_name_used(name.clone());
        (name, Some(def))
      }
    } else {
      let name = format!("{base_name}Known");
      (name.clone(), Some(self.build_known_enum(&name, known_values)))
    }
  }

  fn build_known_enum(&self, name: &str, values: &[(String, Option<String>, bool)]) -> RustType {
    let mut seen_names = BTreeSet::new();
    let mut variants = vec![];

    for (value, description, deprecated) in values {
      let base_name = to_rust_type_name(value);
      let variant_name = naming::ensure_unique(&base_name, &seen_names);
      seen_names.insert(variant_name.clone());

      variants.push(VariantDef {
        name: variant_name,
        docs: metadata::extract_docs(description.as_ref()),
        content: VariantContent::Unit,
        serde_attrs: vec![SerdeAttribute::Rename(value.clone())],
        deprecated: *deprecated,
      });
    }

    RustType::Enum(EnumDef {
      name: name.to_string(),
      docs: vec!["/// Known values for the string enum.".to_string()],
      variants,
      discriminator: None,
      derives: default_enum_derives(true),
      serde_attrs: vec![],
      outer_attrs: vec![],
      case_insensitive: self.case_insensitive,
      methods: vec![],
    })
  }

  fn build_outer_enum(name: &str, known_type_name: &str, schema: &ObjectSchema) -> RustType {
    let variants = vec![
      VariantDef {
        name: "Known".to_string(),
        docs: vec!["/// A known value.".to_string()],
        content: VariantContent::Tuple(vec![TypeRef::new(known_type_name)]),
        serde_attrs: vec![SerdeAttribute::Default],
        deprecated: false,
      },
      VariantDef {
        name: "Other".to_string(),
        docs: vec!["/// An unknown value.".to_string()],
        content: VariantContent::Tuple(vec![TypeRef::new("String")]),
        serde_attrs: vec![],
        deprecated: false,
      },
    ];

    RustType::Enum(EnumDef {
      name: name.to_string(),
      docs: metadata::extract_docs(schema.description.as_ref()),
      variants,
      discriminator: None,
      derives: default_enum_derives(false),
      serde_attrs: vec![SerdeAttribute::Untagged],
      outer_attrs: vec![],
      case_insensitive: false,
      methods: vec![],
    })
  }
}

use std::collections::{BTreeMap, BTreeSet};

use crate::generator::ast::{RustPrimitive, RustType, TypeRef, VariantContent};

pub(crate) struct DependencyGraph {
  edges: BTreeMap<String, BTreeSet<String>>,
}

impl DependencyGraph {
  pub(crate) fn build(types: &[RustType]) -> Self {
    let mut edges: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();

    for rust_type in types {
      let type_name = rust_type.type_name().to_string();
      let deps = Self::extract_dependencies(rust_type);
      edges.entry(type_name).or_default().extend(deps);
    }

    Self { edges }
  }

  pub(crate) fn dependencies_of(&self, type_name: &str) -> Option<&BTreeSet<String>> {
    self.edges.get(type_name)
  }

  fn extract_dependencies(rust_type: &RustType) -> BTreeSet<String> {
    let mut deps = BTreeSet::new();

    match rust_type {
      RustType::Struct(def) => {
        for field in &def.fields {
          Self::collect_custom_type(&field.rust_type, &mut deps);
        }
      }
      RustType::Enum(def) => {
        for variant in &def.variants {
          if let VariantContent::Tuple(types) = &variant.content {
            for type_ref in types {
              Self::collect_custom_type(type_ref, &mut deps);
            }
          }
        }
      }
      RustType::TypeAlias(def) => {
        Self::collect_custom_type(&def.target, &mut deps);
      }
      RustType::DiscriminatedEnum(def) => {
        for variant in &def.variants {
          Self::collect_custom_type(&variant.type_name, &mut deps);
        }
        if let Some(fallback) = &def.fallback {
          Self::collect_custom_type(&fallback.type_name, &mut deps);
        }
      }
      RustType::ResponseEnum(def) => {
        for variant in &def.variants {
          if let Some(type_ref) = &variant.schema_type {
            Self::collect_custom_type(type_ref, &mut deps);
          }
        }
      }
    }

    deps
  }

  fn collect_custom_type(type_ref: &TypeRef, deps: &mut BTreeSet<String>) {
    if let RustPrimitive::Custom(name) = &type_ref.base_type {
      deps.insert(name.to_string());
    }
  }
}

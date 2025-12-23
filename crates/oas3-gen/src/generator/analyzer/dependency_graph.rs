use std::collections::{BTreeMap, BTreeSet};

use crate::generator::ast::{
  DiscriminatedEnumDef, EnumDef, ResponseEnumDef, RustPrimitive, RustType, TypeAliasDef, TypeRef, VariantContent,
};

pub(crate) struct DependencyGraph {
  edges: BTreeMap<String, BTreeSet<String>>,
}

impl DependencyGraph {
  pub(crate) fn build(types: &[RustType]) -> Self {
    let mut edges = BTreeMap::new();

    for rust_type in types {
      let type_name = rust_type.type_name().to_string();
      let deps = Self::extract_dependencies(rust_type);
      edges
        .entry(type_name)
        .and_modify(|existing: &mut BTreeSet<String>| existing.extend(deps.clone()))
        .or_insert(deps);
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
          Self::collect_custom_types(&field.rust_type, &mut deps);
        }
      }
      RustType::Enum(def) => Self::collect_from_enum(def, &mut deps),
      RustType::TypeAlias(def) => Self::collect_from_type_alias(def, &mut deps),
      RustType::DiscriminatedEnum(def) => Self::collect_from_discriminated_enum(def, &mut deps),
      RustType::ResponseEnum(def) => Self::collect_from_response_enum(def, &mut deps),
    }

    deps
  }

  fn collect_from_enum(def: &EnumDef, deps: &mut BTreeSet<String>) {
    for variant in &def.variants {
      if let VariantContent::Tuple(types) = &variant.content {
        for type_ref in types {
          Self::collect_custom_types(type_ref, deps);
        }
      }
    }
  }

  fn collect_from_type_alias(def: &TypeAliasDef, deps: &mut BTreeSet<String>) {
    Self::collect_custom_types(&def.target, deps);
  }

  fn collect_from_discriminated_enum(def: &DiscriminatedEnumDef, deps: &mut BTreeSet<String>) {
    for variant in &def.variants {
      Self::collect_custom_types(&variant.type_name, deps);
    }
    if let Some(fallback) = &def.fallback {
      Self::collect_custom_types(&fallback.type_name, deps);
    }
  }

  fn collect_from_response_enum(def: &ResponseEnumDef, deps: &mut BTreeSet<String>) {
    for variant in &def.variants {
      if let Some(type_ref) = &variant.schema_type {
        Self::collect_custom_types(type_ref, deps);
      }
    }
  }

  fn collect_custom_types(type_ref: &TypeRef, deps: &mut BTreeSet<String>) {
    if let RustPrimitive::Custom(name) = &type_ref.base_type {
      deps.insert(name.to_string());
    }
  }
}

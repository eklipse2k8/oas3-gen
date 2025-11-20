use std::collections::{BTreeMap, BTreeSet};

use crate::generator::ast::{
  DiscriminatedEnumDef, EnumDef, ResponseEnumDef, RustPrimitive, RustType, TypeAliasDef, VariantContent,
};

pub(crate) struct DependencyGraph {
  dependencies: BTreeMap<String, BTreeSet<String>>,
}

impl DependencyGraph {
  pub(crate) fn build(types: &[RustType]) -> Self {
    let mut dependencies = BTreeMap::new();

    for rust_type in types {
      let type_name = rust_type.type_name().to_string();
      let deps = Self::extract_dependencies(rust_type);
      dependencies
        .entry(type_name)
        .and_modify(|existing: &mut BTreeSet<String>| existing.extend(deps.clone()))
        .or_insert(deps);
    }

    Self { dependencies }
  }

  pub(crate) fn get_dependencies(&self, type_name: &str) -> Option<&BTreeSet<String>> {
    self.dependencies.get(type_name)
  }

  fn extract_dependencies(rust_type: &RustType) -> BTreeSet<String> {
    let mut deps = BTreeSet::new();

    match rust_type {
      RustType::Struct(def) => {
        for field in &def.fields {
          Self::extract_from_type_ref(&field.rust_type, &mut deps);
        }
      }
      RustType::Enum(def) => Self::extract_from_enum(def, &mut deps),
      RustType::TypeAlias(def) => Self::extract_from_type_alias(def, &mut deps),
      RustType::DiscriminatedEnum(def) => Self::extract_from_discriminated_enum(def, &mut deps),
      RustType::ResponseEnum(def) => Self::extract_from_response_enum(def, &mut deps),
    }

    deps
  }

  fn extract_from_enum(def: &EnumDef, deps: &mut BTreeSet<String>) {
    for variant in &def.variants {
      if let VariantContent::Tuple(types) = &variant.content {
        for type_ref in types {
          Self::extract_from_type_ref(type_ref, deps);
        }
      }
    }
  }

  fn extract_from_type_alias(def: &TypeAliasDef, deps: &mut BTreeSet<String>) {
    Self::extract_from_type_ref(&def.target, deps);
  }

  fn extract_from_discriminated_enum(def: &DiscriminatedEnumDef, deps: &mut BTreeSet<String>) {
    for variant in &def.variants {
      deps.insert(variant.type_name.clone());
    }
    if let Some(fallback) = &def.fallback {
      deps.insert(fallback.type_name.clone());
    }
  }

  fn extract_from_response_enum(def: &ResponseEnumDef, deps: &mut BTreeSet<String>) {
    for variant in &def.variants {
      if let Some(type_ref) = &variant.schema_type {
        Self::extract_from_type_ref(type_ref, deps);
      }
    }
  }

  fn extract_from_type_ref(type_ref: &crate::generator::ast::TypeRef, deps: &mut BTreeSet<String>) {
    if let RustPrimitive::Custom(name) = &type_ref.base_type {
      deps.insert(name.clone());
    }
  }
}

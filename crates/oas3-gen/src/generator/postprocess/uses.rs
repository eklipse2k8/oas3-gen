use std::collections::{BTreeMap, BTreeSet, btree_map::Entry};

use crate::generator::{
  ast::{MethodKind, RustType, SerdeImpl, StructKind, ValidationAttribute, constants::HttpHeaderRef},
  converter::GenerationTarget,
};

pub(crate) struct RustTypeDeduplication {
  types: Vec<RustType>,
}

impl RustTypeDeduplication {
  pub(crate) fn new(types: Vec<RustType>) -> Self {
    Self { types }
  }

  pub(crate) fn process(self) -> Vec<RustType> {
    let mut map = BTreeMap::new();

    for t in &self.types {
      let name = t.type_name().to_string();
      let priority = t.type_priority();

      match map.entry(name) {
        Entry::Vacant(e) => {
          e.insert(t.clone());
        }
        Entry::Occupied(mut e) => {
          if priority > e.get().type_priority() {
            e.insert(t.clone());
          }
        }
      }
    }

    map.into_values().collect::<Vec<_>>()
  }
}

pub(crate) struct HeaderRefCollection {
  types: Vec<RustType>,
}

impl HeaderRefCollection {
  pub(crate) fn new(types: Vec<RustType>) -> Self {
    Self { types }
  }

  pub(crate) fn process(self) -> Vec<HttpHeaderRef> {
    self
      .types
      .iter()
      .filter_map(|t| match t {
        RustType::Struct(def) if def.kind == StructKind::HeaderParams => Some(def),
        _ => None,
      })
      .flat_map(|def| &def.fields)
      .filter_map(|field| field.original_name.as_deref())
      .collect::<BTreeSet<_>>()
      .into_iter()
      .map(HttpHeaderRef::from)
      .collect()
  }
}

pub(crate) struct ModuleImports {
  types: Vec<RustType>,
  target: GenerationTarget,
}

impl ModuleImports {
  pub(crate) fn new(types: Vec<RustType>, target: GenerationTarget) -> Self {
    Self { types, target }
  }

  pub(crate) fn process(self) -> BTreeSet<String> {
    let mut uses = BTreeSet::new();

    let mut needs_serialize = false;
    let mut needs_deserialize = false;
    let mut needs_validate = false;

    for ty in &self.types {
      needs_serialize |= ty.is_serializable() == SerdeImpl::Derive;
      needs_deserialize |= ty.is_deserializable() == SerdeImpl::Derive;

      if let RustType::Struct(def) = ty {
        needs_validate |= def
          .fields
          .iter()
          .any(|f| f.validation_attrs.contains(&ValidationAttribute::Nested));
        needs_validate |= def.methods.iter().any(|m| matches!(m.kind, MethodKind::Builder { .. }));
      }
    }

    if needs_serialize {
      uses.insert("serde::Serialize".to_string());
    }
    if needs_deserialize {
      uses.insert("serde::Deserialize".to_string());
    }
    if needs_validate {
      uses.insert("validator::Validate".to_string());
    }
    if self.target == GenerationTarget::Server {
      uses.insert("axum::response::IntoResponse".to_string());
    }

    uses
  }
}

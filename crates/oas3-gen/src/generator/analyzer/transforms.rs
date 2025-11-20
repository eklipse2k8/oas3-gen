use std::collections::{BTreeMap, BTreeSet};

use super::type_usage::TypeUsage;
use crate::generator::ast::{DeriveTrait, EnumDef, FieldDef, RustType, StructDef, StructKind, default_struct_derives};

const SKIP_SERIALIZING_NONE: &str = "oas3_gen_support::skip_serializing_none";

pub(crate) fn update_derives_from_usage(rust_types: &mut [RustType], type_usage: &BTreeMap<String, TypeUsage>) {
  for rust_type in rust_types {
    match rust_type {
      RustType::Struct(def) => process_struct(def, type_usage),
      RustType::Enum(def) => process_enum(def, type_usage),
      _ => {}
    }
  }
}

fn process_struct(def: &mut StructDef, type_usage: &BTreeMap<String, TypeUsage>) {
  let usage = get_usage(&def.name, type_usage);

  let derives = calculate_struct_derives(def.kind, usage);
  def.derives = derives;

  if usage == TypeUsage::ResponseOnly {
    strip_validation_attrs(&mut def.fields);
  }

  let needs_serialization = matches!(usage, TypeUsage::RequestOnly | TypeUsage::Bidirectional);
  adjust_skip_serializing_none(def, needs_serialization);
}

fn process_enum(def: &mut EnumDef, type_usage: &BTreeMap<String, TypeUsage>) {
  let usage = get_usage(&def.name, type_usage);

  let mut derives = def.derives.clone();
  derives.remove(&DeriveTrait::Serialize);
  derives.remove(&DeriveTrait::Deserialize);

  apply_usage_derives(&mut derives, usage);

  def.derives = derives;
}

fn get_usage(name: &str, map: &BTreeMap<String, TypeUsage>) -> TypeUsage {
  map.get(name).copied().unwrap_or(TypeUsage::Bidirectional)
}

fn calculate_struct_derives(kind: StructKind, usage: TypeUsage) -> BTreeSet<DeriveTrait> {
  let mut derives = default_struct_derives();

  if kind == StructKind::OperationRequest {
    derives.remove(&DeriveTrait::PartialEq);
    derives.insert(DeriveTrait::Validate);
  } else {
    apply_usage_derives(&mut derives, usage);
  }

  derives
}

fn apply_usage_derives(derives: &mut BTreeSet<DeriveTrait>, usage: TypeUsage) {
  match usage {
    TypeUsage::RequestOnly => {
      derives.insert(DeriveTrait::Serialize);
      derives.insert(DeriveTrait::Validate);
    }
    TypeUsage::ResponseOnly => {
      derives.insert(DeriveTrait::Deserialize);
    }
    TypeUsage::Bidirectional => {
      derives.insert(DeriveTrait::Serialize);
      derives.insert(DeriveTrait::Deserialize);
      derives.insert(DeriveTrait::Validate);
    }
  }
}

fn strip_validation_attrs(fields: &mut [FieldDef]) {
  for field in fields {
    field.validation_attrs.clear();
    field.regex_validation = None;
  }
}

fn adjust_skip_serializing_none(def: &mut StructDef, needs_serialization: bool) {
  def.outer_attrs.retain(|attr| !matches_skip_serializing_none(attr));

  if needs_serialization && has_nullable_fields(&def.fields) && def.kind != StructKind::OperationRequest {
    def.outer_attrs.push(SKIP_SERIALIZING_NONE.to_string());
  }
}

fn matches_skip_serializing_none(attr: &str) -> bool {
  let trimmed = attr.trim();
  trimmed == SKIP_SERIALIZING_NONE || trimmed == format!("#[{SKIP_SERIALIZING_NONE}]")
}

fn has_nullable_fields(fields: &[FieldDef]) -> bool {
  fields.iter().any(|field| field.rust_type.nullable)
}

use std::collections::BTreeMap;

use super::type_usage::TypeUsage;
use crate::generator::ast::{EnumDef, FieldDef, RustType, StructDef, StructKind};

const SERIALIZE: &str = "Serialize";
const DESERIALIZE: &str = "Deserialize";
const VALIDATE: &str = "validator::Validate";
const SKIP_SERIALIZING_NONE: &str = "oas3_gen_support::skip_serializing_none";

pub(crate) fn update_derives_from_usage(rust_types: &mut [RustType], type_usage: &BTreeMap<String, TypeUsage>) {
  for rust_type in rust_types {
    match rust_type {
      RustType::Struct(def) => apply_struct_usage_policy(def, type_usage),
      RustType::Enum(def) => apply_enum_usage_policy(def, type_usage),
      _ => {}
    }
  }
}

fn apply_struct_usage_policy(def: &mut StructDef, type_usage: &BTreeMap<String, TypeUsage>) {
  let usage = type_usage.get(&def.name).copied().unwrap_or(TypeUsage::Bidirectional);
  let mut derives = base_struct_derives(def.kind);

  match def.kind {
    StructKind::OperationRequest => {
      push_unique(&mut derives, VALIDATE);
    }
    StructKind::RequestBody | StructKind::Schema => match usage {
      TypeUsage::RequestOnly => {
        push_unique(&mut derives, SERIALIZE);
        push_unique(&mut derives, VALIDATE);
      }
      TypeUsage::ResponseOnly => {
        push_unique(&mut derives, DESERIALIZE);
      }
      TypeUsage::Bidirectional => {
        push_unique(&mut derives, SERIALIZE);
        push_unique(&mut derives, DESERIALIZE);
        push_unique(&mut derives, VALIDATE);
      }
    },
  }

  def.derives = derives;

  if matches!(usage, TypeUsage::ResponseOnly) {
    strip_validation_attrs(&mut def.fields);
  }

  adjust_skip_serializing_none(def, matches!(usage, TypeUsage::RequestOnly | TypeUsage::Bidirectional));
}

fn base_struct_derives(kind: StructKind) -> Vec<String> {
  match kind {
    StructKind::Schema | StructKind::RequestBody => vec![
      "Debug".to_string(),
      "Clone".to_string(),
      "PartialEq".to_string(),
      "oas3_gen_support::Default".to_string(),
    ],
    StructKind::OperationRequest => vec![
      "Debug".to_string(),
      "Clone".to_string(),
      "oas3_gen_support::Default".to_string(),
    ],
  }
}

fn push_unique(target: &mut Vec<String>, derive: &str) {
  if !target.iter().any(|existing| existing == derive) {
    target.push(derive.to_string());
  }
}

fn apply_enum_usage_policy(def: &mut EnumDef, type_usage: &BTreeMap<String, TypeUsage>) {
  let usage = type_usage.get(&def.name).copied().unwrap_or(TypeUsage::Bidirectional);
  let mut derives = def
    .derives
    .iter()
    .filter(|d| d.as_str() != SERIALIZE && d.as_str() != DESERIALIZE)
    .cloned()
    .collect::<Vec<_>>();

  match usage {
    TypeUsage::RequestOnly => {
      push_unique(&mut derives, SERIALIZE);
    }
    TypeUsage::ResponseOnly => {
      push_unique(&mut derives, DESERIALIZE);
    }
    TypeUsage::Bidirectional => {
      push_unique(&mut derives, SERIALIZE);
      push_unique(&mut derives, DESERIALIZE);
    }
  }

  def.derives = derives;
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

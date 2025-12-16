use std::collections::{BTreeMap, BTreeSet};

use super::type_usage::TypeUsage;
use crate::generator::ast::{
  ContentCategory, DiscriminatedEnumDef, EnumDef, EnumToken, FieldDef, OperationInfo, OuterAttr, RustType, SerdeMode,
  StatusCodeToken, StructDef, StructKind, StructMethodKind, TypeRef,
};

pub(crate) fn update_derives_from_usage(rust_types: &mut [RustType], type_usage: &BTreeMap<EnumToken, TypeUsage>) {
  for rust_type in rust_types {
    match rust_type {
      RustType::Struct(def) => process_struct(def, type_usage),
      RustType::Enum(def) => process_enum(def, type_usage),
      RustType::DiscriminatedEnum(def) => process_discriminated_enum(def, type_usage),
      _ => {}
    }
  }
}

fn process_struct(def: &mut StructDef, type_usage: &BTreeMap<EnumToken, TypeUsage>) {
  let key: EnumToken = def.name.as_str().into();
  let usage = get_usage(&key, type_usage);

  def.serde_mode = usage_to_serde_mode(usage);

  if usage == TypeUsage::ResponseOnly {
    strip_validation_attrs(&mut def.fields);
  }

  let needs_serialization = matches!(usage, TypeUsage::RequestOnly | TypeUsage::Bidirectional);
  adjust_skip_serializing_none(def, needs_serialization);
}

fn process_enum(def: &mut EnumDef, type_usage: &BTreeMap<EnumToken, TypeUsage>) {
  let usage = get_usage(&def.name, type_usage);
  def.serde_mode = usage_to_serde_mode(usage);
}

fn process_discriminated_enum(def: &mut DiscriminatedEnumDef, type_usage: &BTreeMap<EnumToken, TypeUsage>) {
  let usage = get_usage(&def.name, type_usage);
  def.serde_mode = usage_to_serde_mode(usage);
}

fn get_usage(name: &EnumToken, map: &BTreeMap<EnumToken, TypeUsage>) -> TypeUsage {
  map.get(name).copied().unwrap_or(TypeUsage::Bidirectional)
}

fn usage_to_serde_mode(usage: TypeUsage) -> SerdeMode {
  match usage {
    TypeUsage::RequestOnly => SerdeMode::SerializeOnly,
    TypeUsage::ResponseOnly => SerdeMode::DeserializeOnly,
    TypeUsage::Bidirectional => SerdeMode::Both,
  }
}

fn strip_validation_attrs(fields: &mut [FieldDef]) {
  for field in fields {
    field.validation_attrs.clear();
  }
}

fn adjust_skip_serializing_none(def: &mut StructDef, needs_serialization: bool) {
  def.outer_attrs.retain(|attr| *attr != OuterAttr::SkipSerializingNone);

  if needs_serialization && has_nullable_fields(&def.fields) && def.kind != StructKind::OperationRequest {
    def.outer_attrs.push(OuterAttr::SkipSerializingNone);
  }
}

fn has_nullable_fields(fields: &[FieldDef]) -> bool {
  fields.iter().any(|field| field.rust_type.nullable)
}

type ResponseEnumSignature = Vec<(StatusCodeToken, String, String, ContentCategory)>;

struct DuplicateCandidate {
  index: usize,
  name: String,
}

pub(crate) fn deduplicate_response_enums(rust_types: &mut Vec<RustType>, operations_info: &mut [OperationInfo]) {
  let mut signature_map: BTreeMap<ResponseEnumSignature, Vec<DuplicateCandidate>> = BTreeMap::new();

  for (i, rt) in rust_types.iter().enumerate() {
    if let RustType::ResponseEnum(def) = rt {
      let mut signature: Vec<_> = def
        .variants
        .iter()
        .map(|v| {
          (
            v.status_code,
            v.variant_name.to_string(),
            v.schema_type
              .as_ref()
              .map_or_else(|| "None".to_string(), TypeRef::to_rust_type),
            v.content_category,
          )
        })
        .collect();

      signature.sort();

      signature_map.entry(signature).or_default().push(DuplicateCandidate {
        index: i,
        name: def.name.to_string(),
      });
    }
  }

  let mut replacements: BTreeMap<String, String> = BTreeMap::new();
  let mut indices_to_remove = BTreeSet::new();

  for group in signature_map.values() {
    if group.len() > 1 {
      let canonical = group
        .iter()
        .min_by(|a, b| a.name.len().cmp(&b.name.len()).then(a.name.cmp(&b.name)))
        .unwrap();

      for candidate in group {
        if candidate.name != canonical.name {
          replacements.insert(candidate.name.clone(), canonical.name.clone());
          indices_to_remove.insert(candidate.index);
        }
      }
    }
  }

  if replacements.is_empty() {
    return;
  }

  for &idx in indices_to_remove.iter().rev() {
    rust_types.remove(idx);
  }

  for op in operations_info.iter_mut() {
    if let Some(ref current_enum) = op.response_enum
      && let Some(new_name) = replacements.get(&current_enum.to_string())
    {
      op.response_enum = Some(EnumToken::new(new_name));
    }
  }

  for rt in rust_types.iter_mut() {
    if let RustType::Struct(def) = rt {
      for method in &mut def.methods {
        if let StructMethodKind::ParseResponse { response_enum, .. } = &mut method.kind
          && let Some(new_name) = replacements.get(&response_enum.to_string())
        {
          *response_enum = EnumToken::new(new_name);
        }
      }
    }
  }
}

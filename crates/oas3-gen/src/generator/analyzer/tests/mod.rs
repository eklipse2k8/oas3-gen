mod error_tests;
mod transform_tests;
mod type_usage_tests;

use std::collections::BTreeMap;

use super::{DependencyGraph, TypeAnalyzer, TypeUsage};
use crate::generator::ast::{DerivesProvider, EnumToken, OuterAttr, RustType, SerdeImpl, StructKind};

pub(super) fn build_type_usage_map(
  seed_usage: BTreeMap<EnumToken, (bool, bool)>,
  types: &[RustType],
) -> BTreeMap<EnumToken, TypeUsage> {
  let dep_graph = DependencyGraph::build(types);
  TypeAnalyzer::build_usage_map(seed_usage, types, &dep_graph)
}

pub(super) fn add_nested_validation_attrs(types: &mut Vec<RustType>) {
  let mut operations = vec![];
  let seed = BTreeMap::new();
  let mut analyzer = TypeAnalyzer::new(types, &mut operations, seed);
  analyzer.add_nested_validation_attrs();
}

pub(super) fn update_derives_from_usage(types: &mut [RustType], usage_map: &BTreeMap<EnumToken, TypeUsage>) {
  for rust_type in types.iter_mut() {
    match rust_type {
      RustType::Struct(def) => {
        let key: EnumToken = def.name.as_str().into();
        let usage = usage_map.get(&key).copied().unwrap_or(TypeUsage::Bidirectional);

        def.serde_mode = usage.to_serde_mode();

        if usage == TypeUsage::ResponseOnly {
          for field in &mut def.fields {
            field.validation_attrs.clear();
          }
        }

        def.outer_attrs.retain(|attr| *attr != OuterAttr::SkipSerializingNone);
        let derives_serialize = def.is_serializable() == SerdeImpl::Derive;
        let has_nullable = def.fields.iter().any(|f| f.rust_type.nullable);
        if derives_serialize && has_nullable && def.kind != StructKind::OperationRequest {
          def.outer_attrs.push(OuterAttr::SkipSerializingNone);
        }
      }
      RustType::Enum(def) => {
        let usage = usage_map.get(&def.name).copied().unwrap_or(TypeUsage::Bidirectional);
        def.serde_mode = usage.to_serde_mode();
      }
      RustType::DiscriminatedEnum(def) => {
        let usage = usage_map.get(&def.name).copied().unwrap_or(TypeUsage::Bidirectional);
        def.serde_mode = usage.to_serde_mode();
      }
      _ => {}
    }
  }
}

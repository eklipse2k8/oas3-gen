mod transform_tests;
mod type_usage_tests;

use std::collections::BTreeMap;

use super::{DependencyGraph, TypePostprocessor, TypeUsage};
use crate::generator::{
  ast::{EnumToken, RustType},
  converter::GenerationTarget,
};

pub(super) fn build_type_usage_map(
  seed_usage: BTreeMap<EnumToken, (bool, bool)>,
  types: &[RustType],
) -> BTreeMap<EnumToken, TypeUsage> {
  let dep_graph = DependencyGraph::build(types);
  TypePostprocessor::build_usage_map(seed_usage, types, &dep_graph)
}

pub(super) fn postprocess_types_with_usage(
  types: Vec<RustType>,
  usage_seeds: BTreeMap<EnumToken, (bool, bool)>,
) -> Vec<RustType> {
  postprocess_types_with_target(types, usage_seeds, GenerationTarget::default())
}

pub(super) fn postprocess_types_for_server(
  types: Vec<RustType>,
  usage_seeds: BTreeMap<EnumToken, (bool, bool)>,
) -> Vec<RustType> {
  postprocess_types_with_target(types, usage_seeds, GenerationTarget::Server)
}

fn postprocess_types_with_target(
  types: Vec<RustType>,
  usage_seeds: BTreeMap<EnumToken, (bool, bool)>,
  target: GenerationTarget,
) -> Vec<RustType> {
  let postprocessor = TypePostprocessor::new(types, vec![], usage_seeds, target);
  postprocessor.postprocess().types
}

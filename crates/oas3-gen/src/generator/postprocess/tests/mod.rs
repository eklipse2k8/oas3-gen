mod transform_tests;
mod type_usage_tests;

use std::collections::BTreeMap;

use crate::generator::{
  ast::{EnumToken, RustType},
  converter::GenerationTarget,
  postprocess::{
    PostprocessOutput,
    serde_usage::{SerdeUsage, TypeUsage},
  },
};

pub(super) fn build_type_usage_map(
  seed_usage: BTreeMap<EnumToken, (bool, bool)>,
  types: &[RustType],
) -> BTreeMap<EnumToken, TypeUsage> {
  let mut serde = SerdeUsage::new(types, seed_usage, GenerationTarget::Client);
  serde.propagate();

  serde
    .usage
    .into_iter()
    .map(|(k, (req, resp))| (k, TypeUsage::from_flags(req, resp)))
    .collect::<BTreeMap<_, _>>()
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
  PostprocessOutput::new(types, vec![], usage_seeds, target).types
}

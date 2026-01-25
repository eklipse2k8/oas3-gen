mod response_enum;
mod serde_usage;
mod uses;
mod validation;

#[cfg(test)]
mod tests;

use std::collections::BTreeSet;

use crate::generator::{
  ast::{EnumToken, OperationInfo, RustType, constants::HttpHeaderRef},
  converter::GenerationTarget,
  postprocess::{
    response_enum::ResponseEnumDeduplicator, serde_usage::UsagePropagator, uses::OutputAssembler,
    validation::NestedValidationProcessor,
  },
};

pub struct PostprocessOutput {
  pub types: Vec<RustType>,
  pub operations: Vec<OperationInfo>,
  pub header_refs: Vec<HttpHeaderRef>,
  pub uses: BTreeSet<String>,
}

pub(crate) fn postprocess(
  types: Vec<RustType>,
  operations: Vec<OperationInfo>,
  seed_usage: std::collections::BTreeMap<EnumToken, (bool, bool)>,
  target: GenerationTarget,
) -> PostprocessOutput {
  let (mut types, operations) = ResponseEnumDeduplicator::new(types, operations).process();

  NestedValidationProcessor::new(&types).process(&mut types);

  UsagePropagator::new(&types, seed_usage, target).propagate(&mut types);

  let assembled = OutputAssembler::new(types, target).assemble();

  PostprocessOutput {
    types: assembled.types,
    operations,
    header_refs: assembled.header_refs,
    uses: assembled.uses,
  }
}

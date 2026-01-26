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
    response_enum::ResponseEnumDeduplicator,
    serde_usage::SerdeUsage,
    uses::{HeaderRefCollection, ModuleImports, RustTypeDeduplication},
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

  SerdeUsage::new(&types, seed_usage, target).apply(&mut types);

  let dedup_output = RustTypeDeduplication::new(types).process();
  let header_output = HeaderRefCollection::new(dedup_output.clone()).process();
  let uses_output = ModuleImports::new(dedup_output.clone(), target).process();

  PostprocessOutput {
    types: dedup_output,
    operations,
    header_refs: header_output,
    uses: uses_output,
  }
}

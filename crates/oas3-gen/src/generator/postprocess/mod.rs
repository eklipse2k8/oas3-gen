mod defaults;
mod response;
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
    defaults::FieldDefaultProcessor,
    response::ResponseProcessor,
    serde_usage::SerdeUsage,
    uses::{ModuleImports, RustTypeDeduplication},
    validation::NestedValidationProcessor,
  },
};

#[derive(Debug, Clone, Default)]
pub struct PostprocessOutput {
  pub types: Vec<RustType>,
  pub operations: Vec<OperationInfo>,
  pub header_refs: Vec<HttpHeaderRef>,
  pub uses: BTreeSet<String>,
}

impl PostprocessOutput {
  pub(crate) fn new(
    types: Vec<RustType>,
    operations: Vec<OperationInfo>,
    seed_usage: std::collections::BTreeMap<EnumToken, (bool, bool)>,
    target: GenerationTarget,
    header_refs: Vec<HttpHeaderRef>,
  ) -> Self {
    let (mut types, operations) = ResponseProcessor::new(types, operations, target).process();

    NestedValidationProcessor::new(&types).process(&mut types);

    SerdeUsage::new(&types, seed_usage, target).apply(&mut types);

    let mut dedup_output = RustTypeDeduplication::new(types).process();
    FieldDefaultProcessor::process(&mut dedup_output);
    let uses_output = ModuleImports::new(&dedup_output).process();

    Self {
      types: dedup_output,
      operations,
      header_refs,
      uses: uses_output,
    }
  }
}

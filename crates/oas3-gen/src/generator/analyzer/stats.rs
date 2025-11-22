use std::collections::BTreeSet;

use crate::generator::ast::{OperationInfo, ParameterLocation};

pub(crate) fn count_unique_headers(operations: &[OperationInfo]) -> usize {
  operations
    .iter()
    .flat_map(|op| &op.parameters)
    .filter(|param| matches!(param.location, ParameterLocation::Header))
    .map(|param| param.original_name.to_ascii_lowercase())
    .collect::<BTreeSet<_>>()
    .len()
}

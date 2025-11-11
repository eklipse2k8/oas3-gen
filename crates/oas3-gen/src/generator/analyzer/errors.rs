use std::collections::HashSet;

use super::dependency_graph::DependencyGraph;
use crate::generator::ast::{OperationInfo, RustType};

pub(crate) struct ErrorAnalyzer;

impl ErrorAnalyzer {
  pub(crate) fn build_error_schema_set(operations_info: &[OperationInfo], rust_types: &[RustType]) -> HashSet<String> {
    let mut error_schemas = HashSet::new();
    let mut success_schemas = HashSet::new();

    for op_info in operations_info {
      for schema in &op_info.error_response_types {
        error_schemas.insert(schema.clone());
      }
      for schema in &op_info.success_response_types {
        success_schemas.insert(schema.clone());
      }
    }

    let root_errors: HashSet<String> = error_schemas
      .into_iter()
      .filter(|schema| !success_schemas.contains(schema))
      .collect();

    Self::expand_error_types(&root_errors, rust_types, &success_schemas)
  }

  fn expand_error_types(
    roots: &HashSet<String>,
    rust_types: &[RustType],
    success_schemas: &HashSet<String>,
  ) -> HashSet<String> {
    let dep_graph = DependencyGraph::build(rust_types);

    let mut result = roots.clone();
    let mut queue: Vec<String> = roots.iter().cloned().collect();
    let mut visited = HashSet::new();

    while let Some(type_name) = queue.pop() {
      if !visited.insert(type_name.clone()) {
        continue;
      }

      if let Some(deps) = dep_graph.get_dependencies(&type_name) {
        for nested_type in deps {
          if !success_schemas.contains(nested_type) && result.insert(nested_type.clone()) {
            queue.push(nested_type.clone());
          }
        }
      }
    }

    result
  }
}

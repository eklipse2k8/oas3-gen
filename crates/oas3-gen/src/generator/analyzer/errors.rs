use std::collections::{HashMap, HashSet};

use crate::generator::ast::{OperationInfo, RustPrimitive, RustType, VariantContent};

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
    let type_map: HashMap<&str, &RustType> = rust_types.iter().map(|t| (t.type_name(), t)).collect();

    let mut result = roots.clone();
    let mut queue: Vec<&str> = roots.iter().map(String::as_str).collect();
    let mut visited = HashSet::new();

    while let Some(type_name) = queue.pop() {
      if !visited.insert(type_name) {
        continue;
      }

      let Some(&rust_type) = type_map.get(type_name) else {
        continue;
      };

      match rust_type {
        RustType::Struct(def) => {
          for field in &def.fields {
            if let RustPrimitive::Custom(nested_type) = &field.rust_type.base_type
              && !success_schemas.contains(nested_type)
              && result.insert(nested_type.clone())
            {
              queue.push(nested_type);
            }
          }
        }
        RustType::Enum(def) => {
          for variant in &def.variants {
            if let VariantContent::Tuple(types) = &variant.content {
              for type_ref in types {
                if let RustPrimitive::Custom(nested_type) = &type_ref.base_type
                  && !success_schemas.contains(nested_type)
                  && result.insert(nested_type.clone())
                {
                  queue.push(nested_type);
                }
              }
            }
          }
        }
        _ => {}
      }
    }

    result
  }
}

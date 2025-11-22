use std::collections::HashSet;

use http::Method;
use indexmap::IndexMap;
use oas3::Spec;

use crate::reserved::to_rust_field_name;

#[derive(Debug, Clone)]
pub struct OperationLocation {
  pub method: Method,
  pub path: String,
}

#[derive(Debug)]
pub struct OperationRegistry {
  id_to_location: IndexMap<String, OperationLocation>,
  spec: Spec,
}

impl OperationRegistry {
  pub fn from_spec(spec: &Spec) -> Self {
    Self::from_spec_filtered(spec, None, None)
  }

  pub fn from_spec_filtered(
    spec: &Spec,
    only_operations: Option<&HashSet<String>>,
    excluded_operations: Option<&HashSet<String>>,
  ) -> Self {
    let mut id_to_location = IndexMap::new();

    for (path, method, operation) in spec.operations() {
      let stable_id = compute_stable_id(method.as_str(), &path, operation);

      if let Some(included) = only_operations
        && !included.contains(&stable_id)
      {
        continue;
      }

      if let Some(excluded) = excluded_operations
        && excluded.contains(&stable_id)
      {
        continue;
      }

      let location = OperationLocation {
        method: method.clone(),
        path,
      };
      id_to_location.insert(stable_id, location);
    }

    Self {
      id_to_location,
      spec: spec.clone(),
    }
  }

  pub fn operations(&self) -> impl Iterator<Item = (&str, &OperationLocation)> {
    self.id_to_location.iter().map(|(id, loc)| (id.as_str(), loc))
  }

  pub fn operations_with_details(&self) -> impl Iterator<Item = (&str, &Method, &str, &oas3::spec::Operation)> + '_ {
    self.id_to_location.iter().filter_map(|(stable_id, location)| {
      let paths = self.spec.paths.as_ref()?;
      let path_item = paths.get(&location.path)?;

      let operation = match location.method {
        Method::GET => path_item.get.as_ref(),
        Method::POST => path_item.post.as_ref(),
        Method::PUT => path_item.put.as_ref(),
        Method::DELETE => path_item.delete.as_ref(),
        Method::PATCH => path_item.patch.as_ref(),
        Method::OPTIONS => path_item.options.as_ref(),
        Method::HEAD => path_item.head.as_ref(),
        Method::TRACE => path_item.trace.as_ref(),
        _ => None,
      }?;

      Some((stable_id.as_str(), &location.method, location.path.as_str(), operation))
    })
  }

  #[cfg(test)]
  pub fn len(&self) -> usize {
    self.id_to_location.len()
  }

  #[cfg(test)]
  #[must_use]
  pub fn is_empty(&self) -> bool {
    self.id_to_location.is_empty()
  }
}

pub fn compute_stable_id(method: &str, path: &str, operation: &oas3::spec::Operation) -> String {
  let id = operation
    .operation_id
    .clone()
    .unwrap_or_else(|| generate_operation_id(method, path));
  to_rust_field_name(&id)
}

pub(crate) fn generate_operation_id(method: &str, path: &str) -> String {
  let path_parts: Vec<&str> = path
    .split('/')
    .filter(|s| !s.is_empty())
    .map(|s| {
      if s.starts_with('{') && s.ends_with('}') {
        "by_id"
      } else {
        s
      }
    })
    .collect();

  let method_lower = method.to_lowercase();
  if path_parts.is_empty() {
    method_lower
  } else {
    format!("{}_{}", method_lower, path_parts.join("_"))
  }
}

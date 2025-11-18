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
  pub fn count(&self) -> usize {
    self.id_to_location.len()
  }
}

pub fn compute_stable_id(method: &str, path: &str, operation: &oas3::spec::Operation) -> String {
  let id = operation
    .operation_id
    .clone()
    .unwrap_or_else(|| generate_operation_id(method, path));
  to_rust_field_name(&id)
}

fn generate_operation_id(method: &str, path: &str) -> String {
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

#[cfg(test)]
mod tests {
  use oas3::spec::Operation;

  use super::*;

  fn create_test_spec(operations: Vec<(&str, &str, Option<&str>)>) -> Spec {
    use std::{collections::HashMap, fmt::Write};

    let mut paths_map: HashMap<&str, Vec<(&str, Option<&str>)>> = HashMap::new();

    for (path, method, operation_id) in operations {
      paths_map.entry(path).or_default().push((method, operation_id));
    }

    let mut paths_json = String::from(r"{");
    let mut first_path = true;

    for (path, methods) in paths_map {
      if !first_path {
        paths_json.push(',');
      }
      first_path = false;

      write!(paths_json, r#""{path}": {{"#).unwrap();

      let mut first_method = true;
      for (method, operation_id) in methods {
        if !first_method {
          paths_json.push(',');
        }
        first_method = false;

        let op_id_json = operation_id.map_or_else(String::new, |id| format!(r#""operationId": "{id}""#));

        write!(paths_json, r#""{method}": {{ {op_id_json} }}"#).unwrap();
      }

      paths_json.push('}');
    }

    paths_json.push('}');

    let spec_json = format!(
      r#"{{
        "openapi": "3.1.0",
        "info": {{
          "title": "Test API",
          "version": "1.0.0"
        }},
        "paths": {paths_json}
      }}"#
    );

    oas3::from_json(&spec_json).expect("Failed to parse test spec")
  }

  #[test]
  fn test_compute_stable_id_with_operation_id() {
    let operation = Operation {
      operation_id: Some("getUserById".to_string()),
      ..Default::default()
    };

    let stable_id = compute_stable_id("GET", "/users/{id}", &operation);
    assert_eq!(stable_id, "get_user_by_id");
  }

  #[test]
  fn test_compute_stable_id_without_operation_id() {
    let operation = Operation {
      operation_id: None,
      ..Default::default()
    };

    let stable_id = compute_stable_id("GET", "/users/{id}", &operation);
    assert_eq!(stable_id, "get_users_by_id");
  }

  #[test]
  fn test_generate_operation_id_simple_path() {
    assert_eq!(generate_operation_id("GET", "/users"), "get_users");
    assert_eq!(generate_operation_id("POST", "/users"), "post_users");
    assert_eq!(generate_operation_id("DELETE", "/users"), "delete_users");
  }

  #[test]
  fn test_generate_operation_id_with_path_param() {
    assert_eq!(generate_operation_id("GET", "/users/{id}"), "get_users_by_id");
    assert_eq!(generate_operation_id("PUT", "/users/{userId}"), "put_users_by_id");
  }

  #[test]
  fn test_generate_operation_id_nested_path() {
    assert_eq!(
      generate_operation_id("GET", "/users/{id}/posts"),
      "get_users_by_id_posts"
    );
    assert_eq!(
      generate_operation_id("POST", "/organizations/{orgId}/members/{memberId}"),
      "post_organizations_by_id_members_by_id"
    );
  }

  #[test]
  fn test_generate_operation_id_root_path() {
    assert_eq!(generate_operation_id("GET", "/"), "get");
    assert_eq!(generate_operation_id("POST", "/"), "post");
  }

  #[test]
  fn test_generate_operation_id_with_trailing_slash() {
    assert_eq!(generate_operation_id("GET", "/users/"), "get_users");
  }

  #[test]
  fn test_registry_from_spec() {
    let spec = create_test_spec(vec![
      ("/users", "get", Some("listUsers")),
      ("/users/{id}", "get", None),
      ("/posts", "post", Some("createPost")),
    ]);

    let registry = OperationRegistry::from_spec(&spec);

    assert_eq!(registry.count(), 3);
  }

  #[test]
  fn test_registry_operations() {
    let spec = create_test_spec(vec![("/users", "get", Some("listUsers")), ("/users/{id}", "get", None)]);

    let registry = OperationRegistry::from_spec(&spec);
    let mut entries: Vec<_> = registry.operations().collect();
    entries.sort_by_key(|(id, _)| *id);

    assert_eq!(entries.len(), 2);

    let (id, location) = entries[0];
    assert_eq!(id, "get_users_by_id");
    assert_eq!(location.method, "GET");
    assert_eq!(location.path, "/users/{id}");

    let (id, location) = entries[1];
    assert_eq!(id, "list_users");
    assert_eq!(location.method, "GET");
    assert_eq!(location.path, "/users");
  }

  #[test]
  fn test_registry_operation_ids() {
    let spec = create_test_spec(vec![
      ("/users", "get", Some("listUsers")),
      ("/posts", "post", Some("createPost")),
    ]);

    let registry = OperationRegistry::from_spec(&spec);
    let mut ids: Vec<&str> = registry.operations().map(|(id, _)| id).collect();
    ids.sort_unstable();

    assert_eq!(ids, vec!["create_post", "list_users"]);
  }

  #[test]
  fn test_registry_uniqueness() {
    let spec = create_test_spec(vec![
      ("/users", "get", None),
      ("/users", "post", None),
      ("/users/{id}", "get", None),
      ("/users/{id}", "put", None),
      ("/users/{id}", "delete", None),
    ]);

    let registry = OperationRegistry::from_spec(&spec);

    assert_eq!(registry.count(), 5);

    let ids: Vec<&str> = registry.operations().map(|(id, _)| id).collect();
    let unique_count = ids.iter().collect::<std::collections::HashSet<_>>().len();
    assert_eq!(unique_count, 5, "All stable IDs should be unique");
  }

  #[test]
  fn test_compute_stable_id_with_special_characters() {
    let operation = Operation {
      operation_id: Some("user-profile.get".to_string()),
      ..Default::default()
    };

    let stable_id = compute_stable_id("GET", "/user-profile", &operation);
    assert_eq!(stable_id, "user_profile_get");
  }

  #[test]
  fn test_compute_stable_id_with_numbers() {
    let operation = Operation {
      operation_id: Some("v2GetUsers".to_string()),
      ..Default::default()
    };

    let stable_id = compute_stable_id("GET", "/v2/users", &operation);
    assert_eq!(stable_id, "v2get_users");
  }

  #[test]
  fn test_generate_operation_id_with_multiple_params() {
    assert_eq!(
      generate_operation_id("GET", "/users/{userId}/posts/{postId}/comments/{commentId}"),
      "get_users_by_id_posts_by_id_comments_by_id"
    );
  }

  #[test]
  fn test_empty_registry() {
    let spec_json = r#"{
      "openapi": "3.1.0",
      "info": {
        "title": "Empty API",
        "version": "1.0.0"
      },
      "paths": {}
    }"#;
    let spec: Spec = oas3::from_json(spec_json).unwrap();
    let registry = OperationRegistry::from_spec(&spec);

    assert_eq!(registry.count(), 0);
    assert_eq!(registry.operations().count(), 0);
  }

  #[test]
  fn test_registry_case_sensitivity() {
    let spec = create_test_spec(vec![
      ("/users", "get", Some("GetUsers")),
      ("/users", "post", Some("getUsers")),
    ]);

    let registry = OperationRegistry::from_spec(&spec);

    assert_eq!(registry.count(), 1, "Both operations should map to same stable_id");

    let operations: Vec<_> = registry.operations().collect();
    assert_eq!(operations.len(), 1);
    assert_eq!(operations[0].0, "get_users");
  }

  #[test]
  fn test_registry_filtered() {
    use std::collections::HashSet;

    let spec = create_test_spec(vec![
      ("/users", "get", Some("listUsers")),
      ("/users/{id}", "get", None),
      ("/posts", "post", Some("createPost")),
    ]);

    let mut excluded = HashSet::new();
    excluded.insert("list_users".to_string());

    let registry = OperationRegistry::from_spec_filtered(&spec, None, Some(&excluded));

    assert_eq!(registry.count(), 2);

    let ids: Vec<&str> = registry.operations().map(|(id, _)| id).collect();
    assert!(!ids.contains(&"list_users"));
    assert!(ids.contains(&"get_users_by_id"));
    assert!(ids.contains(&"create_post"));
  }

  #[test]
  fn test_registry_with_rust_keywords() {
    let operation = Operation {
      operation_id: Some("type".to_string()),
      ..Default::default()
    };

    let stable_id = compute_stable_id("GET", "/type", &operation);
    assert_eq!(stable_id, "r#type");
  }
}

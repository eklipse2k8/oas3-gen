use std::collections::HashSet;

use oas3::{Spec, spec::Operation};

use crate::generator::operation_registry::{OperationRegistry, compute_stable_id, generate_operation_id};

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
fn test_compute_stable_id() {
  // Test with operation_id
  let operation = Operation {
    operation_id: Some("getUserById".to_string()),
    ..Default::default()
  };
  assert_eq!(compute_stable_id("GET", "/users/{id}", &operation), "get_user_by_id");

  // Test without operation_id
  let operation = Operation {
    operation_id: None,
    ..Default::default()
  };
  assert_eq!(compute_stable_id("GET", "/users/{id}", &operation), "get_users_by_id");

  // Test with special characters
  let operation = Operation {
    operation_id: Some("user-profile.get".to_string()),
    ..Default::default()
  };
  assert_eq!(
    compute_stable_id("GET", "/user-profile", &operation),
    "user_profile_get"
  );

  // Test with numbers
  let operation = Operation {
    operation_id: Some("v2GetUsers".to_string()),
    ..Default::default()
  };
  assert_eq!(compute_stable_id("GET", "/v2/users", &operation), "v2get_users");

  // Test with Rust keywords
  let operation = Operation {
    operation_id: Some("type".to_string()),
    ..Default::default()
  };
  assert_eq!(compute_stable_id("GET", "/type", &operation), "r#type");
}

#[test]
fn test_generate_operation_id() {
  // Simple paths
  assert_eq!(generate_operation_id("GET", "/users"), "get_users");
  assert_eq!(generate_operation_id("POST", "/users"), "post_users");
  assert_eq!(generate_operation_id("DELETE", "/users"), "delete_users");

  // Path parameters
  assert_eq!(generate_operation_id("GET", "/users/{id}"), "get_users_by_id");
  assert_eq!(generate_operation_id("PUT", "/users/{userId}"), "put_users_by_id");

  // Nested paths
  assert_eq!(
    generate_operation_id("GET", "/users/{id}/posts"),
    "get_users_by_id_posts"
  );
  assert_eq!(
    generate_operation_id("POST", "/organizations/{orgId}/members/{memberId}"),
    "post_organizations_by_id_members_by_id"
  );

  // Root path
  assert_eq!(generate_operation_id("GET", "/"), "get");
  assert_eq!(generate_operation_id("POST", "/"), "post");

  // Trailing slash
  assert_eq!(generate_operation_id("GET", "/users/"), "get_users");

  // Multiple parameters
  assert_eq!(
    generate_operation_id("GET", "/users/{userId}/posts/{postId}/comments/{commentId}"),
    "get_users_by_id_posts_by_id_comments_by_id"
  );
}

#[test]
fn test_registry_operations() {
  let spec = create_test_spec(vec![
    ("/users", "get", Some("listUsers")),
    ("/users/{id}", "get", None),
    ("/posts", "post", Some("createPost")),
  ]);

  let registry = OperationRegistry::from_spec(&spec);

  assert_eq!(registry.len(), 3);

  let mut entries: Vec<_> = registry.operations().collect();
  entries.sort_by_key(|(id, _)| *id);

  let (id, location) = entries[1];
  assert_eq!(id, "get_users_by_id");
  assert_eq!(location.method, "GET");
  assert_eq!(location.path, "/users/{id}");

  let (id, location) = entries[2];
  assert_eq!(id, "list_users");
  assert_eq!(location.method, "GET");
  assert_eq!(location.path, "/users");

  let mut ids: Vec<&str> = registry.operations().map(|(id, _)| id).collect();
  ids.sort_unstable();
  assert_eq!(ids, vec!["create_post", "get_users_by_id", "list_users"]);
}

#[test]
fn test_registry_uniqueness_and_case_sensitivity() {
  // Uniqueness
  let spec = create_test_spec(vec![
    ("/users", "get", None),
    ("/users", "post", None),
    ("/users/{id}", "get", None),
    ("/users/{id}", "put", None),
    ("/users/{id}", "delete", None),
  ]);

  let registry = OperationRegistry::from_spec(&spec);
  assert_eq!(registry.len(), 5);
  let ids: Vec<&str> = registry.operations().map(|(id, _)| id).collect();
  let unique_count = ids.iter().collect::<HashSet<_>>().len();
  assert_eq!(unique_count, 5, "All stable IDs should be unique");

  // Case sensitivity
  let spec = create_test_spec(vec![
    ("/users", "get", Some("GetUsers")),
    ("/users", "post", Some("getUsers")),
  ]);

  let registry = OperationRegistry::from_spec(&spec);
  assert_eq!(registry.len(), 1, "Both operations should map to same stable_id");
  let operations: Vec<_> = registry.operations().collect();
  assert_eq!(operations.len(), 1);
  assert_eq!(operations[0].0, "get_users");
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

  assert_eq!(registry.len(), 0);
  assert!(registry.is_empty());
  assert_eq!(registry.operations().count(), 0);
}

#[test]
fn test_registry_filtered() {
  let spec = create_test_spec(vec![
    ("/users", "get", Some("listUsers")),
    ("/users/{id}", "get", None),
    ("/posts", "post", Some("createPost")),
  ]);

  let mut excluded = HashSet::new();
  excluded.insert("list_users".to_string());

  let registry = OperationRegistry::from_spec_filtered(&spec, None, Some(&excluded));

  assert_eq!(registry.len(), 2);

  let ids: Vec<&str> = registry.operations().map(|(id, _)| id).collect();
  assert!(!ids.contains(&"list_users"));
  assert!(ids.contains(&"get_users_by_id"));
  assert!(ids.contains(&"create_post"));
}

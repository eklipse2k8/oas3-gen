use std::collections::HashSet;

use http::Method;
use oas3::{Spec, spec::Operation};

use crate::generator::{
  ast::OperationKind,
  operation_registry::{OperationRegistry, compute_stable_id, generate_operation_id},
};

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
  let cases = [
    ("GET", "/users/{id}", Some("getUserById"), "get_user_by_id"),
    ("GET", "/users/{id}", None, "get_users_by_id"),
    ("GET", "/user-profile", Some("user-profile.get"), "user_profile_get"),
    ("GET", "/v2/users", Some("v2GetUsers"), "v2get_users"),
    ("GET", "/type", Some("type"), "r#type"),
  ];

  for (method, path, op_id, expected) in cases {
    let operation = Operation {
      operation_id: op_id.map(String::from),
      ..Default::default()
    };
    assert_eq!(
      compute_stable_id(method, path, &operation),
      expected,
      "failed for {method} {path} with operation_id={op_id:?}"
    );
  }
}

#[test]
fn test_generate_operation_id() {
  let cases = [
    ("GET", "/users", "get_users"),
    ("POST", "/users", "post_users"),
    ("DELETE", "/users", "delete_users"),
    ("GET", "/users/{id}", "get_users_by_id"),
    ("PUT", "/users/{userId}", "put_users_by_id"),
    ("GET", "/users/{id}/posts", "get_users_by_id_posts"),
    (
      "POST",
      "/organizations/{orgId}/members/{memberId}",
      "post_organizations_by_id_members_by_id",
    ),
    ("GET", "/", "get"),
    ("POST", "/", "post"),
    ("GET", "/users/", "get_users"),
    (
      "GET",
      "/users/{userId}/posts/{postId}/comments/{commentId}",
      "get_users_by_id_posts_by_id_comments_by_id",
    ),
  ];

  for (method, path, expected) in cases {
    assert_eq!(
      generate_operation_id(method, path),
      expected,
      "failed for {method} {path}"
    );
  }
}

#[test]
fn test_operation_registry() {
  // Basic operations
  {
    let spec = create_test_spec(vec![
      ("/users", "get", Some("listUsers")),
      ("/users/{id}", "get", None),
      ("/posts", "post", Some("createPost")),
    ]);

    let registry = OperationRegistry::from_spec(&spec);
    assert_eq!(registry.len(), 3, "expected 3 operations");

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

  // Uniqueness
  {
    let spec = create_test_spec(vec![
      ("/users", "get", None),
      ("/users", "post", None),
      ("/users/{id}", "get", None),
      ("/users/{id}", "put", None),
      ("/users/{id}", "delete", None),
    ]);

    let registry = OperationRegistry::from_spec(&spec);
    assert_eq!(registry.len(), 5, "expected 5 operations for uniqueness test");
    let ids: Vec<&str> = registry.operations().map(|(id, _)| id).collect();
    let unique_count = ids.iter().collect::<HashSet<_>>().len();
    assert_eq!(unique_count, 5, "all stable IDs should be unique");
  }

  // Case sensitivity (both map to same stable_id)
  {
    let spec = create_test_spec(vec![
      ("/users", "get", Some("GetUsers")),
      ("/users", "post", Some("getUsers")),
    ]);

    let registry = OperationRegistry::from_spec(&spec);
    assert_eq!(registry.len(), 1, "both operations should map to same stable_id");
    let operations: Vec<_> = registry.operations().collect();
    assert_eq!(operations[0].0, "get_users");
  }

  // Empty registry
  {
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

  // Filtered registry
  {
    let spec = create_test_spec(vec![
      ("/users", "get", Some("listUsers")),
      ("/users/{id}", "get", None),
      ("/posts", "post", Some("createPost")),
    ]);

    let mut excluded = HashSet::new();
    excluded.insert("list_users".to_string());

    let registry = OperationRegistry::from_spec_filtered(&spec, None, Some(&excluded));

    assert_eq!(registry.len(), 2, "filtered registry should have 2 operations");

    let ids: Vec<&str> = registry.operations().map(|(id, _)| id).collect();
    assert!(!ids.contains(&"list_users"), "list_users should be excluded");
    assert!(ids.contains(&"get_users_by_id"), "get_users_by_id should be included");
    assert!(ids.contains(&"create_post"), "create_post should be included");
  }

  // Webhook operations
  {
    let spec_json = r#"{
      "openapi": "3.1.0",
      "info": {"title": "Webhook API", "version": "1.0.0"},
      "paths": {},
      "webhooks": {
        "petAdded": {
          "post": {
            "operationId": "petAddedHook",
            "responses": {"200": {"description": "ok"}}
          }
        }
      }
    }"#;

    let spec: Spec = oas3::from_json(spec_json).unwrap();
    let registry = OperationRegistry::from_spec(&spec);

    assert_eq!(registry.len(), 1);

    let (id, location) = registry.operations().next().unwrap();
    assert_eq!(id, "pet_added_hook");
    assert_eq!(location.path, "webhooks/petAdded");
    assert_eq!(location.lookup_path, "petAdded");
    assert_eq!(location.kind, OperationKind::Webhook);

    let mut details = registry.operations_with_details();
    let (_, method, path, _, kind) = details.next().unwrap();
    assert_eq!(method, &Method::POST);
    assert_eq!(path, "webhooks/petAdded");
    assert_eq!(kind, OperationKind::Webhook);
  }
}

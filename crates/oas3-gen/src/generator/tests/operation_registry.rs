use std::collections::HashSet;

use http::Method;
use oas3::Spec;
use serde_json::{Map, Value, json};

use super::support::parse_spec;
use crate::generator::{
  ast::OperationKind,
  naming::operations::{compute_stable_id, generate_operation_id},
  operation_registry::OperationRegistry,
};

type TestOperation<'a> = (&'a str, &'a str, Option<&'a str>);

fn create_test_spec(operations: &[TestOperation<'_>]) -> Spec {
  let mut paths = Map::new();
  for (path, method, operation_id) in operations {
    let path_entry = paths
      .entry((*path).to_string())
      .or_insert_with(|| Value::Object(Map::new()));
    let Value::Object(path_methods) = path_entry else {
      panic!("path entry should be a JSON object");
    };

    let mut operation = Map::new();
    if let Some(operation_id) = operation_id {
      operation.insert("operationId".to_string(), Value::String((*operation_id).to_string()));
    }

    path_methods.insert((*method).to_string(), Value::Object(operation));
  }

  let spec_json = json!({
    "openapi": "3.1.0",
    "info": {
      "title": "Test API",
      "version": "1.0.0"
    },
    "paths": Value::Object(paths)
  });
  parse_spec(&spec_json.to_string())
}

fn sorted_stable_ids(registry: &OperationRegistry) -> Vec<String> {
  let mut ids = registry
    .operations()
    .map(|entry| entry.stable_id.clone())
    .collect::<Vec<String>>();
  ids.sort_unstable();
  ids
}

fn assert_stable_ids(registry: &OperationRegistry, expected_ids: &[&str], context: &str) {
  let actual = sorted_stable_ids(registry);
  let expected = expected_ids.iter().map(|id| (*id).to_string()).collect::<Vec<_>>();
  assert_eq!(actual, expected, "{context}");
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
    assert_eq!(
      compute_stable_id(method, path, op_id),
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
fn test_operation_registry_collects_operations_and_metadata() {
  let spec = create_test_spec(&[
    ("/users", "get", Some("listUsers")),
    ("/users/{id}", "get", None),
    ("/posts", "post", Some("createPost")),
  ]);

  let registry = OperationRegistry::new(&spec);
  assert_eq!(registry.entries.len(), 3, "expected 3 operations");

  let mut entries = registry.operations().collect::<Vec<_>>();
  entries.sort_by(|a, b| a.stable_id.cmp(&b.stable_id));

  let by_id_entry = &entries[1];
  assert_eq!(by_id_entry.stable_id, "get_users_by_id");
  assert_eq!(by_id_entry.method, Method::GET);
  assert_eq!(by_id_entry.path, "/users/{id}");

  let list_entry = &entries[2];
  assert_eq!(list_entry.stable_id, "list_users");
  assert_eq!(list_entry.method, Method::GET);
  assert_eq!(list_entry.path, "/users");

  assert_stable_ids(
    &registry,
    &["create_post", "get_users_by_id", "list_users"],
    "stable IDs should match expected set",
  );
}

#[test]
fn test_operation_registry_generates_unique_ids_without_operation_ids() {
  let spec = create_test_spec(&[
    ("/users", "get", None),
    ("/users", "post", None),
    ("/users/{id}", "get", None),
    ("/users/{id}", "put", None),
    ("/users/{id}", "delete", None),
  ]);

  let registry = OperationRegistry::new(&spec);
  assert_eq!(registry.entries.len(), 5, "expected 5 operations");

  let ids = sorted_stable_ids(&registry);
  let unique_count = ids.iter().collect::<HashSet<_>>().len();
  assert_eq!(unique_count, 5, "all stable IDs should be unique");
}

#[test]
fn test_operation_registry_resolves_conflicting_operation_ids() {
  let spec = create_test_spec(&[
    ("/users", "get", Some("GetUsers")),
    ("/users", "post", Some("getUsers")),
  ]);

  let registry = OperationRegistry::new(&spec);
  assert_eq!(
    registry.entries.len(),
    2,
    "both operations should be included with unique ids",
  );
  assert_stable_ids(
    &registry,
    &["users", "users_2"],
    "common prefix 'get' should be stripped while preserving uniqueness",
  );
}

#[test]
fn test_operation_registry_handles_empty_paths() {
  let spec = parse_spec(
    r#"{
      "openapi": "3.1.0",
      "info": {
        "title": "Empty API",
        "version": "1.0.0"
      },
      "paths": {}
    }"#,
  );

  let registry = OperationRegistry::new(&spec);
  assert_eq!(registry.entries.len(), 0);
  assert!(registry.entries.is_empty());
  assert_eq!(registry.operations().count(), 0);
}

#[test]
fn test_operation_registry_applies_exclude_filters() {
  let spec = create_test_spec(&[
    ("/users", "get", Some("listUsers")),
    ("/users/{id}", "get", None),
    ("/posts", "post", Some("createPost")),
  ]);
  let mut excluded = HashSet::new();
  excluded.insert("list_users".to_string());

  let registry = OperationRegistry::with_filters(&spec, None, Some(&excluded));
  assert_eq!(registry.entries.len(), 2, "filtered registry should have 2 operations");
  assert_stable_ids(
    &registry,
    &["create_post", "get_users_by_id"],
    "excluded operation should not be present",
  );
}

#[test]
fn test_operation_registry_includes_webhooks() {
  let spec = parse_spec(
    r#"{
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
    }"#,
  );

  let registry = OperationRegistry::new(&spec);
  assert_eq!(registry.entries.len(), 1);

  let entry = registry.operations().next().expect("webhook operation should exist");
  assert_eq!(entry.stable_id, "pet_added_hook");
  assert_eq!(entry.path, "webhooks/petAdded");
  assert_eq!(entry.kind, OperationKind::Webhook);
  assert_eq!(entry.method, Method::POST);
}

#[test]
fn test_operation_registry_strips_common_prefix_with_numeric_suffixes() {
  let spec = create_test_spec(&[
    ("/api/v1/users", "get", Some("listItems")),
    ("/api/v2/users", "get", Some("listItems")),
    ("/api/v3/users", "get", Some("listItems")),
  ]);

  let registry = OperationRegistry::new(&spec);
  assert_eq!(
    registry.entries.len(),
    3,
    "all operations should be registered with unique ids",
  );
  assert_stable_ids(
    &registry,
    &["items", "items_2", "items_3"],
    "common prefix 'list' should be stripped",
  );
}

#[test]
fn test_operation_registry_strips_common_prefix_for_verb_only_ids() {
  let spec = create_test_spec(&[
    (
      "/me/mail/folders/messages",
      "get",
      Some("me_mail_folders_messages_list"),
    ),
    (
      "/me/mail/folders/messages/{id}",
      "get",
      Some("me_mail_folders_messages_get"),
    ),
    (
      "/me/mail/folders/messages/{id}",
      "delete",
      Some("me_mail_folders_messages_delete"),
    ),
  ]);

  let registry = OperationRegistry::new(&spec);
  assert_eq!(registry.entries.len(), 3);
  assert_stable_ids(
    &registry,
    &["delete", "get", "list"],
    "common prefix should be stripped",
  );
}

#[test]
fn test_operation_registry_strips_common_suffix() {
  let spec = create_test_spec(&[
    ("/users", "get", Some("list_users_request")),
    ("/users", "post", Some("create_users_request")),
    ("/users/{id}", "delete", Some("delete_users_request")),
  ]);

  let registry = OperationRegistry::new(&spec);
  assert_eq!(registry.entries.len(), 3);
  assert_stable_ids(
    &registry,
    &["create", "delete", "list"],
    "common suffix should be stripped",
  );
}

#[test]
fn test_operation_registry_retains_middle_segment_after_affix_stripping() {
  let spec = create_test_spec(&[
    ("/users", "get", Some("api_users_list")),
    ("/posts", "get", Some("api_posts_list")),
  ]);

  let registry = OperationRegistry::new(&spec);
  assert_eq!(registry.entries.len(), 2);
  assert_stable_ids(
    &registry,
    &["posts", "users"],
    "middle segment should remain when prefix and suffix are stripped",
  );
}

#[test]
fn test_operation_registry_does_not_simplify_when_no_common_affixes() {
  let spec = create_test_spec(&[
    ("/users", "get", Some("listUsers")),
    ("/posts", "post", Some("createPost")),
  ]);

  let registry = OperationRegistry::new(&spec);
  assert_eq!(registry.entries.len(), 2);
  assert_stable_ids(
    &registry,
    &["create_post", "list_users"],
    "IDs should remain unchanged when there are no common affixes",
  );
}

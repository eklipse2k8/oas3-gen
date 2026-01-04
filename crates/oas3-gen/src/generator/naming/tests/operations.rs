use crate::generator::naming::operations::trim_common_affixes;

#[test]
fn test_simplify_common_prefix() {
  let ids = ["me_mail_folders_list", "me_mail_folders_get", "me_mail_folders_delete"];
  assert_eq!(trim_common_affixes(&ids), ["list", "get", "delete"]);
}

#[test]
fn test_simplify_common_suffix() {
  let ids = ["list_users_request", "create_users_request", "delete_users_request"];
  assert_eq!(trim_common_affixes(&ids), ["list", "create", "delete"]);
}

#[test]
fn test_simplify_common_prefix_and_suffix() {
  let ids = [
    "api_users_list_request",
    "api_users_create_request",
    "api_users_delete_request",
  ];
  assert_eq!(trim_common_affixes(&ids), ["list", "create", "delete"]);
}

#[test]
fn test_no_simplification_when_nothing_common() {
  let ids = ["list_users", "create_posts", "delete_comments"];
  assert_eq!(trim_common_affixes(&ids), ids);
}

#[test]
fn test_no_simplification_when_would_create_empty() {
  let ids = ["users", "users_list"];
  assert_eq!(trim_common_affixes(&ids), ids);
}

#[test]
fn test_no_simplification_when_would_create_duplicates() {
  let ids = ["api_get_users", "api_get_posts"];
  assert_eq!(trim_common_affixes(&ids), ["users", "posts"]);
}

#[test]
fn test_single_operation_not_simplified() {
  let ids = ["me_mail_folders_list"];
  assert_eq!(trim_common_affixes(&ids), ids);
}

#[test]
fn test_empty_slice_returns_empty() {
  let ids: [&str; 0] = [];
  assert!(trim_common_affixes(&ids).is_empty());
}

#[test]
fn test_preserves_order_after_simplification() {
  let ids = [
    "api_users_create",
    "api_users_read",
    "api_users_update",
    "api_users_delete",
  ];
  assert_eq!(trim_common_affixes(&ids), ["create", "read", "update", "delete"]);
}

#[test]
fn test_simplify_with_deduplicated_suffix() {
  let ids = ["get_message", "get_message_2"];
  assert_eq!(trim_common_affixes(&ids), ["message", "message_2"]);
}

#[test]
fn test_simplify_reduces_prefix_for_shortest_name() {
  let ids = ["api_v1_users_list", "api_v1_users"];
  assert_eq!(trim_common_affixes(&ids), ["users_list", "users"]);
}

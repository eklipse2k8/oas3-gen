use oas3_gen::generator::naming::operations::trim_common_affixes;
use rstest::rstest;

use super::assert_converts;

#[rstest]
fn operation_ids_drop_shared_leading_and_trailing_words() {
  assert_converts(
    |ids: &[&str]| trim_common_affixes(ids),
    &[
      (
        &["me_mail_folders_list", "me_mail_folders_get", "me_mail_folders_delete"][..],
        &["list", "get", "delete"][..],
      ),
      (
        &["list_users_request", "create_users_request", "delete_users_request"],
        &["list", "create", "delete"],
      ),
      (
        &[
          "api_users_create",
          "api_users_read",
          "api_users_update",
          "api_users_delete",
        ],
        &["create", "read", "update", "delete"],
      ),
      (
        &[
          "api_users_list_request",
          "api_users_create_request",
          "api_users_delete_request",
        ],
        &["list", "create", "delete"],
      ),
      (&["api_get_users", "api_get_posts"], &["users", "posts"]),
      (&["get_message", "get_message_2"], &["message", "message_2"]),
      (&["api_v1_users_list", "api_v1_users"], &["users_list", "users"]),
      (&["users", "users_list"], &["users", "users_list"]),
      (
        &["list_users", "create_posts", "delete_comments"],
        &["list_users", "create_posts", "delete_comments"],
      ),
      (&["me_mail_folders_list"], &["me_mail_folders_list"]),
      (&[], &[]),
    ],
  );
}

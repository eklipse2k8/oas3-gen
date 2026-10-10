use rstest::rstest;

use super::convert;

#[rstest]
fn parameters_group_by_location_in_declaration_order() {
  let converted = convert(spec!("converter/operations.json"), config!());

  assert_eq!(
    converted.fields("GetUserPostRequest"),
    [
      "path: GetUserPostRequestPath",
      "query: GetUserPostRequestQuery",
      "header: GetUserPostRequestHeader",
      "cookie: GetUserPostRequestCookie",
    ]
  );
  assert_eq!(
    converted.fields("GetUserPostRequestPath"),
    ["user_id: i64", "post_id: String", "slug: String"]
  );
  assert_eq!(
    converted.fields("GetUserPostRequestQuery"),
    ["page: Option<i64>", "limit: Option<i32>"]
  );
  assert_eq!(
    converted.fields("GetUserPostRequestHeader"),
    ["x_trace: Option<String>"]
  );

  let declared = converted
    .operation("get_user_post")
    .parameters
    .iter()
    .map(|parameter| parameter.name.as_str())
    .collect::<Vec<_>>();
  assert_eq!(
    declared,
    ["user_id", "theme", "x_trace", "post_id", "page", "limit", "slug"]
  );
}

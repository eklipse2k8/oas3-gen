use std::collections::{BTreeSet, HashMap, HashSet};

use indexmap::IndexMap;
use oas3_gen::generator::{ast::OperationKind, operation_registry::OperationRegistry};
use proptest::{
  collection::vec,
  option,
  prelude::*,
  sample::{select, subsequence},
};
use rstest::rstest;
use serde_json::{Map, Value, json};

use crate::{parse_spec, string_set};

const TRIMMED_IDS: [&str; 4] = ["get_users", "get_posts", "get_comments", "get_users_on_change"];
const ROUTES: [&str; 4] = ["/users", "/users/{id}", "/items/{itemId}/tags", "/"];
const METHODS: [&str; 4] = ["get", "put", "post", "delete"];
const OPERATION_IDS: [&str; 7] = [
  "getUsers",
  "GetUsers",
  "get-users",
  "type",
  "self",
  "listItems",
  "listItemsV2",
];

fn stable_ids(registry: &OperationRegistry) -> Vec<&str> {
  registry.operations().map(|entry| entry.stable_id.as_str()).collect()
}

fn id_subset() -> impl Strategy<Value = Option<Vec<&'static str>>> {
  option::of(subsequence(TRIMMED_IDS.to_vec(), 0..=TRIMMED_IDS.len()))
}

fn spec_with_operations(operations: &[(&str, &str, Option<&str>)]) -> serde_json::Result<oas3::Spec> {
  let mut paths = IndexMap::<&str, Map<String, Value>>::new();
  for (route, method, operation_id) in operations {
    let operation = operation_id.map_or_else(|| json!({}), |id| json!({ "operationId": id }));
    paths.entry(route).or_default().insert((*method).to_string(), operation);
  }
  super::skeleton_spec(json!(paths), json!({}))
}

#[rstest]
fn registers_http_operations_then_webhooks_then_callbacks_in_declaration_order() {
  let registry = OperationRegistry::new(&parse_spec(spec!("generator/registry_order.json")));
  let registered = registry
    .operations()
    .map(|entry| {
      (
        entry.kind,
        entry.method.as_str(),
        entry.path.as_str(),
        entry.stable_id.as_str(),
      )
    })
    .collect::<Vec<_>>();

  assert_eq!(
    registered,
    [
      (OperationKind::Http, "GET", "/users/{id}", "get_user_by_id"),
      (OperationKind::Http, "PUT", "/users/{id}", "put_users_by_id"),
      (OperationKind::Http, "GET", "/people", "get_people"),
      (OperationKind::Http, "POST", "/people", "get_people_2"),
      (OperationKind::Http, "GET", "/user-profile", "user_profile_get"),
      (OperationKind::Http, "GET", "/v2/users", "v2get_users"),
      (OperationKind::Http, "GET", "/type", "r#type"),
      (OperationKind::Http, "POST", "/", "post"),
      (
        OperationKind::Http,
        "DELETE",
        "/organizations/{orgId}/members/{memberId}/",
        "delete_organizations_by_id_members_by_id",
      ),
      (OperationKind::Http, "POST", "/subscriptions", "subscribe"),
      (OperationKind::Webhook, "POST", "webhooks/petAdded", "pet_added_hook"),
      (
        OperationKind::Callback,
        "PUT",
        "callbacks/subscribe/onEvent",
        "subscribe_on_event",
      ),
      (
        OperationKind::Callback,
        "POST",
        "callbacks/subscribe/onEvent",
        "subscribe_on_event_2",
      ),
      (
        OperationKind::Callback,
        "POST",
        "callbacks/subscribe/onRetry/0",
        "subscribe_on_retry",
      ),
      (
        OperationKind::Callback,
        "POST",
        "callbacks/subscribe/onRetry/1",
        "retry_backup",
      ),
    ]
  );
}

#[rstest]
#[case::callbacks_keep_their_id_and_never_block_trimming(&[], &["users", "posts", "comments", "get_users_on_change"])]
#[case::overrides_match_the_operation_id_or_the_trimmed_id(
  &[("getUsers", "fetchAllUsers"), ("comments", "list comments"), ("missingOp", "unused")],
  &["fetch_all_users", "posts", "list_comments", "get_users_on_change"]
)]
fn http_ids_drop_shared_affixes_before_callbacks_and_overrides_apply(
  #[case] overrides: &[(&str, &str)],
  #[case] expected: &[&str],
) {
  let overrides = overrides
    .iter()
    .map(|(operation, name)| ((*operation).to_string(), (*name).to_string()))
    .collect::<HashMap<_, _>>();
  let spec = parse_spec(spec!("generator/registry_trimming.json"));
  let registry = OperationRegistry::with_filters(&spec, None, None, &overrides);

  assert_eq!(stable_ids(&registry), expected);
}

proptest! {
  #![proptest_config(ProptestConfig::with_cases(48))]

  #[test]
  fn callbacks_pass_filters_through_their_owner_or_their_own_id(only in id_subset(), excluded in id_subset()) {
    let spec = parse_spec(spec!("generator/registry_trimming.json"));
    let only = only.as_deref().map(string_set);
    let excluded = excluded.as_deref().map(string_set);
    let registry = OperationRegistry::with_filters(&spec, only.as_ref(), excluded.as_ref(), &HashMap::new());

    let names_any = |set: &HashSet<String>, ids: &[&str]| ids.iter().any(|id| set.contains(*id));
    let selected = |ids: &[&str]| {
      only.as_ref().is_none_or(|only| names_any(only, ids)) && !excluded.as_ref().is_some_and(|excluded| names_any(excluded, ids))
    };
    let expected = [
      ("/users", &["get_users"][..]),
      ("/posts", &["get_posts"][..]),
      ("/comments", &["get_comments"][..]),
      ("callbacks/getUsers/onChange", &["get_users", "get_users_on_change"][..]),
    ]
    .into_iter()
    .filter(|(_, ids)| selected(ids))
    .map(|(path, _)| path)
    .collect::<BTreeSet<_>>();
    let registered = registry.operations().map(|entry| entry.path.as_str()).collect::<BTreeSet<_>>();
    prop_assert_eq!(registered, expected);

    let callback_ids = registry
      .operations()
      .filter(|entry| entry.kind == OperationKind::Callback)
      .map(|entry| entry.stable_id.as_str())
      .collect::<Vec<_>>();
    prop_assert!(callback_ids.iter().all(|id| *id == "get_users_on_change"), "callback ids {callback_ids:?}");
  }

  #[test]
  fn stable_ids_are_unique_rust_identifiers(
    operations in vec((select(ROUTES.to_vec()), select(METHODS.to_vec()), option::of(select(OPERATION_IDS.to_vec()))), 1..10)
  ) {
    let spec = spec_with_operations(&operations)?;
    let registry = OperationRegistry::new(&spec);
    let ids = stable_ids(&registry);

    let declared = operations.iter().map(|(route, method, _)| (*route, *method)).collect::<HashSet<_>>();
    prop_assert_eq!(ids.len(), declared.len());
    prop_assert_eq!(ids.iter().collect::<HashSet<_>>().len(), ids.len(), "duplicate ids in {:?}", ids);
    for id in &ids {
      prop_assert!(syn::parse_str::<syn::Ident>(id).is_ok(), "`{}` is not an identifier", id);
    }
    let rebuilt = OperationRegistry::new(&spec);
    prop_assert_eq!(stable_ids(&rebuilt), ids);
  }
}

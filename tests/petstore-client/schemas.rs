use petstore_client as client;
use petstore_server as server;
use rstest::rstest;
use serde_json::json;

#[rstest]
fn recursive_unions_default_to_a_variant_that_does_not_recurse() -> anyhow::Result<()> {
  assert_eq!(client::Node::default(), client::Node::Leaf);
  assert_eq!(server::Node::default(), server::Node::Leaf);
  let null_node = serde_json::from_value::<client::Wrapper>(json!({ "node": null, "kind": "w" }))?;
  assert_eq!(*null_node.node, client::Node::Leaf);

  let wire = json!({ "node": { "child": { "child": "leaf", "label": "inner" } }, "kind": "w" });
  assert_eq!(
    serde_json::to_value(serde_json::from_value::<client::Wrapper>(wire.clone())?)?,
    wire
  );
  assert_eq!(
    serde_json::to_value(serde_json::from_value::<server::Wrapper>(wire.clone())?)?,
    wire
  );
  Ok(())
}

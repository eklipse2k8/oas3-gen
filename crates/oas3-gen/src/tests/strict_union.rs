#[cfg(test)]
mod tests {
  use serde_json::json;

  use crate::fixtures::strict_union::*;

  #[test]
  fn test_discriminated_variant_round_trips_under_deny_unknown_fields() {
    let media_item = json!({
      "type": "mediaItem",
      "uuid": "item-1",
      "deviceId": "device-1",
      "title": "Track One"
    });

    let change: Change = serde_json::from_value(media_item).expect("mediaItem should deserialize");
    let Change::MediaItem(ref item) = change else {
      panic!("expected MediaItem variant, got {change:?}");
    };
    assert_eq!(item.uuid, "item-1", "uuid mismatch");
    assert_eq!(item.device_id, "device-1", "deviceId mismatch");
    assert_eq!(item.title.as_deref(), Some("Track One"), "title mismatch");
  }

  #[test]
  fn test_constructed_change_serializes_with_discriminator() {
    let change = Change::MediaItem(MediaItemChange {
      uuid: "item-1".to_string(),
      device_id: "device-1".to_string(),
      r#type: Some("mediaItem"),
      title: Some("Track One".to_string()),
      filter: None,
    });
    let serialized = serde_json::to_value(&change).expect("Change should serialize");
    assert_eq!(serialized["type"], "mediaItem", "serialized discriminator mismatch");

    let reparsed: Change = serde_json::from_value(serialized).expect("re-deserialize a constructed Change");
    let Change::MediaItem(item) = reparsed else {
      panic!("expected MediaItem after reparse");
    };
    assert_eq!(item.title.as_deref(), Some("Track One"), "title lost on round-trip");
  }

  #[test]
  fn test_second_variant_round_trips() {
    let playlist = json!({
      "type": "playlist",
      "uuid": "list-1",
      "deviceId": "device-9",
      "name": "Summer Set"
    });
    let change: Change = serde_json::from_value(playlist).expect("playlist should deserialize");
    let Change::Playlist(ref list) = change else {
      panic!("expected Playlist variant, got {change:?}");
    };
    assert_eq!(list.name.as_deref(), Some("Summer Set"), "name mismatch");
  }

  #[test]
  fn test_nested_discriminated_union_round_trips() {
    let with_filter = json!({
      "type": "mediaItem",
      "uuid": "item-2",
      "deviceId": "device-2",
      "filter": {"type": "number", "value": 128.0}
    });
    let change: Change = serde_json::from_value(with_filter).expect("nested filter should deserialize");
    let Change::MediaItem(item) = change.clone() else {
      panic!("expected MediaItem variant, got {change:?}");
    };
    let filter = item.filter.expect("filter should be present");
    assert!(
      matches!(&*filter, Filter::Number(n) if (n.value - 128.0).abs() < f64::EPSILON),
      "nested filter should be Number(128.0), got {filter:?}"
    );
  }

  #[test]
  fn test_nested_discriminated_union_serializes_with_discriminator() {
    let change = Change::MediaItem(MediaItemChange {
      uuid: "item-2".to_string(),
      device_id: "device-2".to_string(),
      r#type: Some("mediaItem"),
      title: None,
      filter: Some(Box::new(Filter::number(128.0))),
    });
    let serialized = serde_json::to_value(&change).expect("Change with nested filter should serialize");
    assert_eq!(serialized["type"], "mediaItem", "outer discriminator mismatch");
    assert_eq!(serialized["filter"]["type"], "number", "nested discriminator mismatch");
    assert!(
      (serialized["filter"]["value"].as_f64().unwrap() - 128.0).abs() < f64::EPSILON,
      "nested value mismatch"
    );

    let reparsed: Change = serde_json::from_value(serialized).expect("re-deserialize nested");
    let Change::MediaItem(item) = reparsed else {
      panic!("expected MediaItem after reparse");
    };
    let filter = item.filter.expect("filter should survive round-trip");
    assert!(
      matches!(&*filter, Filter::Number(n) if (n.value - 128.0).abs() < f64::EPSILON),
      "nested filter should remain Number(128.0)"
    );
  }

  #[test]
  fn test_unknown_field_still_rejected() {
    let with_unknown = json!({
      "type": "playlist",
      "uuid": "list-2",
      "deviceId": "device-3",
      "name": "Has Extra",
      "bogus": true
    });
    let result = serde_json::from_value::<Change>(with_unknown);
    assert!(
      result.is_err(),
      "deny_unknown_fields must still reject genuinely unknown fields after the discriminator strip"
    );
  }

  #[test]
  fn test_missing_discriminator_is_an_error() {
    let no_type = json!({"uuid": "x", "deviceId": "y"});
    let result = serde_json::from_value::<Change>(no_type);
    assert!(
      result.is_err(),
      "a Change without a discriminator should fail to deserialize"
    );
  }

  #[test]
  fn test_client_constructs() {
    let client = StrictDiscriminatedUnionApiClient::with_base_url("https://example.com");
    assert!(client.is_ok(), "client should construct from a valid base url");
  }
}

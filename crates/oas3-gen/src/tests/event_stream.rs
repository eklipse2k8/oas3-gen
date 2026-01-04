#[allow(unused)]
#[path = "../../fixtures/event_stream/mod.rs"]
#[allow(clippy::module_inception)]
mod event_stream;

#[cfg(test)]
mod tests {
  use super::event_stream::*;

  #[test]
  fn test_stream_event_struct_compiles() {
    let event = StreamEvent {
      id: "evt-123".to_string(),
      data: r#"{"message":"hello"}"#.to_string(),
      timestamp: None,
    };
    assert_eq!(event.id, "evt-123");
  }

  #[test]
  fn test_stream_event_deserializes() {
    let json = r#"{"id": "evt-456", "data": "test data"}"#;
    let event: StreamEvent = serde_json::from_str(json).unwrap();
    assert_eq!(event.id, "evt-456");
    assert_eq!(event.data, "test data");
    assert!(event.timestamp.is_none());
  }

  #[test]
  fn test_typed_event_with_enum() {
    let payload = EventPayload {
      id: "item-1".to_string(),
      name: Some("Test Item".to_string()),
      value: Some(42),
    };
    let event = TypedEvent {
      r#type: TypedEventType::Created,
      payload,
    };
    assert!(matches!(event.r#type, TypedEventType::Created));
    assert_eq!(event.payload.id, "item-1");
  }

  #[test]
  fn test_typed_event_deserializes() {
    let json = r#"{
      "type": "updated",
      "payload": {
        "id": "item-2",
        "name": "Updated Item",
        "value": 100
      }
    }"#;
    let event: TypedEvent = serde_json::from_str(json).unwrap();
    assert!(matches!(event.r#type, TypedEventType::Updated));
    assert_eq!(event.payload.id, "item-2");
    assert_eq!(event.payload.value, Some(100));
  }

  #[test]
  fn test_typed_event_type_enum_values() {
    let json_created = r#""created""#;
    let json_updated = r#""updated""#;
    let json_deleted = r#""deleted""#;

    let created: TypedEventType = serde_json::from_str(json_created).unwrap();
    let updated: TypedEventType = serde_json::from_str(json_updated).unwrap();
    let deleted: TypedEventType = serde_json::from_str(json_deleted).unwrap();

    assert!(matches!(created, TypedEventType::Created));
    assert!(matches!(updated, TypedEventType::Updated));
    assert!(matches!(deleted, TypedEventType::Deleted));
  }

  #[test]
  fn test_stream_events_request_is_empty() {
    let request = EventsRequest {};
    assert!(std::mem::size_of_val(&request) == 0 || std::mem::size_of_val(&request) == 1);
  }

  #[test]
  fn test_stream_typed_events_request_has_query() {
    let request = TypedEventsRequest {
      query: TypedEventsRequestQuery {
        filter: Some("active".to_string()),
      },
    };
    assert_eq!(request.query.filter, Some("active".to_string()));
  }

  #[test]
  fn test_query_serialization() {
    let query = TypedEventsRequestQuery {
      filter: Some("status:active".to_string()),
    };
    let serialized = serde_json::to_string(&query).unwrap();
    assert!(serialized.contains("filter"));
    assert!(serialized.contains("status:active"));
  }

  #[test]
  fn test_query_with_none_filter() {
    let query = TypedEventsRequestQuery { filter: None };
    let serialized = serde_json::to_string(&query).unwrap();
    assert!(!serialized.contains("filter"));
  }
}

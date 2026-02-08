#[cfg(test)]
mod tests {
  use std::collections::HashMap;

  use serde_json::json;

  use crate::fixtures::union_serde::*;

  fn text_block(text: &str) -> TextBlock {
    TextBlock {
      text: text.to_string(),
      annotations: None,
      recipes: None,
      r#type: Some("text".to_string()),
    }
  }

  fn code_block(code: &str) -> CodeBlock {
    CodeBlock {
      code: code.to_string(),
      language: None,
      r#type: Some("code".to_string()),
    }
  }

  fn url_source(url: &str) -> UrlImageSource {
    UrlImageSource {
      url: url.to_string(),
      r#type: Some("url".to_string()),
    }
  }

  fn image_block(source: ImageSource) -> ImageBlock {
    ImageBlock {
      source: Box::new(source),
      alt_text: None,
      r#type: Some("image".to_string()),
    }
  }

  #[test]
  fn test_deserialize_content_blocks() {
    let text_json = json!({"type": "text", "text": "Hello, world!"});
    let block: ContentBlock = serde_json::from_value(text_json).expect("text block");
    assert!(
      matches!(block, ContentBlock::Text(ref tb) if tb.text == "Hello, world!"),
      "text block mismatch"
    );

    let code_json = json!({"type": "code", "code": "fn main() {}", "language": "rust"});
    let block: ContentBlock = serde_json::from_value(code_json).expect("code block");
    let ContentBlock::Code(ref cb) = block else {
      panic!("Expected CodeBlock, got {block:?}");
    };
    assert_eq!(cb.code, "fn main() {}", "code mismatch");
    assert_eq!(cb.language, Some("rust".to_string()), "language mismatch");

    let tool_json =
      json!({"type": "tool_use", "id": "tool_123", "name": "calculator", "input": {"expression": "2 + 2"}});
    let block: ContentBlock = serde_json::from_value(tool_json).expect("tool use block");
    let ContentBlock::ToolUse(ref tu) = block else {
      panic!("Expected ToolUseBlock, got {block:?}");
    };
    assert_eq!(tu.id, "tool_123", "tool id mismatch");
    assert_eq!(tu.name, "calculator", "tool name mismatch");

    let base64_json = json!({"type": "image", "source": {"type": "base64", "media_type": "image/png", "data": [104, 101, 108, 108, 111]}});
    let block: ContentBlock = serde_json::from_value(base64_json).expect("image with base64");
    let ContentBlock::Image(ref img) = block else {
      panic!("Expected ImageBlock with base64, got {block:?}");
    };
    assert!(
      matches!(&*img.source, ImageSource::Base64(src) if src.media_type == MediaType::ImagePng),
      "base64 source mismatch"
    );

    let url_json = json!({"type": "image", "source": {"type": "url", "url": "https://example.com/image.png"}, "alt_text": "An example image"});
    let block: ContentBlock = serde_json::from_value(url_json).expect("image with url");
    let ContentBlock::Image(ref img) = block else {
      panic!("Expected ImageBlock with url, got {block:?}");
    };
    assert_eq!(img.alt_text, Some("An example image".to_string()), "alt_text mismatch");
    assert!(
      matches!(&*img.source, ImageSource::Url(src) if src.url == "https://example.com/image.png"),
      "url source mismatch"
    );
  }

  #[test]
  fn test_deserialize_tool_results() {
    let string_json = json!({
      "type": "tool_result",
      "tool_use_id": "tool_123",
      "content": "The result is 4"
    });
    let block: ContentBlock = serde_json::from_value(string_json).unwrap();
    let ContentBlock::ToolResult(tr) = block else {
      panic!("Expected ToolResultBlock for string content, got {block:?}");
    };
    assert_eq!(tr.tool_use_id, "tool_123", "tool_use_id mismatch for string content");
    assert!(
      matches!(tr.content, ToolResultContent::String(ref s) if s == "The result is 4"),
      "content mismatch for string content"
    );

    let array_json = json!({
      "type": "tool_result",
      "tool_use_id": "tool_456",
      "content": [
        {"type": "text", "text": "Here is the result"},
        {"type": "image", "source": {"type": "url", "url": "https://example.com/result.png"}}
      ],
      "is_error": false
    });
    let block: ContentBlock = serde_json::from_value(array_json).unwrap();
    let ContentBlock::ToolResult(tr) = block else {
      panic!("Expected ToolResultBlock for array content, got {block:?}");
    };
    assert_eq!(tr.tool_use_id, "tool_456", "tool_use_id mismatch for array content");
    assert_eq!(tr.is_error, Some(false), "is_error mismatch for array content");
    let ToolResultContent::Array(arr) = &tr.content else {
      panic!("Expected array content, got {:?}", tr.content);
    };
    assert_eq!(arr.len(), 2, "array length mismatch");
    assert!(
      matches!(&arr[0], ToolResultContentBlock::Text(_)),
      "first element should be Text"
    );
    assert!(
      matches!(&arr[1], ToolResultContentBlock::Image(_)),
      "second element should be Image"
    );
  }

  #[test]
  fn test_deserialize_text_block_with_annotations() {
    let json = json!({
      "type": "text",
      "text": "Check out this link and citation",
      "annotations": [
        {"type": "citation", "start": 0, "end": 10, "source": "doc.pdf"},
        {"type": "link", "start": 20, "end": 30, "url": "https://example.com"}
      ]
    });

    let block: ContentBlock = serde_json::from_value(json).unwrap();
    let ContentBlock::Text(tb) = block else {
      panic!("Expected TextBlock, got {block:?}");
    };
    let annotations = tb.annotations.as_ref().unwrap();
    assert_eq!(annotations.len(), 2, "expected 2 annotations");
    assert!(
      matches!(&annotations[0], Annotation::Citation(c) if c.source == "doc.pdf"),
      "first annotation should be citation"
    );
    assert!(
      matches!(&annotations[1], Annotation::Link(l) if l.url == "https://example.com"),
      "second annotation should be link"
    );
  }

  #[test]
  fn test_deserialize_array_of_content_blocks() {
    let json = json!([
      { "type": "text", "text": "First" },
      { "type": "code", "code": "print('hello')", "language": "python" },
      { "type": "text", "text": "Last" }
    ]);

    let blocks: Vec<ContentBlock> = serde_json::from_value(json).unwrap();
    assert_eq!(blocks.len(), 3, "expected 3 blocks");
    assert!(matches!(&blocks[0], ContentBlock::Text(_)), "first should be Text");
    assert!(matches!(&blocks[1], ContentBlock::Code(_)), "second should be Code");
    assert!(matches!(&blocks[2], ContentBlock::Text(_)), "third should be Text");
  }

  #[test]
  fn test_deserialize_events() {
    let message_start = json!({
      "type": "message_start",
      "message": {"id": "msg_123", "role": "assistant", "content": [], "model": "claude-3"}
    });
    let event: Event = serde_json::from_value(message_start).unwrap();
    let Event::MessageStart(e) = event else {
      panic!("Expected MessageStartEvent, got {event:?}");
    };
    assert_eq!(e.message.id, "msg_123", "message_start id mismatch");
    assert_eq!(e.message.role, Role::Assistant, "message_start role mismatch");

    let content_block_start = json!({
      "type": "content_block_start",
      "index": 0,
      "content_block": {"type": "text", "text": ""}
    });
    let event: Event = serde_json::from_value(content_block_start).unwrap();
    let Event::ContentBlockStart(e) = event else {
      panic!("Expected ContentBlockStartEvent, got {event:?}");
    };
    assert_eq!(e.index, 0, "content_block_start index mismatch");
    assert!(
      matches!(*e.content_block, ContentBlock::Text(_)),
      "content_block should be Text"
    );

    let text_delta = json!({
      "type": "content_block_delta",
      "index": 0,
      "delta": {"type": "text_delta", "text": "Hello"}
    });
    let event: Event = serde_json::from_value(text_delta).unwrap();
    let Event::ContentBlockDelta(e) = event else {
      panic!("Expected ContentBlockDeltaEvent for text, got {event:?}");
    };
    assert_eq!(e.index, 0, "text delta index mismatch");
    assert!(
      matches!(*e.delta, Delta::Text(ref d) if d.text == "Hello"),
      "delta should be text"
    );

    let json_delta = json!({
      "type": "content_block_delta",
      "index": 1,
      "delta": {"type": "input_json_delta", "partial_json": "{\"key\": \"val"}
    });
    let event: Event = serde_json::from_value(json_delta).unwrap();
    let Event::ContentBlockDelta(e) = event else {
      panic!("Expected ContentBlockDeltaEvent for json, got {event:?}");
    };
    assert_eq!(e.index, 1, "json delta index mismatch");
    let Delta::InputJson(ref d) = *e.delta else {
      panic!("Expected InputJsonDelta, got {:?}", *e.delta);
    };
    assert_eq!(d.partial_json, "{\"key\": \"val", "partial_json mismatch");

    let ping_event: Event = serde_json::from_value(json!({"type": "ping"})).unwrap();
    assert!(matches!(ping_event, Event::Ping(_)), "expected Ping event");

    let stop_event: Event = serde_json::from_value(json!({"type": "message_stop"})).unwrap();
    assert!(
      matches!(stop_event, Event::MessageStop(_)),
      "expected MessageStop event"
    );

    let event_list_json = json!({
      "events": [
        {"type": "ping"},
        {"type": "message_stop"},
        {"type": "content_block_stop", "index": 5}
      ]
    });
    let list: EventList = serde_json::from_value(event_list_json).unwrap();
    assert_eq!(list.events.len(), 3, "event list length mismatch");
    assert!(matches!(&list.events[0], Event::Ping(_)), "first event should be Ping");
    assert!(
      matches!(&list.events[1], Event::MessageStop(_)),
      "second event should be MessageStop"
    );
    assert!(
      matches!(&list.events[2], Event::ContentBlockStop(e) if e.index == 5),
      "third event should be ContentBlockStop with index 5"
    );
  }

  #[test]
  fn test_deserialize_responses() {
    let content_response_json = json!({
      "id": "resp_123",
      "content": {"type": "text", "text": "Response text"},
      "usage": {"input_tokens": 100, "output_tokens": 50}
    });
    let resp: ContentResponse = serde_json::from_value(content_response_json).unwrap();
    assert_eq!(resp.id, "resp_123", "content response id mismatch");
    assert!(
      matches!(*resp.content, ContentBlock::Text(ref tb) if tb.text == "Response text"),
      "content mismatch"
    );
    assert_eq!(resp.usage.as_ref().unwrap().input_tokens, 100, "input_tokens mismatch");
    assert_eq!(resp.usage.as_ref().unwrap().output_tokens, 50, "output_tokens mismatch");

    let error_response_json = json!({
      "type": "error",
      "error": {"type": "invalid_request_error", "message": "Invalid request parameters"}
    });
    let err: ErrorResponse = serde_json::from_value(error_response_json).unwrap();
    assert_eq!(err.r#type, "error", "error response type mismatch");
    assert_eq!(
      err.error.message, "Invalid request parameters",
      "error message mismatch"
    );
  }

  #[test]
  fn test_serialize_content_blocks() {
    let text = ContentBlock::Text(text_block("Hello, world!"));
    let json = serde_json::to_value(&text).unwrap();
    assert_eq!(json["type"], "text", "text type mismatch");
    assert_eq!(json["text"], "Hello, world!", "text content mismatch");

    let code = ContentBlock::Code(code_block("println!(\"Hello\")"));
    let json = serde_json::to_value(&code).unwrap();
    assert_eq!(json["type"], "code", "code type mismatch");
    assert_eq!(json["code"], "println!(\"Hello\")", "code content mismatch");

    let source = ImageSource::Url(url_source("https://example.com/img.png"));
    let image = ContentBlock::Image(image_block(source));
    let json = serde_json::to_value(&image).unwrap();
    assert_eq!(json["type"], "image", "image type mismatch");
    assert_eq!(json["source"]["type"], "url", "source type mismatch");
    assert_eq!(json["source"]["url"], "https://example.com/img.png", "url mismatch");
  }

  #[test]
  fn test_serialize_content_request() {
    let request = ContentRequest {
      blocks: vec![
        ContentBlock::Text(text_block("Hello")),
        ContentBlock::Code(code_block("x = 1")),
      ],
      metadata: None,
    };

    let json = serde_json::to_value(&request).unwrap();
    assert!(json["blocks"].is_array(), "blocks should be array");
    assert_eq!(json["blocks"].as_array().unwrap().len(), 2, "blocks length mismatch");
    assert_eq!(json["blocks"][0]["type"], "text", "first block type mismatch");
    assert_eq!(json["blocks"][1]["type"], "code", "second block type mismatch");
  }

  #[test]
  fn test_serialize_tool_results() {
    let string_block = ToolResultBlock {
      tool_use_id: "tool_123".to_string(),
      content: ToolResultContent::String("Result".to_string()),
      is_error: None,
      r#type: Some("tool_result".to_string()),
      iterations: None,
    };
    let json = serde_json::to_value(&string_block).unwrap();
    assert_eq!(json["type"], "tool_result", "string result type mismatch");
    assert_eq!(json["tool_use_id"], "tool_123", "tool_use_id mismatch");
    assert_eq!(json["content"], "Result", "content should be string");

    let array_block = ToolResultBlock {
      tool_use_id: "tool_456".to_string(),
      content: ToolResultContent::Array(vec![ToolResultContentBlock::Text(text_block("Text result"))]),
      is_error: Some(false),
      r#type: Some("tool_result".to_string()),
      iterations: None,
    };
    let json = serde_json::to_value(&array_block).unwrap();
    assert_eq!(json["type"], "tool_result", "array result type mismatch");
    assert_eq!(json["is_error"], false, "is_error mismatch");
    assert!(json["content"].is_array(), "content should be array");
    assert_eq!(json["content"][0]["type"], "text", "first content type mismatch");
  }

  #[test]
  fn test_roundtrips() {
    let text = TextBlock {
      text: "Round trip test".to_string(),
      annotations: None,
      recipes: None,
      r#type: Some("text".to_string()),
    };
    let json = serde_json::to_string(&text).unwrap();
    let deserialized: TextBlock = serde_json::from_str(&json).unwrap();
    assert_eq!(text.text, deserialized.text, "text roundtrip failed");

    let content = ContentBlock::Text(text_block("Test content"));
    let json = serde_json::to_string(&content).unwrap();
    let deserialized: ContentBlock = serde_json::from_str(&json).unwrap();
    let (ContentBlock::Text(a), ContentBlock::Text(b)) = (content, deserialized) else {
      panic!("Type mismatch after ContentBlock roundtrip");
    };
    assert_eq!(a.text, b.text, "content block roundtrip failed");

    let base64_source = ImageSource::Base64(Base64ImageSource {
      media_type: MediaType::ImagePng,
      data: vec![1, 2, 3, 4],
      r#type: Some("base64".to_string()),
    });
    let json = serde_json::to_string(&base64_source).unwrap();
    let deserialized: ImageSource = serde_json::from_str(&json).unwrap();
    let (ImageSource::Base64(a), ImageSource::Base64(b)) = (base64_source, deserialized) else {
      panic!("Type mismatch after ImageSource roundtrip");
    };
    assert_eq!(a.media_type, b.media_type, "image source roundtrip failed");

    let nested = ContentBlock::ToolResult(ToolResultBlock {
      tool_use_id: "nested_test".to_string(),
      content: ToolResultContent::Array(vec![
        ToolResultContentBlock::Text(text_block("Nested text")),
        ToolResultContentBlock::Image(ImageBlock {
          source: Box::new(ImageSource::Url(url_source("https://example.com/nested.png"))),
          alt_text: Some("Nested image".to_string()),
          r#type: Some("image".to_string()),
        }),
      ]),
      is_error: None,
      r#type: Some("tool_result".to_string()),
      iterations: None,
    });
    let json = serde_json::to_string(&nested).unwrap();
    let deserialized: ContentBlock = serde_json::from_str(&json).unwrap();
    let ContentBlock::ToolResult(tr) = deserialized else {
      panic!("Expected ToolResultBlock in nested roundtrip");
    };
    assert_eq!(tr.tool_use_id, "nested_test", "nested roundtrip tool_use_id mismatch");
    let ToolResultContent::Array(arr) = tr.content else {
      panic!("Expected array content in nested roundtrip");
    };
    assert_eq!(arr.len(), 2, "nested roundtrip array length mismatch");
  }

  #[test]
  fn test_array_or_single() {
    let single_json = json!({"type": "text", "text": "Single item"});
    let result: ArrayOrSingle = serde_json::from_value(single_json).unwrap();
    assert!(
      matches!(result, ArrayOrSingle::TextBlock(tb) if tb.text == "Single item"),
      "single item mismatch"
    );

    let array_json = json!([
      {"type": "text", "text": "First"},
      {"type": "text", "text": "Second"}
    ]);
    let result: ArrayOrSingle = serde_json::from_value(array_json).unwrap();
    let ArrayOrSingle::Array(arr) = result else {
      panic!("Expected array, got {result:?}");
    };
    assert_eq!(arr.len(), 2, "array length mismatch");
    assert_eq!(arr[0].text, "First", "first item mismatch");
    assert_eq!(arr[1].text, "Second", "second item mismatch");

    let helper_result = ArrayOrSingle::text_block("Helper created".to_string());
    let ArrayOrSingle::TextBlock(tb) = helper_result else {
      panic!("Expected TextBlock from helper, got {helper_result:?}");
    };
    assert_eq!(tb.text, "Helper created", "helper text mismatch");
    assert_eq!(tb.r#type, Some("text".to_string()), "helper type should be set");
  }

  #[test]
  fn test_nullable_string_or_number() {
    let string_json = json!("hello");
    let result: NullableStringOrNumber = serde_json::from_value(string_json).unwrap();
    assert!(
      matches!(result, NullableStringOrNumber::String(s) if s == "hello"),
      "string case failed"
    );

    let number_json = json!(42.5);
    let result: NullableStringOrNumber = serde_json::from_value(number_json).unwrap();
    assert!(
      matches!(result, NullableStringOrNumber::Number(n) if (n - 42.5).abs() < f64::EPSILON),
      "number case failed"
    );
  }

  #[test]
  fn test_type_construction() {
    let text = ContentBlock::Text(text_block("Hello"));
    assert!(matches!(text, ContentBlock::Text(_)), "text construction failed");

    let code = ContentBlock::Code(code_block("x = 1"));
    assert!(matches!(code, ContentBlock::Code(_)), "code construction failed");

    let source = ImageSource::Url(url_source("https://example.com"));
    let image = ContentBlock::Image(image_block(source));
    assert!(matches!(image, ContentBlock::Image(_)), "image construction failed");

    let ping = Event::Ping(PingEvent {
      r#type: Some("ping".to_string()),
    });
    assert!(matches!(ping, Event::Ping(_)), "ping event construction failed");

    let stop = Event::MessageStop(MessageStopEvent {
      r#type: Some("message_stop".to_string()),
    });
    assert!(
      matches!(stop, Event::MessageStop(_)),
      "message stop event construction failed"
    );

    let block_stop = Event::ContentBlockStop(ContentBlockStopEvent {
      index: 5,
      r#type: Some("content_block_stop".to_string()),
    });
    let Event::ContentBlockStop(e) = block_stop else {
      panic!("Expected ContentBlockStop");
    };
    assert_eq!(e.index, 5, "content block stop index mismatch");

    let text_delta = Delta::Text(TextDelta {
      text: "chunk".to_string(),
      r#type: Some("text_delta".to_string()),
    });
    assert!(
      matches!(text_delta, Delta::Text(d) if d.text == "chunk"),
      "text delta construction failed"
    );

    let json_delta = Delta::InputJson(InputJsonDelta {
      partial_json: "{\"partial\":".to_string(),
      r#type: Some("input_json_delta".to_string()),
    });
    assert!(
      matches!(json_delta, Delta::InputJson(d) if d.partial_json == "{\"partial\":"),
      "json delta construction failed"
    );
  }

  #[test]
  fn test_complex_server_response_simulation() {
    let server_response = json!({
      "events": [
        {"type": "message_start", "message": {"id": "msg_abc123", "role": "assistant", "content": [], "model": "claude-3-opus"}},
        {"type": "content_block_start", "index": 0, "content_block": {"type": "text", "text": ""}},
        {"type": "content_block_delta", "index": 0, "delta": {"type": "text_delta", "text": "Here is your "}},
        {"type": "content_block_delta", "index": 0, "delta": {"type": "text_delta", "text": "response."}},
        {"type": "content_block_stop", "index": 0},
        {"type": "content_block_start", "index": 1, "content_block": {"type": "tool_use", "id": "tool_xyz", "name": "calculator", "input": {}}},
        {"type": "content_block_delta", "index": 1, "delta": {"type": "input_json_delta", "partial_json": "{\"expr\":"}},
        {"type": "content_block_delta", "index": 1, "delta": {"type": "input_json_delta", "partial_json": "\"2+2\"}"}},
        {"type": "content_block_stop", "index": 1},
        {"type": "message_stop"}
      ]
    });

    let event_list: EventList = serde_json::from_value(server_response).unwrap();
    assert_eq!(event_list.events.len(), 10, "expected 10 events");

    let mut text_chunks = vec![];
    let mut json_chunks = vec![];

    for event in &event_list.events {
      if let Event::ContentBlockDelta(e) = event {
        match &*e.delta {
          Delta::Text(d) => text_chunks.push(d.text.clone()),
          Delta::InputJson(d) => json_chunks.push(d.partial_json.clone()),
        }
      }
    }

    assert_eq!(text_chunks.join(""), "Here is your response.", "text chunks mismatch");
    assert_eq!(json_chunks.join(""), "{\"expr\":\"2+2\"}", "json chunks mismatch");
  }

  #[test]
  fn test_complex_client_request_simulation() {
    let request = ContentRequest {
      blocks: vec![
        ContentBlock::Text(text_block("Please analyze this image and code:")),
        ContentBlock::Image(ImageBlock {
          source: Box::new(ImageSource::Base64(Base64ImageSource {
            media_type: MediaType::ImagePng,
            data: vec![0x89, 0x50, 0x4E, 0x47],
            r#type: Some("base64".to_string()),
          })),
          alt_text: Some("Screenshot".to_string()),
          r#type: Some("image".to_string()),
        }),
        ContentBlock::Code(code_block("def hello():\n    print('world')")),
        ContentBlock::ToolResult(ToolResultBlock {
          tool_use_id: "prev_tool".to_string(),
          content: ToolResultContent::Array(vec![ToolResultContentBlock::Text(text_block("Previous tool output"))]),
          is_error: None,
          r#type: Some("tool_result".to_string()),
          iterations: None,
        }),
      ],
      metadata: None,
    };

    let json = serde_json::to_value(&request).unwrap();

    assert_eq!(json["blocks"].as_array().unwrap().len(), 4, "expected 4 blocks");
    assert_eq!(json["blocks"][0]["type"], "text", "block 0 type mismatch");
    assert_eq!(json["blocks"][1]["type"], "image", "block 1 type mismatch");
    assert_eq!(
      json["blocks"][1]["source"]["type"], "base64",
      "block 1 source type mismatch"
    );
    assert_eq!(json["blocks"][2]["type"], "code", "block 2 type mismatch");
    assert_eq!(json["blocks"][3]["type"], "tool_result", "block 3 type mismatch");
    assert!(
      json["blocks"][3]["content"].is_array(),
      "block 3 content should be array"
    );

    let blocks: Vec<ContentBlock> = serde_json::from_value(json["blocks"].clone()).unwrap();
    assert_eq!(blocks.len(), 4, "deserialized blocks length mismatch");
  }

  #[test]
  fn test_deeply_nested_tool_result() {
    let json = json!({
      "type": "tool_result",
      "tool_use_id": "deep_tool",
      "content": [
        {"type": "text", "text": "Here are the results:", "annotations": [{"type": "citation", "start": 0, "end": 4, "source": "analysis.pdf"}]},
        {"type": "image", "source": {"type": "url", "url": "https://results.example.com/chart.png"}, "alt_text": "Results chart"}
      ]
    });

    let block: ContentBlock = serde_json::from_value(json).unwrap();
    let ContentBlock::ToolResult(tr) = block else {
      panic!("Expected ToolResultBlock");
    };
    assert_eq!(tr.tool_use_id, "deep_tool", "tool_use_id mismatch");

    let ToolResultContent::Array(arr) = &tr.content else {
      panic!("Expected array content");
    };
    assert_eq!(arr.len(), 2, "content array length mismatch");

    let ToolResultContentBlock::Text(tb) = &arr[0] else {
      panic!("Expected TextBlock in content[0]");
    };
    let annotations = tb.annotations.as_ref().unwrap();
    assert_eq!(annotations.len(), 1, "annotations length mismatch");
    assert!(
      matches!(&annotations[0], Annotation::Citation(c) if c.source == "analysis.pdf"),
      "annotation should be citation"
    );

    let ToolResultContentBlock::Image(ib) = &arr[1] else {
      panic!("Expected ImageBlock in content[1]");
    };
    assert_eq!(ib.alt_text, Some("Results chart".to_string()), "alt_text mismatch");
    assert!(
      matches!(&*ib.source, ImageSource::Url(u) if u.url == "https://results.example.com/chart.png"),
      "image source url mismatch"
    );
  }

  #[test]
  fn test_metadata_variants() {
    let string_metadata = Metadata::String("simple metadata".to_string());
    let json = serde_json::to_value(&string_metadata).unwrap();
    assert_eq!(json, "simple metadata", "string metadata serialization failed");

    let object_data: HashMap<String, serde_json::Value> =
      [("key".to_string(), json!("value")), ("count".to_string(), json!(42))]
        .into_iter()
        .collect();
    let object_metadata = Metadata::Object(object_data);
    let json = serde_json::to_value(&object_metadata).unwrap();
    assert_eq!(json["key"], "value", "object metadata key mismatch");
    assert_eq!(json["count"], 42, "object metadata count mismatch");

    let session_data: HashMap<String, serde_json::Value> =
      [("session_id".to_string(), json!("abc123"))].into_iter().collect();
    let request_with_metadata = ContentRequest {
      blocks: vec![ContentBlock::Text(text_block("Hello"))],
      metadata: Some(Metadata::Object(session_data)),
    };
    let json = serde_json::to_value(&request_with_metadata).unwrap();
    assert_eq!(
      json["metadata"]["session_id"], "abc123",
      "nested object metadata mismatch"
    );
  }

  #[test]
  fn test_stop_reason_enum() {
    let cases = [
      ("end_turn", StopReason::EndTurn),
      ("max_tokens", StopReason::MaxTokens),
      ("stop_sequence", StopReason::StopSequence),
      ("tool_use", StopReason::ToolUse),
    ];
    for (json_str, expected) in cases {
      let deserialized: StopReason = serde_json::from_value(json!(json_str)).unwrap();
      assert_eq!(
        deserialized, expected,
        "StopReason deserialization failed for {json_str}"
      );
    }

    assert_eq!(StopReason::default(), StopReason::EndTurn, "default should be EndTurn");
  }

  #[test]
  fn test_request_types() {
    let _ = GetEventsRequest {};

    let send_content = SendContentRequest {
      body: ContentRequest {
        blocks: vec![ContentBlock::Text(text_block("Test"))],
        metadata: None,
      },
    };
    assert_eq!(send_content.body.blocks.len(), 1, "body blocks count mismatch");

    let _body: ContentRequest = ContentRequest {
      blocks: vec![],
      metadata: None,
    };
  }

  #[tokio::test]
  async fn test_parse_response_methods() {
    let event_json = json!({"events": [{"type": "ping"}]}).to_string();
    let mock_response = http::Response::builder()
      .status(200)
      .header("content-type", "application/json")
      .body(event_json)
      .unwrap();
    let reqwest_response = reqwest::Response::from(mock_response);
    let result = GetEventsRequest::parse_response(reqwest_response).await.unwrap();
    let GetEventsResponse::Ok(list) = result else {
      panic!("Expected Ok response, got {result:?}");
    };
    assert_eq!(list.events.len(), 1, "parsed events count mismatch");

    let content_json = json!({
      "id": "resp_456",
      "content": {"type": "text", "text": "Parsed response"},
      "usage": {"input_tokens": 5, "output_tokens": 15}
    })
    .to_string();
    let mock_response = http::Response::builder()
      .status(200)
      .header("content-type", "application/json")
      .body(content_json)
      .unwrap();
    let reqwest_response = reqwest::Response::from(mock_response);
    let result = SendContentRequest::parse_response(reqwest_response).await.unwrap();
    let SendContentResponse::Ok(resp) = result else {
      panic!("Expected Ok response, got {result:?}");
    };
    assert_eq!(resp.id, "resp_456", "parsed response id mismatch");
  }

  #[test]
  fn test_response_types() {
    let event_list = EventList {
      events: vec![Event::Ping(PingEvent {
        r#type: Some("ping".to_string()),
      })],
    };
    let ok_response = GetEventsResponse::Ok(event_list.clone());
    let GetEventsResponse::Ok(list) = ok_response else {
      panic!("Expected Ok variant, got {ok_response:?}");
    };
    assert_eq!(list.events.len(), 1, "event list length mismatch");

    let unknown_response = GetEventsResponse::Unknown;
    assert!(
      matches!(unknown_response, GetEventsResponse::Unknown),
      "expected Unknown variant"
    );

    let content_resp = ContentResponse {
      id: "resp_123".to_string(),
      content: Box::new(ContentBlock::Text(text_block("Response"))),
      usage: Some(Usage {
        input_tokens: 10,
        output_tokens: 20,
      }),
    };
    let send_ok = SendContentResponse::Ok(content_resp);
    let SendContentResponse::Ok(resp) = send_ok else {
      panic!("Expected Ok variant, got {send_ok:?}");
    };
    assert_eq!(resp.id, "resp_123", "response id mismatch");

    let error_resp = ErrorResponse {
      r#type: "error".to_string(),
      error: ErrorDetails {
        r#type: ErrorType::InvalidRequestError,
        message: "Bad request".to_string(),
      },
    };
    let send_error = SendContentResponse::BadRequest(error_resp);
    let SendContentResponse::BadRequest(err) = send_error else {
      panic!("Expected BadRequest variant, got {send_error:?}");
    };
    assert_eq!(err.error.message, "Bad request", "error message mismatch");

    let send_unknown = SendContentResponse::Unknown;
    assert!(
      matches!(send_unknown, SendContentResponse::Unknown),
      "expected Unknown variant"
    );
  }

  #[test]
  fn test_enum_deserialization() {
    let error_cases = [
      ("invalid_request_error", ErrorType::InvalidRequestError),
      ("authentication_error", ErrorType::AuthenticationError),
      ("permission_error", ErrorType::PermissionError),
      ("not_found_error", ErrorType::NotFoundError),
      ("rate_limit_error", ErrorType::RateLimitError),
      ("api_error", ErrorType::ApiError),
      ("overloaded_error", ErrorType::OverloadedError),
    ];
    for (json_str, expected) in error_cases {
      let deserialized: ErrorType = serde_json::from_value(json!(json_str)).unwrap();
      assert_eq!(
        deserialized, expected,
        "ErrorType deserialization failed for {json_str}"
      );
    }

    let role_cases = [("user", Role::User), ("assistant", Role::Assistant)];
    for (json_str, expected) in role_cases {
      let deserialized: Role = serde_json::from_value(json!(json_str)).unwrap();
      assert_eq!(deserialized, expected, "Role deserialization failed for {json_str}");
    }

    let media_cases = [
      (MediaType::ImageJpeg, "image/jpeg"),
      (MediaType::ImagePng, "image/png"),
      (MediaType::ImageGif, "image/gif"),
      (MediaType::ImageWebp, "image/webp"),
    ];
    for (variant, expected) in media_cases {
      let json = serde_json::to_value(&variant).unwrap();
      assert_eq!(
        json.as_str().unwrap(),
        expected,
        "MediaType serialization failed for {variant:?}"
      );
      let deserialized: MediaType = serde_json::from_value(json).unwrap();
      assert_eq!(deserialized, variant, "MediaType deserialization failed for {expected}");
    }
  }
}

use super::support::{
  assert_contains_all, assert_not_contains, generate_client, generate_server_files, generate_types, make_orchestrator,
  make_server_orchestrator,
};
use crate::utils::spec::SpecFormat;

const EVENT_STREAMS_SPEC: &str = r##"{
  "openapi": "3.1.0",
  "info": { "title": "Event Streams", "version": "1" },
  "paths": {
    "/frames": {
      "get": {
        "operationId": "streamFrames",
        "responses": {
          "200": {
            "description": "A document or a stream of frames",
            "content": {
              "application/json": { "schema": { "$ref": "#/components/schemas/Payload" } },
              "text/event-stream": {
                "schema": { "$ref": "#/components/schemas/Frame" },
                "x-speakeasy-sse-sentinel": "[DONE]"
              }
            }
          }
        }
      }
    },
    "/payloads": {
      "get": {
        "operationId": "streamPayloads",
        "responses": {
          "200": {
            "description": "Each event's data is a payload",
            "content": {
              "text/event-stream": {
                "schema": { "$ref": "#/components/schemas/Payload" },
                "x-sse-sentinel": "[END]"
              }
            }
          }
        }
      }
    },
    "/keyed": {
      "get": {
        "operationId": "streamKeyed",
        "responses": {
          "200": {
            "description": "Frames told apart by their event name",
            "content": {
              "text/event-stream": {
                "schema": { "$ref": "#/components/schemas/Keyed" },
                "x-sse-sentinel": "[STOP]",
                "x-speakeasy-sse-sentinel": "[HALT]"
              }
            }
          }
        }
      }
    },
    "/documents": {
      "get": {
        "operationId": "getDocument",
        "responses": {
          "200": {
            "description": "A JSON object that happens to look like a frame type",
            "content": { "application/json": { "schema": { "$ref": "#/components/schemas/JsonOnly" } } }
          }
        }
      }
    }
  },
  "components": {
    "schemas": {
      "Payload": {
        "type": "object",
        "required": ["data", "count"],
        "properties": { "data": { "type": "string" }, "count": { "type": "integer" } }
      },
      "Frame": {
        "type": "object",
        "required": ["data"],
        "properties": {
          "data": { "$ref": "#/components/schemas/Payload" },
          "event": { "type": "string", "enum": ["update", "final"] },
          "id": { "type": "string" },
          "retry": { "type": "integer" }
        }
      },
      "Keyed": { "oneOf": [{ "$ref": "#/components/schemas/TextFrame" }, { "$ref": "#/components/schemas/JsonFrame" }] },
      "TextFrame": {
        "type": "object",
        "required": ["event", "data"],
        "properties": { "event": { "const": "text" }, "data": { "type": "string" } }
      },
      "JsonFrame": {
        "type": "object",
        "required": ["event", "data"],
        "properties": {
          "event": { "const": "json" },
          "data": {
            "type": "string",
            "contentMediaType": "application/json",
            "contentSchema": { "$ref": "#/components/schemas/Payload" }
          }
        }
      },
      "JsonOnly": {
        "type": "object",
        "properties": { "data": { "type": "string" }, "event": { "type": "string" } }
      }
    }
  }
}"##;

fn impl_body<'a>(code: &'a str, header: &str) -> &'a str {
  let start = code
    .find(header)
    .unwrap_or_else(|| panic!("missing `{header}` in:\n{code}"));
  let end = start + code[start..].find("\n}\n").expect("impl closes");
  &code[start..end]
}

#[test]
fn frame_types_decode_and_encode_whole_frames() {
  let spec = SpecFormat::Json
    .parse(EVENT_STREAMS_SPEC.as_bytes())
    .expect("the spec parses");

  let client = make_orchestrator(spec.clone(), false);
  let types = generate_types(&client, "test.json").code;
  assert_contains_all(
    &types,
    &[
      (
        "pub type Frame = oas3_gen_support::SseFrame<Payload, Option<FrameEvent>>;",
        "a frame struct reads as the frame of its data and optional event types",
      ),
      (
        "pub type TextFrame = oas3_gen_support::SseFrame<String>;",
        "a string event name is the frame's default",
      ),
      (
        "pub type JsonFrame = oas3_gen_support::SseFrame<Payload>;",
        "data with a JSON contentSchema decodes as that schema",
      ),
      ("pub struct Payload {", "a payload stays a struct"),
      (
        "pub struct JsonOnly {",
        "a JSON body shaped like a frame stays a struct",
      ),
    ],
  );
  let keyed = impl_body(&types, "impl oas3_gen_support::FromSseFrame for Keyed {");
  assert_contains_all(
    keyed,
    &[
      (
        "\"text\" => frame.text().map(Self::Text),",
        "plain string data keeps the raw text",
      ),
      (
        "\"json\" => frame.json().map(Self::Json),",
        "the JSON member decodes its data as JSON",
      ),
      (
        "_ => Err(frame.unknown_event(&[\"text\", \"json\"])),",
        "an unlisted event name is an error at the event",
      ),
    ],
  );
  assert_contains_all(
    &types,
    &[
      (
        "pub enum PayloadOrFrameStream {",
        "the stream member is named after its item",
      ),
      (
        "FrameStream(oas3_gen_support::EventStream<Frame>)",
        "the stream variant",
      ),
      (
        ">::json(req, Some(\"[DONE]\"))",
        "a JSON frame type decodes each frame and ends at its x-speakeasy-sse-sentinel",
      ),
      (
        "oas3_gen_support::EventStream<oas3_gen_support::SseFrame<Payload>>",
        "a payload item is read in a frame",
      ),
      (
        ">::json(req, Some(\"[END]\"))",
        "payload data decodes as JSON and ends at its x-sse-sentinel",
      ),
      (
        ">::frames(req, Some(\"[STOP]\"))",
        "a union picks its member, and x-sse-sentinel wins when both are declared",
      ),
    ],
  );
  for absent in [
    "pub struct Frame",
    "pub struct TextFrame",
    "SseFrame<JsonOnly",
    "impl Default for Keyed",
    "Oas3GenSupport",
    "[HALT]",
  ] {
    assert_not_contains(&types, absent, "only streamed frame types read as frames");
  }
  let client_code = generate_client(&client, "test.json");
  assert_not_contains(
    &client_code,
    "Oas3GenSupport",
    "client signatures use the stream member name",
  );

  let server = make_server_orchestrator(spec);
  let (server_code, server_types) = generate_server_files(&server, "test.json");
  assert_contains_all(
    impl_body(&server_types, "impl TryFrom<Frame> for axum::response::sse::Event {"),
    &[
      ("event = event.json_data(frame.data)?;", "JSON data serializes"),
      (
        "let text = value.to_string();",
        "an enum event name is written through Display",
      ),
      (
        "if text.contains(['\\r', '\\n']) {",
        "an event name axum can't send is an error, not a panic, whatever its type",
      ),
      (
        "if value.contains(['\\0', '\\r', '\\n']) {",
        "an id axum can't send is an error, not a panic",
      ),
      (
        "u64::try_from(value).map_err(axum::Error::new)?",
        "the retry is written in milliseconds",
      ),
    ],
  );
  assert_contains_all(
    impl_body(
      &server_types,
      "impl TryFrom<TextFrame> for axum::response::sse::Event {",
    ),
    &[
      ("event = event.data(frame.data);", "plain string data is sent as text"),
      (
        "if frame.event.contains(['\\r', '\\n']) {",
        "an event name axum can't send is an error, not a panic",
      ),
    ],
  );
  assert_contains_all(
    impl_body(&server_types, "impl TryFrom<Keyed> for axum::response::sse::Event {"),
    &[(
      "Keyed::Text(member) => Self::try_from(member),",
      "a union sends its member",
    )],
  );
  assert_contains_all(
    &server_code,
    &[
      (
        "futures::stream::BoxStream<'static, Keyed>",
        "services return a stream of items",
      ),
      ("axum::response::sse::Event::try_from,", "frames become axum events"),
      (
        "futures::StreamExt::chain(",
        "the server ends the stream after its items",
      ),
      (
        "Ok(axum::response::sse::Event::default().data(\"[DONE]\"))",
        "the last event is the sentinel",
      ),
      (".json_data(item)", "payload items are sent as JSON data"),
    ],
  );
  for absent in ["IntoSseFrame", "SseFrame", "ServerEvents"] {
    assert_not_contains(&server_code, absent, "the server sends events through axum alone");
    assert_not_contains(&server_types, absent, "the server sends events through axum alone");
  }
}

const FRAME_EDGES_SPEC: &str = r##"{
  "openapi": "3.1.0",
  "info": { "title": "Frame Edges", "version": "1" },
  "paths": {
    "/shared": {
      "get": {
        "operationId": "streamShared",
        "responses": {
          "200": {
            "description": "Frames of a type that JSON bodies also use",
            "content": { "text/event-stream": { "schema": { "$ref": "#/components/schemas/Shared" } } }
          }
        }
      }
    },
    "/latest": {
      "get": {
        "operationId": "latestShared",
        "responses": {
          "200": {
            "description": "The last frame as JSON",
            "content": { "application/json": { "schema": { "$ref": "#/components/schemas/Shared" } } }
          }
        }
      }
    },
    "/mapped": {
      "get": {
        "operationId": "streamMapped",
        "responses": {
          "200": {
            "description": "Frames keyed by an explicit discriminator mapping",
            "content": { "text/event-stream": { "schema": { "$ref": "#/components/schemas/Mapped" } } }
          }
        }
      }
    },
    "/nested": {
      "get": {
        "operationId": "streamNested",
        "responses": {
          "200": {
            "description": "Frames of a union that a JSON type also holds",
            "content": { "text/event-stream": { "schema": { "$ref": "#/components/schemas/Nested" } } }
          }
        }
      }
    },
    "/holder": {
      "get": {
        "operationId": "getHolder",
        "responses": {
          "200": {
            "description": "A document holding the nested union",
            "content": { "application/json": { "schema": { "$ref": "#/components/schemas/Holder" } } }
          }
        }
      }
    }
  },
  "components": {
    "schemas": {
      "Shared": {
        "type": "object",
        "required": ["data"],
        "properties": { "data": { "type": "string" }, "event": { "type": "string", "enum": ["a", "b"] } }
      },
      "Mapped": {
        "oneOf": [{ "$ref": "#/components/schemas/Named" }, { "$ref": "#/components/schemas/Unnamed" }],
        "discriminator": {
          "propertyName": "event",
          "mapping": { "named": "#/components/schemas/Named", "unnamed": "#/components/schemas/Unnamed" }
        }
      },
      "Named": {
        "type": "object",
        "required": ["data"],
        "properties": { "data": { "type": "string" }, "event": { "type": "string" } }
      },
      "Unnamed": {
        "type": "object",
        "required": ["data"],
        "properties": { "data": { "type": "integer" } }
      },
      "Nested": {
        "oneOf": [{ "$ref": "#/components/schemas/NestedA" }, { "$ref": "#/components/schemas/NestedB" }],
        "discriminator": { "propertyName": "event" }
      },
      "NestedA": {
        "type": "object",
        "required": ["event", "data"],
        "properties": { "event": { "const": "nested-a" }, "data": { "type": "string" } }
      },
      "NestedB": {
        "type": "object",
        "required": ["event", "data"],
        "properties": { "event": { "const": "nested-b" }, "data": { "type": "string" } }
      },
      "Holder": {
        "type": "object",
        "properties": { "nested": { "$ref": "#/components/schemas/Nested" } }
      }
    }
  }
}"##;

#[test]
fn frame_types_other_types_use_stay_usable() {
  let spec = SpecFormat::Json
    .parse(FRAME_EDGES_SPEC.as_bytes())
    .expect("the spec parses");

  let client = make_orchestrator(spec.clone(), false);
  let types = generate_types(&client, "test.json").code;
  assert_contains_all(
    &types,
    &[
      (
        "pub struct Shared {",
        "a frame struct a JSON body also uses stays a struct",
      ),
      (
        "oas3_gen_support::SseFrame<String, Option<SharedEvent>>",
        "its stream reads the frame type in its place",
      ),
      (
        "pub type Named = oas3_gen_support::SseFrame<String>;",
        "a discriminator-typed event never becomes the frame's event type",
      ),
      (
        "pub type Unnamed = oas3_gen_support::SseFrame<i64>;",
        "a member without an event reads the default event type",
      ),
      (
        "\"unnamed\" => frame.json().map(Self::Unnamed),",
        "a member without an event is picked by its discriminator value",
      ),
      (
        "oas3_gen_support::EventStream<oas3_gen_support::SseFrame<Nested>>",
        "a union a JSON type also holds streams as a payload",
      ),
    ],
  );
  for absent in ["Option<&'static str>>;", "FromSseFrame for Nested", "pub type NestedA"] {
    assert_not_contains(&types, absent, "only types nothing else uses become frames");
  }

  let server = make_server_orchestrator(spec);
  let (server_code, server_types) = generate_server_files(&server, "test.json");
  assert_contains_all(
    impl_body(&server_types, "impl TryFrom<Mapped> for axum::response::sse::Event {"),
    &[
      (
        "Mapped::Named(member) => Self::try_from(member),",
        "a member with an event sends its own",
      ),
      (
        "Self::try_from(member).map(|sse| sse.event(\"unnamed\"))",
        "a member without an event sends its discriminator value",
      ),
    ],
  );
  assert_contains_all(
    &server_code,
    &[(
      "|item| axum::response::sse::Event::default().json_data(item)",
      "the nested union streams as JSON payloads",
    )],
  );
  assert_not_contains(
    &server_types,
    "impl TryFrom<Nested>",
    "a union a JSON type also holds isn't a frame type",
  );
}

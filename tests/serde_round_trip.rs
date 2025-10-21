//! Integration test for serde round-trip serialization/deserialization
//!
//! This test generates code from OpenAPI specs and verifies that the generated
//! types correctly serialize and deserialize with serde, maintaining symmetry.

use std::{fs, process::Command};

/// Test that discriminated unions (with #[serde(tag)]) work correctly
#[test]
fn test_discriminated_union_round_trip() {
  let spec_json = r#"{
  "openapi": "3.0.0",
  "info": {
    "title": "Test API",
    "version": "1.0.0"
  },
  "paths": {},
  "components": {
    "schemas": {
      "Message": {
        "oneOf": [
          {
            "type": "object",
            "title": "TextMessage",
            "required": ["type", "content"],
            "properties": {
              "type": {
                "type": "string",
                "const": "text"
              },
              "content": {
                "type": "string"
              }
            }
          },
          {
            "type": "string",
            "title": "SimpleString"
          },
          {
            "type": "integer",
            "title": "Number"
          }
        ],
        "discriminator": {
          "propertyName": "type"
        }
      }
    }
  }
}"#;

  // Generate code and verify it has correct structure
  let temp_dir = tempfile::tempdir().unwrap();
  let spec_path = temp_dir.path().join("spec.json");
  let output_path = temp_dir.path().join("generated.rs");

  fs::write(&spec_path, spec_json).unwrap();

  // Run the generator
  let status = Command::new("cargo")
    .args(&[
      "run",
      "--",
      "-i",
      spec_path.to_str().unwrap(),
      "-o",
      output_path.to_str().unwrap(),
    ])
    .status()
    .unwrap();

  assert!(status.success(), "Code generation failed");

  // Read generated code
  let generated_code = fs::read_to_string(&output_path).unwrap();

  // Verify discriminated enum uses struct variants
  assert!(
    generated_code.contains("#[serde(tag = \"type\")]"),
    "Should have serde tag attribute"
  );
  assert!(
    generated_code.contains("TextMessage { content: String }"),
    "TextMessage should be a struct variant"
  );
  assert!(
    generated_code.contains("SimpleString { value: String }"),
    "SimpleString should be a struct variant with 'value' field"
  );
  assert!(
    generated_code.contains("Number { value: i64 }"),
    "Number should be a struct variant with 'value' field"
  );
  assert!(
    !generated_code.contains("SimpleString(String)"),
    "Should NOT have tuple variants (not compatible with serde tag)"
  );

  println!("✓ Discriminated union generated correctly with struct variants only");
}

/// Test that catch-all enums use two-level structure
#[test]
fn test_catch_all_enum_round_trip() {
  let spec_json = r#"{
  "openapi": "3.0.0",
  "info": {
    "title": "Test API",
    "version": "1.0.0"
  },
  "paths": {},
  "components": {
    "schemas": {
      "Model": {
        "anyOf": [
          {
            "type": "string",
            "const": "gpt-4",
            "description": "GPT-4 model"
          },
          {
            "type": "string",
            "const": "gpt-3.5-turbo",
            "description": "GPT-3.5 Turbo model"
          },
          {
            "type": "string",
            "description": "Any other model string"
          }
        ]
      }
    }
  }
}"#;

  let temp_dir = tempfile::tempdir().unwrap();
  let spec_path = temp_dir.path().join("spec.json");
  let output_path = temp_dir.path().join("generated.rs");

  fs::write(&spec_path, spec_json).unwrap();

  let status = Command::new("cargo")
    .args(&[
      "run",
      "--",
      "-i",
      spec_path.to_str().unwrap(),
      "-o",
      output_path.to_str().unwrap(),
    ])
    .status()
    .unwrap();

  assert!(status.success(), "Code generation failed");

  let generated_code = fs::read_to_string(&output_path).unwrap();

  // Verify two-level structure
  assert!(
    generated_code.contains("pub(crate) enum ModelKnown"),
    "Should have inner Known enum"
  );
  assert!(
    generated_code.contains("pub(crate) enum Model"),
    "Should have outer enum"
  );
  assert!(
    generated_code.contains("#[serde(rename = \"gpt-4\")]"),
    "Inner enum should have renamed variants"
  );
  assert!(
    generated_code.contains("#[serde(untagged)]"),
    "Outer enum should be untagged"
  );
  assert!(
    generated_code.contains("Known(ModelKnown)"),
    "Outer enum should have Known variant wrapping inner enum"
  );
  assert!(
    generated_code.contains("Other(String)"),
    "Outer enum should have Other variant for unknown strings"
  );

  // Verify untagged is at ENUM level, not variant level
  let lines: Vec<&str> = generated_code.lines().collect();
  for (i, line) in lines.iter().enumerate() {
    if line.contains("#[serde(untagged)]") {
      // Next non-comment, non-attribute line should be "pub(crate) enum Model"
      for j in (i + 1)..lines.len() {
        let next_line = lines[j].trim();
        if !next_line.is_empty() && !next_line.starts_with("//") && !next_line.starts_with("#[") {
          assert!(
            next_line.contains("pub(crate) enum Model"),
            "untagged should be at enum level, not variant level"
          );
          break;
        }
      }
    }
  }

  println!("✓ Catch-all enum generated correctly with two-level structure");
}

/// Test that nullable patterns are detected correctly
#[test]
fn test_nullable_pattern() {
  let spec_json = r#"{
  "openapi": "3.0.0",
  "info": {
    "title": "Test API",
    "version": "1.0.0"
  },
  "paths": {},
  "components": {
    "schemas": {
      "User": {
        "type": "object",
        "properties": {
          "name": {
            "anyOf": [
              {"type": "string"},
              {"type": "null"}
            ]
          }
        }
      }
    }
  }
}"#;

  let temp_dir = tempfile::tempdir().unwrap();
  let spec_path = temp_dir.path().join("spec.json");
  let output_path = temp_dir.path().join("generated.rs");

  fs::write(&spec_path, spec_json).unwrap();

  let status = Command::new("cargo")
    .args(&[
      "run",
      "--",
      "-i",
      spec_path.to_str().unwrap(),
      "-o",
      output_path.to_str().unwrap(),
    ])
    .status()
    .unwrap();

  assert!(status.success(), "Code generation failed");

  let generated_code = fs::read_to_string(&output_path).unwrap();

  // Should generate Option<String>, not an enum
  assert!(
    generated_code.contains("pub(crate) name: Option<String>"),
    "Nullable pattern should be converted to Option<String>"
  );
  assert!(
    !generated_code.contains("pub(crate) enum Name"),
    "Should not generate an enum for simple nullable pattern"
  );

  println!("✓ Nullable pattern detected and converted to Option<T>");
}

/// Test that message stream events are generated with correct serde attributes
/// This tests the actual Anthropic API event stream format
#[test]
fn test_message_stream_event_deserialization() {
  let spec_json = r##"{
  "openapi": "3.0.0",
  "info": {
    "title": "Test API",
    "version": "1.0.0"
  },
  "paths": {},
  "components": {
    "schemas": {
      "MessageStreamEvent": {
        "oneOf": [
          {"$ref": "#/components/schemas/MessageStartEvent"},
          {"$ref": "#/components/schemas/ContentBlockStartEvent"},
          {"$ref": "#/components/schemas/ContentBlockDeltaEvent"},
          {"$ref": "#/components/schemas/ContentBlockStopEvent"},
          {"$ref": "#/components/schemas/MessageStopEvent"}
        ],
        "discriminator": {
          "propertyName": "type",
          "mapping": {
            "message_start": "#/components/schemas/MessageStartEvent",
            "content_block_start": "#/components/schemas/ContentBlockStartEvent",
            "content_block_delta": "#/components/schemas/ContentBlockDeltaEvent",
            "content_block_stop": "#/components/schemas/ContentBlockStopEvent",
            "message_stop": "#/components/schemas/MessageStopEvent"
          }
        }
      },
      "MessageStartEvent": {
        "type": "object",
        "title": "MessageStartEvent",
        "required": ["type", "message"],
        "properties": {
          "type": {"type": "string", "const": "message_start"},
          "message": {"$ref": "#/components/schemas/Message"}
        }
      },
      "ContentBlockStartEvent": {
        "type": "object",
        "title": "ContentBlockStartEvent",
        "required": ["type", "index", "content_block"],
        "properties": {
          "type": {"type": "string", "const": "content_block_start"},
          "index": {"type": "integer"},
          "content_block": {"$ref": "#/components/schemas/ContentBlock"}
        }
      },
      "ContentBlockDeltaEvent": {
        "type": "object",
        "title": "ContentBlockDeltaEvent",
        "required": ["type", "index", "delta"],
        "properties": {
          "type": {"type": "string", "const": "content_block_delta"},
          "index": {"type": "integer"},
          "delta": {"$ref": "#/components/schemas/Delta"}
        }
      },
      "ContentBlockStopEvent": {
        "type": "object",
        "title": "ContentBlockStopEvent",
        "required": ["type", "index"],
        "properties": {
          "type": {"type": "string", "const": "content_block_stop"},
          "index": {"type": "integer"}
        }
      },
      "MessageStopEvent": {
        "type": "object",
        "title": "MessageStopEvent",
        "required": ["type"],
        "properties": {
          "type": {"type": "string", "const": "message_stop"}
        }
      },
      "Message": {
        "type": "object",
        "required": ["id", "role"],
        "properties": {
          "id": {"type": "string"},
          "role": {"type": "string"}
        }
      },
      "ContentBlock": {
        "oneOf": [
          {"$ref": "#/components/schemas/TextBlock"}
        ],
        "discriminator": {
          "propertyName": "type",
          "mapping": {
            "text": "#/components/schemas/TextBlock"
          }
        }
      },
      "TextBlock": {
        "type": "object",
        "title": "TextBlock",
        "required": ["type", "text"],
        "properties": {
          "type": {"type": "string", "const": "text"},
          "text": {"type": "string"}
        }
      },
      "Delta": {
        "oneOf": [
          {"$ref": "#/components/schemas/TextDelta"}
        ],
        "discriminator": {
          "propertyName": "type",
          "mapping": {
            "text_delta": "#/components/schemas/TextDelta"
          }
        }
      },
      "TextDelta": {
        "type": "object",
        "title": "TextDelta",
        "required": ["type", "text"],
        "properties": {
          "type": {"type": "string", "const": "text_delta"},
          "text": {"type": "string"}
        }
      }
    }
  }
}"##;

  let temp_dir = tempfile::tempdir().unwrap();
  let spec_path = temp_dir.path().join("spec.json");
  let output_path = temp_dir.path().join("generated.rs");

  fs::write(&spec_path, spec_json).unwrap();

  let status = Command::new("cargo")
    .args(&[
      "run",
      "--",
      "-i",
      spec_path.to_str().unwrap(),
      "-o",
      output_path.to_str().unwrap(),
    ])
    .status()
    .unwrap();

  assert!(status.success(), "Code generation failed");

  let generated_code = fs::read_to_string(&output_path).unwrap();

  // Verify MessageStreamEvent enum has correct structure
  assert!(
    generated_code.contains("pub(crate) enum MessageStreamEvent"),
    "Should generate MessageStreamEvent enum"
  );
  assert!(
    generated_code.contains("#[serde(tag = \"type\")]"),
    "Should have serde tag attribute for discriminated union"
  );

  // Verify variant names are stripped (clippy-compliant)
  assert!(
    generated_code.contains("MessageStart {"),
    "Variant names should be stripped: MessageStartEvent -> MessageStart"
  );
  assert!(
    generated_code.contains("ContentBlockStart {"),
    "Variant names should be stripped: ContentBlockStartEvent -> ContentBlockStart"
  );
  assert!(
    generated_code.contains("ContentBlockDelta {"),
    "Variant names should be stripped"
  );
  assert!(
    generated_code.contains("ContentBlockStop {"),
    "Variant names should be stripped"
  );
  assert!(
    generated_code.contains("MessageStop {"),
    "Variant names should be stripped"
  );

  // Verify serde(rename) attributes map to snake_case discriminator values
  assert!(
    generated_code.contains("#[serde(rename = \"message_start\")]"),
    "Should have serde(rename) for message_start to match JSON discriminator"
  );
  assert!(
    generated_code.contains("#[serde(rename = \"content_block_start\")]"),
    "Should have serde(rename) for content_block_start"
  );
  assert!(
    generated_code.contains("#[serde(rename = \"content_block_delta\")]"),
    "Should have serde(rename) for content_block_delta"
  );
  assert!(
    generated_code.contains("#[serde(rename = \"content_block_stop\")]"),
    "Should have serde(rename) for content_block_stop"
  );
  assert!(
    generated_code.contains("#[serde(rename = \"message_stop\")]"),
    "Should have serde(rename) for message_stop"
  );

  // Verify nested discriminated unions also work correctly
  assert!(
    generated_code.contains("pub(crate) enum Delta"),
    "Should generate nested Delta enum"
  );
  assert!(
    generated_code.contains("#[serde(rename = \"text_delta\")]"),
    "Nested enum should have serde(rename) for text_delta"
  );

  // Verify ContentBlock enum
  assert!(
    generated_code.contains("pub(crate) enum ContentBlock"),
    "Should generate ContentBlock enum"
  );
  assert!(
    generated_code.contains("#[serde(rename = \"text\")]"),
    "ContentBlock should have serde(rename) for text"
  );

  println!("✓ Message stream event generated with correct serde attributes for Anthropic API format");
  println!("  - Discriminated unions use #[serde(tag = \"type\")]");
  println!("  - Variant names are stripped (clippy-compliant)");
  println!("  - serde(rename) attributes match snake_case JSON discriminators");
}

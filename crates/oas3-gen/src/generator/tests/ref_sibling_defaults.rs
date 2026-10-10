use super::support::{field_attributes, generate_types, make_orchestrator};
use crate::utils::spec::SpecFormat;

fn struct_body<'a>(code: &'a str, name: &str) -> &'a str {
  let start = code
    .find(&format!("pub struct {name} {{"))
    .unwrap_or_else(|| panic!("missing struct {name}"));
  let end = start + code[start..].find("\n}").expect("struct closes");
  &code[start..end]
}

#[test]
fn union_reference_defaults_render_as_declared() {
  let spec_json = r##"{
    "openapi": "3.1.0",
    "info": { "title": "Union reference defaults", "version": "1" },
    "paths": {},
    "components": {
      "schemas": {
        "Mode": { "oneOf": [{ "const": "slow" }, { "const": "fast" }] },
        "OpenSize": { "anyOf": [{ "enum": ["small", "large"] }, { "type": "string" }] },
        "PaceEnum": { "type": "string", "enum": ["walk", "run"] },
        "Pace": { "anyOf": [{ "$ref": "#/components/schemas/PaceEnum" }, { "type": "integer" }] },
        "Holder": {
          "type": "object",
          "properties": {
            "mode": { "$ref": "#/components/schemas/Mode", "default": "slow" },
            "size": { "$ref": "#/components/schemas/OpenSize", "default": "large" },
            "other_size": { "$ref": "#/components/schemas/OpenSize", "default": "medium" },
            "pace": { "$ref": "#/components/schemas/Pace", "default": "run" },
            "pace_count": { "$ref": "#/components/schemas/Pace", "default": 3 }
          }
        }
      }
    }
  }"##;
  let cases = [
    ("mode", "#[default(Some(Mode::Slow))]"),
    ("size", "#[default(Some(OpenSize::Large))]"),
    (
      "other_size",
      "#[default(Some(OpenSize::Other(\"medium\".to_string())))]",
    ),
    ("pace", "#[default(Some(Pace::PaceEnum(PaceEnum::Run)))]"),
    ("pace_count", "#[default(Some(Pace::Integer(3i64)))]"),
  ];

  let spec = SpecFormat::Json
    .parse(spec_json.as_bytes())
    .expect("failed to parse test spec");
  let orchestrator = make_orchestrator(spec, true);
  let output = generate_types(&orchestrator, "test.json");
  let holder = struct_body(&output.code, "Holder");

  for (field, expected) in cases {
    assert!(
      holder.contains(expected),
      "field `{field}` should render `{expected}`, got:\n{holder}"
    );
  }
}

#[test]
fn defaults_render_by_target_type() {
  let spec_json = r##"{
    "openapi": "3.1.0",
    "info": { "title": "Defaults by target type", "version": "1" },
    "paths": {},
    "components": {
      "schemas": {
        "Circle": { "type": "object", "properties": { "r": { "type": "number" } } },
        "Square": { "type": "object", "properties": { "s": { "type": "number" } } },
        "Shape": { "oneOf": [{ "$ref": "#/components/schemas/Circle" }, { "$ref": "#/components/schemas/Square" }] },
        "Level": { "type": "string", "enum": ["1.0", "2.0"] },
        "Drawing": {
          "type": "object",
          "properties": {
            "config": {
              "type": "object",
              "properties": { "n": { "type": "integer" } },
              "default": {}
            },
            "inline_shape": {
              "oneOf": [{ "$ref": "#/components/schemas/Circle" }, { "$ref": "#/components/schemas/Square" }],
              "default": { "s": 2 }
            },
            "explicit_shape": {
              "allOf": [{ "$ref": "#/components/schemas/Shape" }],
              "default": { "s": 2 }
            },
            "level": { "$ref": "#/components/schemas/Level", "default": "3.0" },
            "shape": { "$ref": "#/components/schemas/Shape", "default": { "s": 2 } }
          }
        }
      }
    }
  }"##;
  let cases = [
    ("config", Some("#[default(Some(Default::default()))]")),
    ("inline_shape", None),
    ("explicit_shape", None),
    ("level", None),
    ("shape", None),
  ];

  let spec = SpecFormat::Json
    .parse(spec_json.as_bytes())
    .expect("failed to parse test spec");
  let orchestrator = make_orchestrator(spec, true);
  let output = generate_types(&orchestrator, "test.json");
  let drawing = struct_body(&output.code, "Drawing");

  for (field, expected) in cases {
    let default = field_attributes(drawing, field)
      .lines()
      .map(str::trim)
      .find(|line| line.starts_with("#[default("));
    assert_eq!(default, expected, "default attribute of `{field}`, got:\n{drawing}");
  }
}

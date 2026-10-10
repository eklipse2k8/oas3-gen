use std::{
  collections::{BTreeMap, HashSet},
  ffi::OsStr,
  fmt::Write as _,
  path::Path,
};

use fmmap::tokio::{AsyncMmapFile, AsyncMmapFileExt};
use mediatype::MediaType;
use oas3::{
  OpenApiV3Spec,
  spec::{Operation, PathItem, SecurityRequirement},
};
use serde::{
  Deserialize,
  de::{DeserializeOwned, IgnoredAny},
};
use serde_json::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SpecFormat {
  #[default]
  Json,
  Yaml,
}

impl SpecFormat {
  #[must_use]
  pub fn from_extension(ext: &str) -> Self {
    match ext {
      "yaml" | "yml" => Self::Yaml,
      _ => Self::Json,
    }
  }

  /// Parses a document, keeping the operations of path items that reference
  /// `components.pathItems`, the operations that clear the top-level `security` with
  /// an empty array public, and the `default` a property sets next to its `$ref`.
  pub fn parse(self, content: &[u8]) -> anyhow::Result<oas3::Spec> {
    let mut document = self.deserialize::<Value>(content)?;
    let content_schemas = SseContentSchemas::collect(&document, content);
    for (pointer, schema) in &content_schemas {
      if let Some(property) = document.pointer_mut(pointer) {
        property.clone_from(schema);
      }
    }
    let inlined = PathItemRefs::inline(&mut document);
    let lifted = RefSiblingDefaults::lift(&mut document);
    let overrides = SecurityOverrides::deserialize(&document)?;
    let mut spec = if inlined || lifted || !content_schemas.is_empty() {
      serde_json::from_value::<OpenApiV3Spec>(document)?
    } else {
      self.deserialize::<OpenApiV3Spec>(content)?
    };
    if !spec.security.is_empty() {
      overrides.apply(&mut spec);
    }
    Ok(spec)
  }

  fn deserialize<T: DeserializeOwned>(self, content: &[u8]) -> anyhow::Result<T> {
    match self {
      Self::Json => Ok(serde_json::from_slice(content)?),
      Self::Yaml => Ok(yaml_serde::from_str(std::str::from_utf8(content)?)?),
    }
  }
}

pub struct SpecLoader {
  file: AsyncMmapFile,
  format: SpecFormat,
}

impl SpecLoader {
  pub async fn open(path: &Path) -> anyhow::Result<Self> {
    let format = path
      .extension()
      .and_then(OsStr::to_str)
      .map_or(SpecFormat::default(), SpecFormat::from_extension);

    let file = unsafe { AsyncMmapFile::open(path).await? };

    Ok(Self { file, format })
  }

  pub fn parse(&self) -> anyhow::Result<oas3::Spec> {
    self.format.parse(self.file.as_slice())
  }
}

/// The JSON pointer to the node a document walk is visiting, kept in one buffer that
/// grows and shrinks with the walk.
#[derive(Default)]
struct PointerBuf(String);

impl PointerBuf {
  /// Appends `key` as an escaped segment, returning the length to truncate back to.
  fn push(&mut self, key: &str) -> usize {
    let mark = self.0.len();
    self.0.push('/');
    for ch in key.chars() {
      match ch {
        '~' => self.0.push_str("~0"),
        '/' => self.0.push_str("~1"),
        other => self.0.push(other),
      }
    }
    mark
  }

  /// Appends an array index segment, returning the length to truncate back to.
  fn push_index(&mut self, index: usize) -> usize {
    let mark = self.0.len();
    let _ = write!(self.0, "/{index}");
    mark
  }

  fn truncate(&mut self, mark: usize) {
    self.0.truncate(mark);
  }

  fn as_str(&self) -> &str {
    &self.0
  }
}

/// The frame fields a `text/event-stream` frame type draws its properties from.
pub(crate) const SSE_FRAME_FIELDS: [&str; 4] = ["data", "event", "id", "retry"];

/// Whether properties named `names` make a `text/event-stream` frame type: every one a
/// frame field, `data` among them.
pub(crate) fn is_frame_type<'a>(names: impl IntoIterator<Item = &'a str>) -> bool {
  let mut has_data = false;
  for name in names {
    if !SSE_FRAME_FIELDS.contains(&name) {
      return false;
    }
    has_data |= name == "data";
  }
  has_data
}

/// Entries of `paths` and `webhooks` in the raw document that reference a
/// `components.pathItems` entry.
///
/// `oas3` keeps the `$ref` of a referenced Path Item but none of the operations it
/// points to, so those operations would never be generated. [`PathItemRefs::inline`]
/// replaces each reference with the component it names, following chains of
/// references. Fields set next to the `$ref` win over the component's.
struct PathItemRefs;

impl PathItemRefs {
  const PREFIX: &'static str = "#/components/pathItems/";

  /// Inlines every referenced Path Item, returning `true` when any changed.
  fn inline(document: &mut Value) -> bool {
    let Some(components) = document
      .pointer("/components/pathItems")
      .and_then(Value::as_object)
      .cloned()
    else {
      return false;
    };

    let mut inlined = false;
    for section in ["paths", "webhooks"] {
      let Some(items) = document.get_mut(section).and_then(Value::as_object_mut) else {
        continue;
      };
      for item in items.values_mut() {
        if let Some(resolved) = Self::resolve(item, &components) {
          *item = resolved;
          inlined = true;
        }
      }
    }
    inlined
  }

  /// Merges the components `item` references into it, or returns `None` when it
  /// references none, names a missing component, or loops back on itself.
  fn resolve(item: &Value, components: &serde_json::Map<String, Value>) -> Option<Value> {
    item.get("$ref")?;
    let mut fields = item.as_object()?.clone();
    let mut visited = HashSet::new();
    while let Some(name) = fields
      .get("$ref")
      .and_then(Value::as_str)
      .and_then(|reference| reference.strip_prefix(Self::PREFIX))
      .map(|name| name.replace("~1", "/").replace("~0", "~"))
    {
      let target = components.get(&name)?.as_object()?;
      if !visited.insert(name) {
        return None;
      }
      fields.remove("$ref");
      for (key, value) in target {
        fields.entry(key.clone()).or_insert_with(|| value.clone());
      }
    }
    (!visited.is_empty()).then_some(Value::Object(fields))
  }
}

/// Property schemas in the raw document that set a `default` next to their `$ref`.
///
/// `oas3` keeps only `$ref`, `summary`, and `description` from a reference, so the
/// `default` would be lost. [`RefSiblingDefaults::lift`] rewrites each such property
/// into `{ "allOf": [{ "$ref": ... }], "default": ... }`, which survives parsing. It
/// only lifts references to `enum`, `const`, and union components, whose variants
/// the default can name.
struct RefSiblingDefaults {
  targets: HashSet<String>,
  lifted: bool,
}

impl RefSiblingDefaults {
  /// Keywords whose values map names to schemas, so every value is visited as a schema.
  const SCHEMA_MAPS: [&'static str; 5] = ["schemas", "properties", "patternProperties", "$defs", "definitions"];
  /// Keywords whose values are example or constraint data, never schemas.
  const DATA_KEYWORDS: [&'static str; 4] = ["example", "examples", "enum", "const"];

  /// Rewrites the properties of `document`, returning `true` when any changed.
  fn lift(document: &mut Value) -> bool {
    let targets = document
      .pointer("/components/schemas")
      .and_then(Value::as_object)
      .into_iter()
      .flatten()
      .filter(|(_, schema)| {
        ["enum", "const", "anyOf", "oneOf"]
          .iter()
          .any(|keyword| schema.get(keyword).is_some())
      })
      .map(|(name, _)| format!("#/components/schemas/{}", name.replace('~', "~0").replace('/', "~1")))
      .collect();

    let mut pass = Self { targets, lifted: false };
    if !pass.targets.is_empty() {
      pass.visit(document);
    }
    pass.lifted
  }

  fn visit(&mut self, value: &mut Value) {
    match value {
      Value::Object(map) => {
        for (keyword, child) in map.iter_mut() {
          if Self::SCHEMA_MAPS.contains(&keyword.as_str())
            && let Some(schemas) = child.as_object_mut()
          {
            for schema in schemas.values_mut() {
              if keyword == "properties" {
                self.lift_property(schema);
              }
              self.visit(schema);
            }
          } else if !keyword.starts_with("x-") && !Self::DATA_KEYWORDS.contains(&keyword.as_str()) {
            self.visit(child);
          }
        }
      }
      Value::Array(items) => items.iter_mut().for_each(|item| self.visit(item)),
      _ => {}
    }
  }

  fn lift_property(&mut self, property: &mut Value) {
    let Some(schema) = property.as_object_mut() else {
      return;
    };
    let is_lifted = schema.contains_key("default")
      && schema
        .get("$ref")
        .and_then(Value::as_str)
        .is_some_and(|target| self.targets.contains(target));
    if is_lifted && let Some(reference) = schema.remove("$ref") {
      schema.insert(
        "allOf".to_string(),
        Value::Array(vec![serde_json::json!({ "$ref": reference })]),
      );
      self.lifted = true;
    }
  }
}

/// `text/event-stream` frame types whose `data` carries JSON in a string the OpenAPI 3.2
/// way: `{ "type": "string", "contentMediaType": "application/json", "contentSchema": ... }`.
///
/// `oas3` drops `contentMediaType` and `contentSchema`, so the data would read as plain
/// text. Each such property is replaced with its `contentSchema`, which the generated
/// client then decodes from the frame's data as JSON. The stream schema is a frame type
/// itself, or a `oneOf` or `anyOf` of frame types keyed by their `event`.
struct SseContentSchemas<'a> {
  document: &'a Value,
  pointer: PointerBuf,
  lifts: BTreeMap<String, Value>,
}

impl<'a> SseContentSchemas<'a> {
  /// The replacements `document` calls for. The walk only runs when its text, `content`,
  /// mentions a `contentSchema` at all.
  fn collect(document: &'a Value, content: &[u8]) -> BTreeMap<String, Value> {
    const KEYWORD: &[u8] = b"contentSchema";
    if !content.windows(KEYWORD.len()).any(|window| window == KEYWORD) {
      return BTreeMap::new();
    }
    let mut pass = Self {
      document,
      pointer: PointerBuf::default(),
      lifts: BTreeMap::new(),
    };
    pass.visit(document);
    pass.lifts
  }

  fn visit(&mut self, value: &'a Value) {
    match value {
      Value::Object(map) => {
        for (key, child) in map {
          let mark = self.pointer.push(key);
          if is_event_stream(key)
            && let Some(schema) = child.get("schema")
          {
            self.pointer.push("schema");
            let pointer = self.pointer.as_str().to_string();
            self.stream_schema(schema, pointer);
          } else if !key.starts_with("x-") && !matches!(key.as_str(), "example" | "examples") {
            self.visit(child);
          }
          self.pointer.truncate(mark);
        }
      }
      Value::Array(items) => {
        for (index, item) in items.iter().enumerate() {
          let mark = self.pointer.push_index(index);
          self.visit(item);
          self.pointer.truncate(mark);
        }
      }
      _ => {}
    }
  }

  fn stream_schema(&mut self, schema: &'a Value, pointer: String) {
    let (schema, pointer) = self.resolve(schema, pointer);
    if is_frame_type_schema(schema) {
      self.lift_data(schema, &pointer);
      return;
    }
    let keyed_by_event = schema.pointer("/discriminator/propertyName").and_then(Value::as_str) == Some("event");
    for keyword in ["oneOf", "anyOf"] {
      let Some(members) = schema.get(keyword).and_then(Value::as_array) else {
        continue;
      };
      let members = members
        .iter()
        .enumerate()
        .map(|(index, member)| self.resolve(member, format!("{pointer}/{keyword}/{index}")))
        .collect::<Vec<_>>();
      let frame_types = members
        .iter()
        .all(|(member, _)| is_frame_type_schema(member) && (keyed_by_event || fixes_its_event(member)));
      if frame_types {
        for (member, member_pointer) in &members {
          self.lift_data(member, member_pointer);
        }
      }
    }
  }

  /// Follows a `$ref` to a `components.schemas` entry, returning the schema and its pointer.
  fn resolve(&self, schema: &'a Value, pointer: String) -> (&'a Value, String) {
    let Some(target) = schema
      .get("$ref")
      .and_then(Value::as_str)
      .and_then(|reference| reference.strip_prefix('#'))
    else {
      return (schema, pointer);
    };
    match self.document.pointer(target) {
      Some(resolved) => (resolved, target.to_string()),
      None => (schema, pointer),
    }
  }

  fn lift_data(&mut self, frame_type: &Value, pointer: &str) {
    let Some(data) = frame_type.pointer("/properties/data") else {
      return;
    };
    let json = data
      .get("contentMediaType")
      .and_then(Value::as_str)
      .and_then(|media_type| MediaType::parse(media_type).ok())
      .is_some_and(|media_type| {
        media_type.subty.as_str().eq_ignore_ascii_case("json")
          || media_type
            .suffix
            .is_some_and(|suffix| suffix.as_str().eq_ignore_ascii_case("json"))
      });
    if json && let Some(content_schema) = data.get("contentSchema") {
      self
        .lifts
        .insert(format!("{pointer}/properties/data"), content_schema.clone());
    }
  }
}

/// Whether `key` names the `text/event-stream` media type, parameters aside.
fn is_event_stream(key: &str) -> bool {
  MediaType::parse(key).is_ok_and(|media_type| {
    media_type.ty.as_str().eq_ignore_ascii_case("text")
      && media_type.subty.as_str().eq_ignore_ascii_case("event-stream")
  })
}

fn is_frame_type_schema(schema: &Value) -> bool {
  schema
    .get("properties")
    .and_then(Value::as_object)
    .is_some_and(|properties| is_frame_type(properties.keys().map(String::as_str)))
}

/// Whether a union member fixes its `event` to one value, which keys the union by it.
fn fixes_its_event(member: &Value) -> bool {
  member.pointer("/properties/event").is_some_and(|event| {
    event.get("const").is_some()
      || event
        .get("enum")
        .and_then(Value::as_array)
        .is_some_and(|values| values.len() == 1)
  })
}

/// The `security` arrays operations declare in the raw document.
///
/// `oas3` parses `"security": []` like a missing `security`, so the operation would
/// inherit the top-level requirement. [`SecurityOverrides::apply`] rewrites those
/// operations to the equivalent `[{}]`, which survives parsing.
#[derive(Deserialize)]
struct SecurityOverrides {
  #[serde(default)]
  paths: BTreeMap<String, PathItemSecurity>,
  #[serde(default)]
  webhooks: BTreeMap<String, PathItemSecurity>,
}

#[derive(Deserialize)]
struct PathItemSecurity {
  get: Option<OperationSecurity>,
  put: Option<OperationSecurity>,
  post: Option<OperationSecurity>,
  delete: Option<OperationSecurity>,
  options: Option<OperationSecurity>,
  head: Option<OperationSecurity>,
  patch: Option<OperationSecurity>,
  trace: Option<OperationSecurity>,
}

#[derive(Deserialize)]
struct OperationSecurity {
  security: Option<Vec<IgnoredAny>>,
}

impl SecurityOverrides {
  fn apply(self, spec: &mut oas3::Spec) {
    let groups = [
      (self.paths, spec.paths.as_mut()),
      (self.webhooks, Some(&mut spec.webhooks)),
    ];
    for (overrides, items) in groups {
      let Some(items) = items else {
        continue;
      };
      for (name, overrides) in overrides {
        let Some(item) = items.get_mut(&name) else {
          continue;
        };
        for (security, operation) in overrides.into_operations().into_iter().zip(operations_mut(item)) {
          if let (
            Some(OperationSecurity {
              security: Some(security),
            }),
            Some(operation),
          ) = (security, operation)
            && security.is_empty()
          {
            operation.security = vec![SecurityRequirement(oas3::Map::new())];
          }
        }
      }
    }
  }
}

impl PathItemSecurity {
  fn into_operations(self) -> [Option<OperationSecurity>; 8] {
    [
      self.get,
      self.put,
      self.post,
      self.delete,
      self.options,
      self.head,
      self.patch,
      self.trace,
    ]
  }
}

fn operations_mut(item: &mut PathItem) -> [&mut Option<Operation>; 8] {
  [
    &mut item.get,
    &mut item.put,
    &mut item.post,
    &mut item.delete,
    &mut item.options,
    &mut item.head,
    &mut item.patch,
    &mut item.trace,
  ]
}

#[cfg(test)]
mod tests {
  use super::SpecFormat;
  use crate::utils::SchemaInspect;

  #[test]
  fn ref_sibling_defaults_survive_parsing() {
    let json = r##"{
      "openapi": "3.1.0",
      "info": { "title": "t", "version": "1" },
      "components": {
        "schemas": {
          "Closed": { "type": "string", "enum": ["a", "b"] },
          "Cfg": { "type": "object", "properties": { "n": { "type": "integer" } } },
          "Holder": {
            "type": "object",
            "example": { "properties": { "x": { "$ref": "#/components/schemas/Closed", "default": "a" } } },
            "properties": {
              "lifted": { "$ref": "#/components/schemas/Closed", "default": "a", "description": "kept" },
              "struct": { "$ref": "#/components/schemas/Cfg", "default": {} },
              "plain": { "$ref": "#/components/schemas/Closed" }
            }
          }
        }
      }
    }"##;
    let yaml = "
openapi: 3.1.0
info: { title: t, version: '1' }
components:
  schemas:
    Closed: { type: string, enum: [a, b] }
    Cfg: { type: object, properties: { n: { type: integer } } }
    Holder:
      type: object
      example: { properties: { x: { $ref: '#/components/schemas/Closed', default: a } } }
      properties:
        lifted: { $ref: '#/components/schemas/Closed', default: a, description: kept }
        struct: { $ref: '#/components/schemas/Cfg', default: {} }
        plain: { $ref: '#/components/schemas/Closed' }
";

    for (format, content) in [(SpecFormat::Json, json), (SpecFormat::Yaml, yaml)] {
      let spec = format.parse(content.as_bytes()).expect("spec parses");
      let schemas = &spec.components.as_ref().expect("components").schemas;
      let holder = schemas
        .get("Holder")
        .and_then(SchemaInspect::as_inline)
        .expect("Holder should be inline");

      let lifted = holder
        .properties
        .get("lifted")
        .and_then(SchemaInspect::as_inline)
        .unwrap_or_else(|| panic!("{format:?}: a `$ref` with a `default` should become an inline wrapper"));
      assert_eq!(
        lifted.default,
        Some(serde_json::json!("a")),
        "{format:?}: lifted default"
      );
      assert_eq!(
        lifted.description.as_deref(),
        Some("kept"),
        "{format:?}: lifted description"
      );
      assert_eq!(lifted.all_of.len(), 1, "{format:?}: lifted wraps the reference");

      for unchanged in ["struct", "plain"] {
        assert!(
          holder
            .properties
            .get(unchanged)
            .and_then(SchemaInspect::ref_path)
            .is_some(),
          "{format:?}: {unchanged} should stay a reference"
        );
      }
      assert_eq!(
        holder.example.as_ref().and_then(|e| e.pointer("/properties/x/$ref")),
        Some(&serde_json::json!("#/components/schemas/Closed")),
        "{format:?}: example data should be untouched"
      );
    }
  }

  #[test]
  fn empty_operation_security_clears_the_top_level_requirement() {
    let json = r#"{
      "openapi": "3.1.0",
      "info": { "title": "t", "version": "1" },
      "security": [{ "Key": [] }],
      "paths": {
        "/cleared": { "get": { "security": [], "responses": {} } },
        "/anonymous": { "get": { "security": [{}], "responses": {} } },
        "/inherited": { "get": { "responses": {} } },
        "/own": { "get": { "security": [{ "Other": [] }], "responses": {} } }
      }
    }"#;
    let yaml = "
openapi: 3.1.0
info: { title: t, version: '1' }
security: [{ Key: [] }]
paths:
  /cleared: { get: { security: [], responses: { 200: { description: ok } } } }
  /anonymous: { get: { security: [{}], responses: {} } }
  /inherited: { get: { responses: {} } }
  /own: { get: { security: [{ Other: [] }], responses: {} } }
";
    let cases = [
      ("/cleared", vec![vec![]]),
      ("/anonymous", vec![vec![]]),
      ("/inherited", vec![]),
      ("/own", vec![vec!["Other"]]),
    ];

    for (format, content) in [(SpecFormat::Json, json), (SpecFormat::Yaml, yaml)] {
      let spec = format.parse(content.as_bytes()).expect("spec parses");
      let paths = spec.paths.as_ref().expect("paths");
      for (path, expected) in &cases {
        let security = paths
          .get(*path)
          .and_then(|item| item.get.as_ref())
          .expect("operation")
          .security
          .iter()
          .map(|requirement| requirement.0.keys().map(String::as_str).collect::<Vec<_>>())
          .collect::<Vec<_>>();
        assert_eq!(&security, expected, "{format:?} security of {path}");
      }
    }
  }

  #[test]
  fn referenced_path_items_keep_their_operations() {
    let json = r##"{
      "openapi": "3.1.0",
      "info": { "title": "t", "version": "1" },
      "security": [{ "Key": [] }],
      "paths": {
        "/direct": { "$ref": "#/components/pathItems/Direct", "summary": "local" },
        "/chained": { "$ref": "#/components/pathItems/Alias" },
        "/missing": { "$ref": "#/components/pathItems/Missing" },
        "/looping": { "$ref": "#/components/pathItems/Loop" }
      },
      "webhooks": {
        "ping": { "$ref": "#/components/pathItems/Public" }
      },
      "components": {
        "pathItems": {
          "Direct": { "summary": "shared", "get": { "operationId": "readDirect", "responses": {} } },
          "Alias": { "$ref": "#/components/pathItems/Direct", "post": { "operationId": "writeAlias", "responses": {} } },
          "Public": { "post": { "operationId": "ping", "security": [], "responses": {} } },
          "Loop": { "$ref": "#/components/pathItems/Loop" }
        }
      }
    }"##;
    let yaml = "
openapi: 3.1.0
info: { title: t, version: '1' }
security: [{ Key: [] }]
paths:
  /direct: { $ref: '#/components/pathItems/Direct', summary: local }
  /chained: { $ref: '#/components/pathItems/Alias' }
  /missing: { $ref: '#/components/pathItems/Missing' }
  /looping: { $ref: '#/components/pathItems/Loop' }
webhooks:
  ping: { $ref: '#/components/pathItems/Public' }
components:
  pathItems:
    Direct: { summary: shared, get: { operationId: readDirect, responses: {} } }
    Alias: { $ref: '#/components/pathItems/Direct', post: { operationId: writeAlias, responses: {} } }
    Public: { post: { operationId: ping, security: [], responses: {} } }
    Loop: { $ref: '#/components/pathItems/Loop' }
";

    for (format, content) in [(SpecFormat::Json, json), (SpecFormat::Yaml, yaml)] {
      let spec = format.parse(content.as_bytes()).expect("spec parses");
      let paths = spec.paths.as_ref().expect("paths");
      let operation_id = |path: &str, pick: fn(&oas3::spec::PathItem) -> Option<&oas3::spec::Operation>| {
        paths
          .get(path)
          .and_then(pick)
          .and_then(|operation| operation.operation_id.clone())
      };

      let direct = paths.get("/direct").expect("/direct");
      assert_eq!(direct.reference, None, "{format:?}: /direct reference is inlined");
      assert_eq!(
        direct.summary.as_deref(),
        Some("local"),
        "{format:?}: a sibling field wins"
      );
      assert_eq!(
        operation_id("/direct", |item| item.get.as_ref()).as_deref(),
        Some("readDirect"),
        "{format:?}: /direct get"
      );
      assert_eq!(
        operation_id("/chained", |item| item.get.as_ref()).as_deref(),
        Some("readDirect"),
        "{format:?}: /chained follows the alias"
      );
      assert_eq!(
        operation_id("/chained", |item| item.post.as_ref()).as_deref(),
        Some("writeAlias"),
        "{format:?}: /chained keeps the alias's own operation"
      );
      for unresolved in ["/missing", "/looping"] {
        let item = paths.get(unresolved).expect(unresolved);
        assert!(item.reference.is_some(), "{format:?}: {unresolved} stays a reference");
        assert!(item.get.is_none(), "{format:?}: {unresolved} gains no operations");
      }

      let ping = spec
        .webhooks
        .get("ping")
        .and_then(|item| item.post.as_ref())
        .expect("webhook operation");
      assert_eq!(ping.operation_id.as_deref(), Some("ping"), "{format:?}: webhook");
      assert_eq!(
        ping
          .security
          .iter()
          .map(|requirement| requirement.0.len())
          .collect::<Vec<_>>(),
        vec![0],
        "{format:?}: an empty security array in a referenced item stays public"
      );
    }
  }

  #[test]
  fn event_stream_frame_types_decode_their_content_schema() {
    let json = r##"{
      "openapi": "3.1.0",
      "info": { "title": "t", "version": "1" },
      "paths": {
        "/stream": {
          "get": {
            "responses": {
              "200": {
                "description": "ok",
                "content": { "text/event-stream": { "schema": { "$ref": "#/components/schemas/Frame" } } }
              }
            }
          }
        },
        "/unkeyed": {
          "get": {
            "responses": {
              "200": {
                "description": "ok",
                "content": { "text/event-stream; charset=utf-8": { "schema": { "$ref": "#/components/schemas/Unkeyed" } } }
              }
            }
          }
        },
        "/document": {
          "get": {
            "responses": {
              "200": {
                "description": "ok",
                "content": { "application/json": { "schema": { "$ref": "#/components/schemas/Document" } } }
              }
            }
          }
        }
      },
      "components": {
        "schemas": {
          "Payload": { "type": "object", "properties": { "n": { "type": "integer" } } },
          "Frame": {
            "type": "object",
            "properties": {
              "event": { "type": "string" },
              "data": {
                "type": "string",
                "contentMediaType": "application/json; charset=utf-8",
                "contentSchema": { "$ref": "#/components/schemas/Payload" }
              }
            }
          },
          "Document": {
            "type": "object",
            "properties": {
              "data": {
                "type": "string",
                "contentMediaType": "application/json",
                "contentSchema": { "$ref": "#/components/schemas/Payload" }
              }
            }
          },
          "Unkeyed": { "oneOf": [{ "$ref": "#/components/schemas/Document" }, { "$ref": "#/components/schemas/Frame" }] }
        }
      }
    }"##;
    let spec = SpecFormat::Json.parse(json.as_bytes()).expect("spec parses");
    let data = |schema: &str| {
      spec
        .components
        .as_ref()
        .and_then(|components| components.schemas.get(schema))
        .and_then(SchemaInspect::as_inline)
        .and_then(|object| object.properties.get("data"))
        .cloned()
        .expect("the schema has data")
    };
    assert_eq!(
      data("Frame").ref_path(),
      Some("#/components/schemas/Payload"),
      "a frame type's data reads as its contentSchema"
    );
    assert_eq!(
      data("Document").ref_path(),
      None,
      "a JSON body, and a member of a stream union no event keys, keep their string"
    );
  }
}

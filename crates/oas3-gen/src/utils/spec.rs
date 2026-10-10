use std::{
  collections::{BTreeMap, HashSet},
  ffi::OsStr,
  path::Path,
};

use fmmap::tokio::{AsyncMmapFile, AsyncMmapFileExt};
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
    let inlined = PathItemRefs::inline(&mut document);
    let lifted = RefSiblingDefaults::lift(&mut document);
    let overrides = SecurityOverrides::deserialize(&document)?;
    let mut spec = if inlined || lifted {
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
}

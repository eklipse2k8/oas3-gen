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
  /// an empty array public, and the `default` a property sets next to a `$ref` to an
  /// enum or union component.
  ///
  /// The text is parsed into a [`Value`] to find these rewrites, then into the typed
  /// spec, which keeps located errors, rejects duplicate JSON keys, and reads each YAML
  /// scalar as written. Rewrites edit the typed spec through a [`Value`] round trip; a
  /// YAML document the typed parse rejects is rewritten from its [`Value`] instead.
  pub fn parse(self, content: &[u8]) -> anyhow::Result<oas3::Spec> {
    let mut document: Value = self.deserialize(content)?;
    let rewrite = Rewrite {
      lifts: RefSiblingDefaults::collect(&document),
      content_schemas: SseContentSchemas::collect(&document, content),
    };
    let path_item_components = sibling_path_item_components(&document);
    let inlined = PathItemRefs::inline(&mut document);
    let overrides = SecurityOverrides::read(&document)?;
    let rewrites = inlined || !rewrite.is_empty();
    drop(document);

    let mut spec = match self.deserialize::<OpenApiV3Spec>(content) {
      Ok(typed) if rewrites => rewrite_typed(typed, &rewrite, path_item_components)?,
      Ok(typed) => typed,
      Err(error) if rewrites && matches!(self, Self::Yaml) => {
        rewrite_document(self.deserialize::<Value>(content)?, &rewrite).map_err(|_| error)?
      }
      Err(error) => return Err(error),
    };
    if let Some(overrides) = overrides {
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

/// Rewrites the typed spec through its [`Value`] form. The typed spec reads each
/// `components.pathItems` entry as an `ObjectOrReference`, which drops the siblings of a
/// `$ref`, so those raw entries are put back before the rewrite.
fn rewrite_typed(
  typed: OpenApiV3Spec,
  rewrite: &Rewrite,
  path_item_components: Vec<(String, Value)>,
) -> anyhow::Result<OpenApiV3Spec> {
  let mut document = serde_json::to_value(typed)?;
  if let Some(components) = document
    .pointer_mut("/components/pathItems")
    .and_then(Value::as_object_mut)
  {
    components.extend(path_item_components);
  }
  rewrite_document(document, rewrite)
}

/// The edits the raw document calls for, found before the typed parse.
struct Rewrite {
  lifts: Vec<DefaultLift>,
  /// The `contentSchema` that replaces each frame type `data` property, by JSON pointer.
  content_schemas: BTreeMap<String, Value>,
}

impl Rewrite {
  fn is_empty(&self) -> bool {
    self.lifts.is_empty() && self.content_schemas.is_empty()
  }
}

/// Applies the content schemas and lifts, then inlines the path items, and reads the
/// result as the typed spec.
fn rewrite_document(mut document: Value, rewrite: &Rewrite) -> anyhow::Result<OpenApiV3Spec> {
  for (pointer, schema) in &rewrite.content_schemas {
    if let Some(property) = document.pointer_mut(pointer) {
      property.clone_from(schema);
    }
  }
  apply_lifts(&mut document, &rewrite.lifts);
  PathItemRefs::inline(&mut document);
  Ok(serde_json::from_value(document)?)
}

/// Raw `components.pathItems` entries that are a `$ref` with sibling fields beyond the
/// ones a typed reference keeps.
fn sibling_path_item_components(document: &Value) -> Vec<(String, Value)> {
  document
    .pointer("/components/pathItems")
    .and_then(Value::as_object)
    .into_iter()
    .flatten()
    .filter(|(_, item)| {
      item.as_object().is_some_and(|fields| {
        fields.contains_key("$ref")
          && fields
            .keys()
            .any(|key| !matches!(key.as_str(), "$ref" | "summary" | "description"))
      })
    })
    .map(|(name, item)| (name.clone(), item.clone()))
    .collect()
}

/// Moves each lifted property's `$ref` into `allOf` and sets its `default`.
fn apply_lifts(document: &mut Value, lifts: &[DefaultLift]) {
  for lift in lifts {
    let Some(schema) = document.pointer_mut(&lift.pointer).and_then(Value::as_object_mut) else {
      continue;
    };
    let Some(reference) = schema.remove("$ref") else {
      continue;
    };
    schema.insert(
      "allOf".to_string(),
      Value::Array(vec![serde_json::json!({ "$ref": reference })]),
    );
    schema.insert("default".to_string(), lift.default.clone());
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

/// Escapes a key for use as one segment of a JSON pointer.
fn escape_pointer_segment(key: &str) -> String {
  key.replace('~', "~0").replace('/', "~1")
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
pub const SSE_FRAME_FIELDS: [&str; 4] = ["data", "event", "id", "retry"];

/// Whether properties named `names` make a `text/event-stream` frame type: every one a
/// frame field, `data` among them.
pub fn is_frame_type<'a>(names: impl IntoIterator<Item = &'a str>) -> bool {
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

/// The JSON pointer to a property that [`apply_lifts`] rewrites, and its `default`.
struct DefaultLift {
  pointer: String,
  default: Value,
}

/// Property schemas in the raw document that set a `default` next to their `$ref`.
///
/// `oas3` keeps only `$ref`, `summary`, and `description` from a reference, so the
/// `default` would be lost. [`apply_lifts`] rewrites each such property into
/// `{ "allOf": [{ "$ref": ... }], "default": ... }`, which survives parsing. It only
/// lifts references to `enum`, `const`, and union components, whose variants the
/// default can name.
struct RefSiblingDefaults {
  targets: HashSet<String>,
  pointer: PointerBuf,
  lifts: Vec<DefaultLift>,
}

impl RefSiblingDefaults {
  /// Keywords whose values map names to schemas, so every value is visited as a schema.
  const SCHEMA_MAPS: [&'static str; 5] = ["schemas", "properties", "patternProperties", "$defs", "definitions"];
  /// Keywords whose values are example or constraint data, never schemas.
  const DATA_KEYWORDS: [&'static str; 4] = ["example", "examples", "enum", "const"];

  /// Every property of `document` that [`apply_lifts`] would rewrite.
  fn collect(document: &Value) -> Vec<DefaultLift> {
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
      .map(|(name, _)| format!("#/components/schemas/{}", escape_pointer_segment(name)))
      .collect();

    let mut pass = Self {
      targets,
      pointer: PointerBuf::default(),
      lifts: vec![],
    };
    if !pass.targets.is_empty() {
      pass.visit(document);
    }
    pass.lifts
  }

  fn visit(&mut self, value: &Value) {
    match value {
      Value::Object(map) => {
        for (keyword, child) in map {
          let mark = self.pointer.push(keyword);
          if Self::SCHEMA_MAPS.contains(&keyword.as_str())
            && let Some(schemas) = child.as_object()
          {
            for (name, schema) in schemas {
              let schema_mark = self.pointer.push(name);
              if keyword == "properties" {
                self.lift_property(schema);
              }
              self.visit(schema);
              self.pointer.truncate(schema_mark);
            }
          } else if !keyword.starts_with("x-") && !Self::DATA_KEYWORDS.contains(&keyword.as_str()) {
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

  fn lift_property(&mut self, property: &Value) {
    let targeted = property
      .get("$ref")
      .and_then(Value::as_str)
      .is_some_and(|target| self.targets.contains(target));
    if targeted && let Some(default) = property.get("default") {
      self.lifts.push(DefaultLift {
        pointer: self.pointer.as_str().to_string(),
        default: default.clone(),
      });
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
struct SecurityOverrides {
  paths: BTreeMap<String, PathItemSecurity>,
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
  /// Reads the overrides when the top-level `security` is non-empty. A missing or null
  /// `paths` or `webhooks` reads as empty.
  fn read(document: &Value) -> anyhow::Result<Option<Self>> {
    if document
      .get("security")
      .and_then(Value::as_array)
      .is_none_or(Vec::is_empty)
    {
      return Ok(None);
    }
    let read_section = |section: &str| -> anyhow::Result<BTreeMap<String, PathItemSecurity>> {
      let Some(items) = document.get(section).and_then(Value::as_object) else {
        return Ok(BTreeMap::new());
      };
      items
        .iter()
        .map(|(name, item)| Ok((name.clone(), PathItemSecurity::deserialize(item)?)))
        .collect()
    };
    Ok(Some(Self {
      paths: read_section("paths")?,
      webhooks: read_section("webhooks")?,
    }))
  }

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

use std::{collections::BTreeMap, ffi::OsStr, path::Path};

use fmmap::tokio::{AsyncMmapFile, AsyncMmapFileExt};
use oas3::{
  OpenApiV3Spec,
  spec::{Operation, PathItem, SecurityRequirement},
};
use serde::{
  Deserialize,
  de::{DeserializeOwned, IgnoredAny},
};

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

  /// Parses a document, keeping the operations that clear the top-level `security`
  /// with an empty array public.
  pub fn parse(self, content: &[u8]) -> anyhow::Result<oas3::Spec> {
    let mut spec = self.deserialize::<OpenApiV3Spec>(content)?;
    if !spec.security.is_empty() {
      self.deserialize::<SecurityOverrides>(content)?.apply(&mut spec);
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
}

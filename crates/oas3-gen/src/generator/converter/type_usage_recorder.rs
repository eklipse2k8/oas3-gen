use std::collections::BTreeMap;

use crate::generator::ast::{RustPrimitive, TypeRef};

#[derive(Debug, Clone, Copy, Default)]
struct UsageFlags {
  request: bool,
  response: bool,
}

impl UsageFlags {
  fn mark_request(&mut self) {
    self.request = true;
  }

  fn mark_response(&mut self) {
    self.response = true;
  }

  fn into_tuple(self) -> (bool, bool) {
    (self.request, self.response)
  }
}

#[derive(Debug, Default, Clone)]
pub(crate) struct TypeUsageRecorder {
  entries: BTreeMap<String, UsageFlags>,
}

impl TypeUsageRecorder {
  pub(crate) fn new() -> Self {
    Self {
      entries: BTreeMap::new(),
    }
  }

  pub(crate) fn mark_request<S: AsRef<str>>(&mut self, type_name: S) {
    let type_name = type_name.as_ref();
    if type_name.is_empty() {
      return;
    }
    self.entries.entry(type_name.to_string()).or_default().mark_request();
  }

  pub(crate) fn mark_response<S: AsRef<str>>(&mut self, type_name: S) {
    let type_name = type_name.as_ref();
    if type_name.is_empty() {
      return;
    }
    self.entries.entry(type_name.to_string()).or_default().mark_response();
  }

  pub(crate) fn mark_request_iter<I, S>(&mut self, types: I)
  where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
  {
    for type_name in types {
      self.mark_request(type_name);
    }
  }

  pub(crate) fn mark_response_iter<I, S>(&mut self, types: I)
  where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
  {
    for type_name in types {
      self.mark_response(type_name);
    }
  }

  pub(crate) fn into_usage_map(self) -> BTreeMap<String, (bool, bool)> {
    self
      .entries
      .into_iter()
      .map(|(name, flags)| (name, flags.into_tuple()))
      .collect()
  }
  pub(crate) fn mark_response_type_ref(&mut self, type_ref: &TypeRef) {
    if let RustPrimitive::Custom(name) = &type_ref.base_type {
      self.mark_response(name);
    }
  }
}

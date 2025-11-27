use std::collections::BTreeMap;

use crate::generator::ast::{EnumToken, RustPrimitive, TypeRef};

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

/// Records which Rust types are used as requests or responses.
///
/// Used for dependency analysis and filtering unused types.
#[derive(Debug, Default, Clone)]
pub(crate) struct TypeUsageRecorder {
  entries: BTreeMap<EnumToken, UsageFlags>,
}

impl TypeUsageRecorder {
  /// Creates a new `TypeUsageRecorder`.
  pub(crate) fn new() -> Self {
    Self {
      entries: BTreeMap::new(),
    }
  }

  /// Marks a type name as used in a request.
  pub(crate) fn mark_request(&mut self, type_name: impl Into<EnumToken>) {
    let token = type_name.into();
    if token.is_empty() {
      return;
    }
    self.entries.entry(token).or_default().mark_request();
  }

  /// Marks a type name as used in a response.
  pub(crate) fn mark_response(&mut self, type_name: impl Into<EnumToken>) {
    let token = type_name.into();
    if token.is_empty() {
      return;
    }
    self.entries.entry(token).or_default().mark_response();
  }

  /// Marks multiple types as requests.
  pub(crate) fn mark_request_iter<I, T>(&mut self, types: I)
  where
    I: IntoIterator<Item = T>,
    T: Into<EnumToken>,
  {
    for type_name in types {
      self.mark_request(type_name);
    }
  }

  /// Marks multiple types as responses.
  pub(crate) fn mark_response_iter<I, T>(&mut self, types: I)
  where
    I: IntoIterator<Item = T>,
    T: Into<EnumToken>,
  {
    for type_name in types {
      self.mark_response(type_name);
    }
  }

  /// Returns a map of TypeName -> (is_request, is_response).
  pub(crate) fn into_usage_map(self) -> BTreeMap<EnumToken, (bool, bool)> {
    self
      .entries
      .into_iter()
      .map(|(name, flags)| (name, flags.into_tuple()))
      .collect()
  }

  /// Analyzes a `TypeRef` and marks used types (e.g. custom structs inside `Box`).
  pub(crate) fn mark_response_type_ref(&mut self, type_ref: &TypeRef) {
    if let RustPrimitive::Custom(name) = &type_ref.base_type {
      self.mark_response(name.as_str());
    }
  }
}

use std::collections::{BTreeMap, BTreeSet};

use crate::generator::ast::{EnumToken, RustPrimitive, TypeRef};

#[derive(Debug, Clone, Copy, Default)]
struct UsageFlags {
  request: bool,
  response: bool,
}

impl UsageFlags {
  /// Sets the request usage flag to `true`.
  fn mark_request(&mut self) {
    self.request = true;
  }

  /// Sets the response usage flag to `true`.
  fn mark_response(&mut self) {
    self.response = true;
  }

  /// Converts the usage flags into a `(request, response)` tuple.
  ///
  /// Consumes `self` and returns the flags as a tuple where the first element
  /// indicates request usage and the second indicates response usage.
  fn into_tuple(self) -> (bool, bool) {
    (self.request, self.response)
  }

  /// Combines another `UsageFlags` into this one using logical OR.
  ///
  /// After merging, each flag is `true` if either `self` or `other` had that flag set.
  fn merge(&mut self, other: UsageFlags) {
    if other.request {
      self.request = true;
    }
    if other.response {
      self.response = true;
    }
  }
}

/// Records which Rust types are used as requests or responses.
///
/// Used for dependency analysis and filtering unused types.
/// Also tracks generation statistics like methods and headers.
#[derive(Debug, Default, Clone)]
pub(crate) struct TypeUsageRecorder {
  entries: BTreeMap<EnumToken, UsageFlags>,
  methods_generated: usize,
  unique_headers: BTreeSet<String>,
}

impl TypeUsageRecorder {
  /// Creates an empty recorder with no tracked types or statistics.
  pub(crate) fn new() -> Self {
    Self {
      entries: BTreeMap::new(),
      methods_generated: 0,
      unique_headers: BTreeSet::new(),
    }
  }

  /// Records that a type is used in a request body.
  ///
  /// Empty type names are ignored. The recorded usage propagates through
  /// the dependency graph during postprocessing to determine serde derives.
  pub(crate) fn mark_request(&mut self, type_name: impl Into<EnumToken>) {
    let token = type_name.into();
    if token.is_empty() {
      return;
    }
    self.entries.entry(token).or_default().mark_request();
  }

  /// Records that a type is used in a response body.
  ///
  /// Empty type names are ignored. The recorded usage propagates through
  /// the dependency graph during postprocessing to determine serde derives.
  pub(crate) fn mark_response(&mut self, type_name: impl Into<EnumToken>) {
    let token = type_name.into();
    if token.is_empty() {
      return;
    }
    self.entries.entry(token).or_default().mark_response();
  }

  /// Records multiple types as used in request bodies.
  ///
  /// Convenience method that calls [`mark_request`](Self::mark_request) for each type
  /// in the iterator.
  pub(crate) fn mark_request_iter<I, T>(&mut self, types: I)
  where
    I: IntoIterator<Item = T>,
    T: Into<EnumToken>,
  {
    for type_name in types {
      self.mark_request(type_name);
    }
  }

  /// Records multiple types as used in response bodies.
  ///
  /// Convenience method that calls [`mark_response`](Self::mark_response) for each type
  /// in the iterator.
  pub(crate) fn mark_response_iter<I, T>(&mut self, types: I)
  where
    I: IntoIterator<Item = T>,
    T: Into<EnumToken>,
  {
    for type_name in types {
      self.mark_response(type_name);
    }
  }

  /// Consumes the recorder and returns the usage data as a map.
  ///
  /// Each entry maps a type name to a `(is_request, is_response)` tuple.
  /// This map seeds the postprocessing phase, which propagates usage through
  /// type dependencies to compute final serde derive attributes.
  pub(crate) fn into_usage_map(self) -> BTreeMap<EnumToken, (bool, bool)> {
    self
      .entries
      .into_iter()
      .map(|(name, flags)| (name, flags.into_tuple()))
      .collect()
  }

  /// Records the custom type within a [`TypeRef`] as used in a response.
  ///
  /// Extracts the type name from [`RustPrimitive::Custom`] variants and records
  /// it as a response type. Primitive types are ignored since they do not
  /// require generated serde derives.
  pub(crate) fn mark_response_type_ref(&mut self, type_ref: &TypeRef) {
    if let RustPrimitive::Custom(name) = &type_ref.base_type {
      self.mark_response(name.as_ref());
    }
  }

  /// Increments the generated method counter.
  ///
  /// Called once per API operation to track generation statistics.
  pub(crate) fn record_method(&mut self) {
    self.methods_generated += 1;
  }

  /// Records an HTTP header name for statistics tracking.
  ///
  /// Header names are normalized to ASCII lowercase before insertion,
  /// ensuring case-insensitive deduplication (e.g., "Content-Type" and
  /// "content-type" count as one unique header).
  pub(crate) fn record_header(&mut self, header_name: &str) {
    self.unique_headers.insert(header_name.to_ascii_lowercase());
  }

  /// Returns the number of API methods recorded.
  pub(crate) fn methods_generated(&self) -> usize {
    self.methods_generated
  }

  /// Returns the count of unique HTTP headers recorded.
  pub(crate) fn headers_generated(&self) -> usize {
    self.unique_headers.len()
  }

  /// Combines another recorder's data into this one.
  ///
  /// Usage flags are merged with logical OR: if either recorder marked a type
  /// as request or response, the merged result reflects that. Method counts
  /// and header sets are combined additively.
  pub(crate) fn merge(&mut self, other: TypeUsageRecorder) {
    for (token, flags) in other.entries {
      self.entries.entry(token).or_default().merge(flags);
    }
    self.methods_generated += other.methods_generated;
    self.unique_headers.extend(other.unique_headers);
  }
}

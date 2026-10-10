use std::{
  collections::{HashMap, HashSet},
  rc::Rc,
};

use http::Method;
use indexmap::IndexMap;
use oas3::{Spec, spec::Operation};

use crate::generator::{
  ast::OperationKind,
  naming::{
    identifiers::{ensure_unique_snake_case_id, to_rust_field_name},
    operations::{compute_stable_id, trim_common_affixes},
  },
};

/// Metadata for a single API operation extracted from an OpenAPI specification.
///
/// This struct stores the stable identifier used for code generation along with
/// the HTTP method, path, and the original OpenAPI operation definition.
#[derive(Debug, Clone)]
pub struct OperationEntry {
  /// The stable snake_case identifier used for the generated Rust method name
  /// and as the base for derived request/response type names. Replaced by any
  /// user-provided function name override.
  pub stable_id: String,
  /// The HTTP method (GET, POST, etc.) for this operation.
  pub method: Method,
  /// The URL path pattern (e.g., `/users/{id}`).
  pub path: String,
  /// The original OpenAPI operation definition.
  pub operation: Rc<Operation>,
  /// Whether this is a standard HTTP operation or a webhook.
  pub kind: OperationKind,
}

/// Filter for including or excluding operations from code generation.
///
/// Allows selective generation of specific operations by their identifiers.
/// Both inclusion and exclusion filters can be combined; exclusion takes
/// precedence if an operation matches both.
#[derive(Debug, Clone, Default)]
pub struct OperationFilter {
  only: Option<HashSet<String>>,
  excluded: Option<HashSet<String>>,
}

impl OperationFilter {
  /// Creates a new filter with the given inclusion and exclusion sets.
  #[must_use]
  pub fn new(only: Option<&HashSet<String>>, excluded: Option<&HashSet<String>>) -> Self {
    Self {
      only: only.cloned(),
      excluded: excluded.cloned(),
    }
  }

  /// Returns whether the given operation ID passes this filter.
  ///
  /// An ID passes if it is either in the inclusion set (or there is no
  /// inclusion set) AND not in the exclusion set.
  #[must_use]
  pub fn accepts<S>(&self, base_id: S) -> bool
  where
    S: AsRef<str>,
  {
    if let Some(ref included) = self.only
      && !included.contains(base_id.as_ref())
    {
      return false;
    }

    if let Some(ref excluded) = self.excluded
      && excluded.contains(base_id.as_ref())
    {
      return false;
    }

    true
  }

  /// Returns whether a callback passes this filter. Either the ID of the operation
  /// that declares it or the callback's own ID can include or exclude it.
  #[must_use]
  pub fn accepts_callback<O, C>(&self, owner_id: O, callback_id: C) -> bool
  where
    O: AsRef<str>,
    C: AsRef<str>,
  {
    let names_either = |ids: &HashSet<String>| ids.contains(owner_id.as_ref()) || ids.contains(callback_id.as_ref());
    self.only.as_ref().is_none_or(names_either) && !self.excluded.as_ref().is_some_and(names_either)
  }
}

/// Internal context for collecting operations during the build process.
#[derive(Debug, Default)]
struct RegistrationContext {
  entries: IndexMap<String, OperationEntry>,
}

impl RegistrationContext {
  /// Registers an operation entry with its stable ID as the key.
  fn register(&mut self, entry: OperationEntry) {
    self.entries.insert(entry.stable_id.clone(), entry);
  }

  /// Registers `operation` under a stable ID made unique from `base_id`, unless
  /// `filter` rejects `base_id`.
  fn register_accepted(
    &mut self,
    filter: &OperationFilter,
    base_id: &str,
    method: &Method,
    path: String,
    operation: &Operation,
    kind: OperationKind,
  ) {
    if !filter.accepts(base_id) {
      return;
    }

    self.register_unique(base_id, method, path, operation, kind);
  }

  /// Registers `operation` under a stable ID made unique from `base_id`.
  fn register_unique(
    &mut self,
    base_id: &str,
    method: &Method,
    path: String,
    operation: &Operation,
    kind: OperationKind,
  ) {
    let stable_id = ensure_unique_snake_case_id(base_id, |id| self.contains_id(id));

    self.register(OperationEntry {
      stable_id,
      method: method.clone(),
      path,
      operation: Rc::new(operation.clone()),
      kind,
    });
  }

  /// Returns whether the given ID is already registered.
  fn contains_id<S>(&self, id: S) -> bool
  where
    S: AsRef<str>,
  {
    self.entries.contains_key(id.as_ref())
  }

  /// Simplifies all registered keys by removing common prefixes and suffixes.
  ///
  /// This improves the ergonomics of generated method names by trimming
  /// redundant affixes (e.g., converting `petstore_get_pet` to `get_pet`).
  fn simplify_keys(&mut self) {
    let original_keys = self.entries.keys().cloned().collect::<Vec<_>>();
    let simplified_keys = trim_common_affixes(&original_keys);

    let remapped = original_keys
      .into_iter()
      .zip(simplified_keys)
      .filter_map(|(old, new)| {
        self.entries.shift_remove(&old).map(|mut entry| {
          entry.stable_id.clone_from(&new);
          (new, entry)
        })
      })
      .collect::<IndexMap<_, _>>();

    self.entries = remapped;
  }

  /// Consumes the context and returns all registered entries.
  fn into_entries(self) -> Vec<OperationEntry> {
    self.entries.into_values().collect()
  }
}

/// Trait for sources that can ingest operations into a registration context.
trait OperationSource {
  /// Ingests operations from this source into the given context, applying
  /// the provided filter.
  fn ingest(&self, context: &mut RegistrationContext, filter: &OperationFilter);
}

/// Source that extracts standard HTTP operations from an OpenAPI specification.
struct HttpOperationSource {
  spec: Rc<Spec>,
}

impl HttpOperationSource {
  /// Creates a new HTTP operation source from the given specification.
  fn new(spec: Rc<Spec>) -> Self {
    Self { spec }
  }
}

impl OperationSource for HttpOperationSource {
  fn ingest(&self, context: &mut RegistrationContext, filter: &OperationFilter) {
    for (path, method, operation) in self.spec.operations() {
      let base_id = compute_stable_id(method.as_str(), &path, operation.operation_id.as_deref());
      context.register_accepted(filter, &base_id, &method, path, operation, OperationKind::Http);
    }
  }
}

/// Source that extracts webhook operations from an OpenAPI specification.
struct WebhookOperationSource {
  spec: Rc<Spec>,
}

impl WebhookOperationSource {
  /// Creates a new webhook operation source from the given specification.
  fn new(spec: Rc<Spec>) -> Self {
    Self { spec }
  }
}

impl OperationSource for WebhookOperationSource {
  fn ingest(&self, context: &mut RegistrationContext, filter: &OperationFilter) {
    for (name, path_item) in &self.spec.webhooks {
      for (method, operation) in path_item.methods() {
        let display_path = format!("webhooks/{name}");
        let base_id = compute_stable_id(method.as_str(), &display_path, operation.operation_id.as_deref());
        context.register_accepted(
          filter,
          &base_id,
          &method,
          display_path,
          operation,
          OperationKind::Webhook,
        );
      }
    }
  }
}

/// Source that extracts the callbacks HTTP operations declare.
///
/// A callback's URL is a runtime expression the caller supplies, so the generated
/// server receives each callback at `callbacks/{operationId}/{callbackName}`, the
/// path a caller registers under its own host. A callback with several expressions
/// appends each one's position.
struct CallbackOperationSource {
  spec: Rc<Spec>,
}

impl CallbackOperationSource {
  /// Creates a new callback operation source from the given specification.
  fn new(spec: Rc<Spec>) -> Self {
    Self { spec }
  }
}

impl OperationSource for CallbackOperationSource {
  fn ingest(&self, context: &mut RegistrationContext, filter: &OperationFilter) {
    for (path, method, operation) in self.spec.operations() {
      if operation.callbacks.is_empty() {
        continue;
      }
      let owner = operation
        .operation_id
        .clone()
        .unwrap_or_else(|| compute_stable_id(method.as_str(), &path, None));
      let owner_id = compute_stable_id(method.as_str(), &path, operation.operation_id.as_deref());

      for (name, callback) in &operation.callbacks {
        let Ok(callback) = callback.resolve(&self.spec) else {
          continue;
        };
        let several = callback.paths.len() > 1;
        let fallback_id = format!("{owner}_{name}");

        for (position, path_item) in callback.paths.values().enumerate() {
          let display_path = if several {
            format!("callbacks/{owner}/{name}/{position}")
          } else {
            format!("callbacks/{owner}/{name}")
          };

          for (callback_method, callback_operation) in path_item.methods() {
            let base_id = compute_stable_id(
              callback_method.as_str(),
              display_path.as_str(),
              Some(callback_operation.operation_id.as_deref().unwrap_or(&fallback_id)),
            );
            if !filter.accepts_callback(&owner_id, &base_id) {
              continue;
            }
            context.register_unique(
              &base_id,
              &callback_method,
              display_path.clone(),
              callback_operation,
              OperationKind::Callback,
            );
          }
        }
      }
    }
  }
}

/// Builder for constructing an [`OperationRegistry`] from multiple sources.
#[derive(Default)]
struct OperationRegistryBuilder {
  sources: Vec<Box<dyn OperationSource>>,
  callback_sources: Vec<Box<dyn OperationSource>>,
  filter: OperationFilter,
  fn_name_overrides: HashMap<String, String>,
}

impl OperationRegistryBuilder {
  /// Creates a new builder with no sources and an empty filter.
  fn new() -> Self {
    Self::default()
  }

  /// Sets the filter to apply during the build process.
  fn with_filter(mut self, filter: OperationFilter) -> Self {
    self.filter = filter;
    self
  }

  /// Sets the function name overrides to apply, keyed by operation ID.
  fn with_fn_name_overrides(mut self, overrides: &HashMap<String, String>) -> Self {
    self.fn_name_overrides.clone_from(overrides);
    self
  }

  /// Adds an operation source to this builder.
  fn with_source<S: OperationSource + 'static>(mut self, source: S) -> Self {
    self.sources.push(Box::new(source));
    self
  }

  /// Adds a callback source, ingested after the other sources' identifiers are
  /// simplified so callback IDs never change how those are trimmed.
  fn with_callback_source<S: OperationSource + 'static>(mut self, source: S) -> Self {
    self.callback_sources.push(Box::new(source));
    self
  }

  /// Consumes this builder and constructs the final [`OperationRegistry`].
  ///
  /// This ingests operations from the sources, applies the filter, simplifies the
  /// resulting identifiers, ingests the callback sources, and applies function name
  /// overrides.
  fn build(self) -> OperationRegistry {
    let mut context = RegistrationContext::default();

    for source in &self.sources {
      source.ingest(&mut context, &self.filter);
    }

    context.simplify_keys();

    for source in &self.callback_sources {
      source.ingest(&mut context, &self.filter);
    }

    let mut entries = context.into_entries();
    apply_fn_name_overrides(&mut entries, &self.fn_name_overrides);

    OperationRegistry { entries }
  }
}

/// Applies user-provided function name overrides by replacing entries' stable IDs.
///
/// Overrides are matched by the operation's `operationId` as written in the spec,
/// falling back to the stable ID for operations without one. Custom names are
/// normalized to valid snake_case Rust identifiers. The original `operationId`
/// remains available on `entry.operation.operation_id` for diagnostics.
fn apply_fn_name_overrides(entries: &mut [OperationEntry], overrides: &HashMap<String, String>) {
  if overrides.is_empty() {
    return;
  }

  for entry in entries {
    let custom = entry
      .operation
      .operation_id
      .as_deref()
      .and_then(|id| overrides.get(id))
      .or_else(|| overrides.get(&entry.stable_id));

    if let Some(custom) = custom {
      entry.stable_id = to_rust_field_name(custom);
    }
  }
}

/// Registry of all API operations extracted from an OpenAPI specification.
///
/// Collects and manages both standard HTTP operations and webhooks, providing
/// stable, unique identifiers for each operation to use during code generation.
///
/// The registry filters operations based on CLI options and normalizes
/// operation identifiers to valid Rust method names.
#[derive(Debug)]
pub struct OperationRegistry {
  pub(crate) entries: Vec<OperationEntry>,
}

impl OperationRegistry {
  /// Creates a registry from the given specification without any filtering.
  #[must_use]
  pub fn new(spec: &Spec) -> Self {
    Self::with_filters(spec, None, None, &HashMap::new())
  }

  /// Creates a registry with optional inclusion and exclusion filters and
  /// function name overrides keyed by operation ID.
  #[must_use]
  pub fn with_filters(
    spec: &Spec,
    only_operations: Option<&HashSet<String>>,
    excluded_operations: Option<&HashSet<String>>,
    fn_name_overrides: &HashMap<String, String>,
  ) -> Self {
    let spec = Rc::new(spec.clone());
    OperationRegistryBuilder::new()
      .with_filter(OperationFilter::new(only_operations, excluded_operations))
      .with_fn_name_overrides(fn_name_overrides)
      .with_source(HttpOperationSource::new(Rc::clone(&spec)))
      .with_source(WebhookOperationSource::new(Rc::clone(&spec)))
      .with_callback_source(CallbackOperationSource::new(spec))
      .build()
  }

  /// Returns an iterator over all registered operations.
  ///
  /// Operations are yielded in the order they were registered (HTTP
  /// operations first, then webhooks, then callbacks), with original
  /// specification order preserved within each category.
  pub fn operations(&self) -> impl Iterator<Item = &OperationEntry> {
    self.entries.iter()
  }
}

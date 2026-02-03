use std::{collections::HashSet, rc::Rc};

use http::Method;
use indexmap::IndexMap;
use oas3::{Spec, spec::Operation};

use crate::generator::{
  ast::OperationKind,
  naming::{
    identifiers::ensure_unique_snake_case_id,
    operations::{compute_stable_id, trim_common_affixes},
  },
};

/// Metadata for a single API operation extracted from an OpenAPI specification.
///
/// This struct stores the stable identifier used for code generation along with
/// the HTTP method, path, and the original OpenAPI operation definition.
#[derive(Debug, Clone)]
pub struct OperationEntry {
  /// The stable snake_case identifier used for the generated Rust method name.
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
        self.entries.swap_remove(&old).map(|mut entry| {
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
  fn new(spec: &Spec) -> Self {
    Self {
      spec: Rc::new(spec.clone()),
    }
  }
}

impl OperationSource for HttpOperationSource {
  fn ingest(&self, context: &mut RegistrationContext, filter: &OperationFilter) {
    for (path, method, operation) in self.spec.operations() {
      let base_id = compute_stable_id(method.as_str(), &path, operation.operation_id.as_deref());

      if !filter.accepts(&base_id) {
        continue;
      }

      let stable_id = ensure_unique_snake_case_id(&base_id, |id| context.contains_id(id));

      context.register(OperationEntry {
        stable_id,
        method: method.clone(),
        path,
        operation: Rc::new(operation.clone()),
        kind: OperationKind::Http,
      });
    }
  }
}

/// Source that extracts webhook operations from an OpenAPI specification.
struct WebhookOperationSource {
  spec: Rc<Spec>,
}

impl WebhookOperationSource {
  /// Creates a new webhook operation source from the given specification.
  fn new(spec: &Spec) -> Self {
    Self {
      spec: Rc::new(spec.clone()),
    }
  }
}

impl OperationSource for WebhookOperationSource {
  fn ingest(&self, context: &mut RegistrationContext, filter: &OperationFilter) {
    for (name, path_item) in &self.spec.webhooks {
      for (method, operation) in path_item.methods() {
        let display_path = format!("webhooks/{name}");
        let base_id = compute_stable_id(method.as_str(), &display_path, operation.operation_id.as_deref());

        if !filter.accepts(&base_id) {
          continue;
        }

        let stable_id = ensure_unique_snake_case_id(&base_id, |id| context.contains_id(id));

        context.register(OperationEntry {
          stable_id,
          method: method.clone(),
          path: display_path,
          operation: Rc::new(operation.clone()),
          kind: OperationKind::Webhook,
        });
      }
    }
  }
}

/// Builder for constructing an [`OperationRegistry`] from multiple sources.
#[derive(Default)]
struct OperationRegistryBuilder {
  sources: Vec<Box<dyn OperationSource>>,
  filter: OperationFilter,
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

  /// Adds an operation source to this builder.
  fn with_source<S: OperationSource + 'static>(mut self, source: S) -> Self {
    self.sources.push(Box::new(source));
    self
  }

  /// Consumes this builder and constructs the final [`OperationRegistry`].
  ///
  /// This ingests all operations from registered sources, applies the filter,
  /// and simplifies the resulting identifiers.
  fn build(self) -> OperationRegistry {
    let mut context = RegistrationContext::default();

    for source in &self.sources {
      source.ingest(&mut context, &self.filter);
    }

    context.simplify_keys();

    OperationRegistry {
      entries: context.into_entries(),
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
    Self::with_filters(spec, None, None)
  }

  /// Creates a registry with optional inclusion and exclusion filters.
  #[must_use]
  pub fn with_filters(
    spec: &Spec,
    only_operations: Option<&HashSet<String>>,
    excluded_operations: Option<&HashSet<String>>,
  ) -> Self {
    OperationRegistryBuilder::new()
      .with_filter(OperationFilter::new(only_operations, excluded_operations))
      .with_source(HttpOperationSource::new(spec))
      .with_source(WebhookOperationSource::new(spec))
      .build()
  }

  /// Returns an iterator over all registered operations.
  ///
  /// Operations are yielded in the order they were registered (HTTP
  /// operations first, then webhooks), with original specification order
  /// preserved within each category.
  pub fn operations(&self) -> impl Iterator<Item = &OperationEntry> {
    self.entries.iter()
  }
}

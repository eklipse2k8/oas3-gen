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

#[derive(Debug, Clone)]
pub struct OperationEntry {
  pub stable_id: String,
  pub method: Method,
  pub path: String,
  pub operation: Rc<Operation>,
  pub kind: OperationKind,
}

#[derive(Debug, Clone, Default)]
pub struct OperationFilter {
  only: Option<HashSet<String>>,
  excluded: Option<HashSet<String>>,
}

impl OperationFilter {
  #[must_use]
  pub fn new(only: Option<&HashSet<String>>, excluded: Option<&HashSet<String>>) -> Self {
    Self {
      only: only.cloned(),
      excluded: excluded.cloned(),
    }
  }

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

#[derive(Debug, Default)]
struct RegistrationContext {
  entries: IndexMap<String, OperationEntry>,
}

impl RegistrationContext {
  fn register(&mut self, entry: OperationEntry) {
    self.entries.insert(entry.stable_id.clone(), entry);
  }

  fn contains_id<S>(&self, id: S) -> bool
  where
    S: AsRef<str>,
  {
    self.entries.contains_key(id.as_ref())
  }

  fn simplify_keys(&mut self) {
    let original_keys = self.entries.keys().cloned().collect::<Vec<String>>();
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
      .collect::<IndexMap<String, OperationEntry>>();

    self.entries = remapped;
  }

  fn into_entries(self) -> Vec<OperationEntry> {
    self.entries.into_values().collect()
  }
}

trait OperationSource {
  fn ingest(&self, context: &mut RegistrationContext, filter: &OperationFilter);
}

struct HttpOperationSource {
  spec: Rc<Spec>,
}

impl HttpOperationSource {
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

struct WebhookOperationSource {
  spec: Rc<Spec>,
}

impl WebhookOperationSource {
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

#[derive(Default)]
struct OperationRegistryBuilder {
  sources: Vec<Box<dyn OperationSource>>,
  filter: OperationFilter,
}

impl OperationRegistryBuilder {
  fn new() -> Self {
    Self::default()
  }

  fn with_filter(mut self, filter: OperationFilter) -> Self {
    self.filter = filter;
    self
  }

  fn with_source<S: OperationSource + 'static>(mut self, source: S) -> Self {
    self.sources.push(Box::new(source));
    self
  }

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

#[derive(Debug)]
pub struct OperationRegistry {
  entries: Vec<OperationEntry>,
}

impl OperationRegistry {
  #[must_use]
  pub fn new(spec: &Spec) -> Self {
    Self::with_filters(spec, None, None)
  }

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

  pub fn operations(&self) -> impl Iterator<Item = &OperationEntry> {
    self.entries.iter()
  }

  #[cfg(test)]
  #[must_use]
  pub fn len(&self) -> usize {
    self.entries.len()
  }

  #[cfg(test)]
  #[must_use]
  pub fn is_empty(&self) -> bool {
    self.entries.is_empty()
  }
}

use std::{
  collections::{BTreeMap, BTreeSet},
  string::ToString,
};

use oas3::{
  Spec,
  spec::{ObjectOrReference, ObjectSchema, Schema},
};

use super::orchestrator::GenerationWarning;

const SCHEMA_REF_PREFIX: &str = "#/components/schemas/";

#[derive(Debug)]
struct SchemaRepository {
  schemas: BTreeMap<String, ObjectSchema>,
}

impl SchemaRepository {
  fn from_spec(spec: &Spec) -> (Self, Vec<GenerationWarning>) {
    let mut schemas = BTreeMap::new();
    let mut warnings = Vec::new();

    if let Some(components) = &spec.components {
      for (name, schema_ref) in &components.schemas {
        match schema_ref.resolve(spec) {
          Ok(schema) => {
            schemas.insert(name.clone(), schema);
          }
          Err(error) => {
            warnings.push(GenerationWarning::SchemaConversionFailed {
              schema_name: name.clone(),
              error: error.to_string(),
            });
          }
        }
      }
    }

    (Self { schemas }, warnings)
  }

  fn get(&self, name: &str) -> Option<&ObjectSchema> {
    self.schemas.get(name)
  }

  fn names(&self) -> impl Iterator<Item = &String> {
    self.schemas.keys()
  }
}

#[derive(Debug)]
struct ReferenceExtractor;

impl ReferenceExtractor {
  fn extract_from_schema(schema: &ObjectSchema) -> BTreeSet<String> {
    let mut refs = BTreeSet::new();

    Self::collect_from_properties(schema, &mut refs);
    Self::collect_from_combinators(schema, &mut refs);
    Self::collect_from_items(schema, &mut refs);

    refs
  }

  fn collect_from_properties(schema: &ObjectSchema, refs: &mut BTreeSet<String>) {
    for prop_schema in schema.properties.values() {
      Self::extract_from_schema_ref(prop_schema, refs);
    }
  }

  fn collect_from_combinators(schema: &ObjectSchema, refs: &mut BTreeSet<String>) {
    for schema_ref in schema.one_of.iter().chain(&schema.any_of).chain(&schema.all_of) {
      Self::extract_from_schema_ref(schema_ref, refs);
    }
  }

  fn collect_from_items(schema: &ObjectSchema, refs: &mut BTreeSet<String>) {
    if let Some(ref items_box) = schema.items
      && let Schema::Object(ref schema_ref) = **items_box
    {
      Self::extract_from_schema_ref(schema_ref, refs);
    }
  }

  fn extract_from_schema_ref(schema_ref: &ObjectOrReference<ObjectSchema>, refs: &mut BTreeSet<String>) {
    if let Some(ref_name) = Self::extract_ref_name_from_obj_ref(schema_ref) {
      refs.insert(ref_name);
    }

    if let ObjectOrReference::Object(inline_schema) = schema_ref {
      let inline_refs = Self::extract_from_schema(inline_schema);
      refs.extend(inline_refs);
    }
  }

  fn extract_ref_name_from_obj_ref(obj_ref: &ObjectOrReference<ObjectSchema>) -> Option<String> {
    match obj_ref {
      ObjectOrReference::Ref { ref_path, .. } => ref_path.strip_prefix(SCHEMA_REF_PREFIX).map(ToString::to_string),
      ObjectOrReference::Object(_) => None,
    }
  }
}

#[derive(Debug)]
struct DependencyGraph {
  dependencies: BTreeMap<String, BTreeSet<String>>,
  cyclic_schemas: BTreeSet<String>,
}

impl DependencyGraph {
  fn new() -> Self {
    Self {
      dependencies: BTreeMap::new(),
      cyclic_schemas: BTreeSet::new(),
    }
  }

  fn build(&mut self, repository: &SchemaRepository) {
    for schema_name in repository.names() {
      let deps = repository
        .get(schema_name)
        .map(ReferenceExtractor::extract_from_schema)
        .unwrap_or_default();

      self.dependencies.insert(schema_name.clone(), deps);
    }
  }

  fn detect_cycles(&mut self) -> Vec<Vec<String>> {
    let mut detector = CycleDetector::new(&self.dependencies);
    let cycles = detector.find_all_cycles();

    for cycle in &cycles {
      for schema_name in cycle {
        self.cyclic_schemas.insert(schema_name.clone());
      }
    }

    cycles
  }

  fn is_cyclic(&self, schema_name: &str) -> bool {
    self.cyclic_schemas.contains(schema_name)
  }
}

#[derive(Debug)]
struct CycleDetector<'a> {
  dependencies: &'a BTreeMap<String, BTreeSet<String>>,
  visited: BTreeSet<String>,
  recursion_stack: BTreeSet<String>,
  path: Vec<String>,
  cycles: Vec<Vec<String>>,
}

impl<'a> CycleDetector<'a> {
  fn new(dependencies: &'a BTreeMap<String, BTreeSet<String>>) -> Self {
    Self {
      dependencies,
      visited: BTreeSet::new(),
      recursion_stack: BTreeSet::new(),
      path: Vec::new(),
      cycles: Vec::new(),
    }
  }

  fn find_all_cycles(&mut self) -> Vec<Vec<String>> {
    let nodes: Vec<String> = self.dependencies.keys().cloned().collect();

    for node in nodes {
      if !self.visited.contains(&node) {
        self.visit(&node);
      }
    }

    std::mem::take(&mut self.cycles)
  }

  fn visit(&mut self, node: &str) {
    self.visited.insert(node.to_string());
    self.recursion_stack.insert(node.to_string());
    self.path.push(node.to_string());

    if let Some(deps) = self.dependencies.get(node) {
      for dep in deps {
        if !self.visited.contains(dep) {
          self.visit(dep);
        } else if self.recursion_stack.contains(dep) {
          self.record_cycle(dep);
        }
      }
    }

    self.path.pop();
    self.recursion_stack.remove(node);
  }

  fn record_cycle(&mut self, cycle_start: &str) {
    if let Some(start_pos) = self.path.iter().position(|n| n == cycle_start) {
      let cycle: Vec<String> = self.path[start_pos..].to_vec();
      self.cycles.push(cycle);
    }
  }
}

/// Graph structure for managing OpenAPI schemas and their dependencies
#[derive(Debug)]
pub(crate) struct SchemaGraph {
  repository: SchemaRepository,
  dependency_graph: DependencyGraph,
  discriminator_cache: BTreeMap<String, (String, String)>,
  spec: Spec,
}

impl SchemaGraph {
  pub(crate) fn new(spec: Spec) -> (Self, Vec<GenerationWarning>) {
    let (repository, warnings) = SchemaRepository::from_spec(&spec);
    let discriminator_cache = Self::build_discriminator_cache(&repository);

    (
      Self {
        repository,
        dependency_graph: DependencyGraph::new(),
        discriminator_cache,
        spec,
      },
      warnings,
    )
  }

  fn build_discriminator_cache(repository: &SchemaRepository) -> BTreeMap<String, (String, String)> {
    let mut cache = BTreeMap::new();

    for candidate_name in repository.names() {
      if let Some(candidate_schema) = repository.get(candidate_name)
        && let Some(d) = &candidate_schema.discriminator
        && let Some(mapping) = &d.mapping
      {
        for (val, ref_path) in mapping {
          if let Some(schema_name) = ref_path.strip_prefix(SCHEMA_REF_PREFIX) {
            cache.insert(schema_name.to_string(), (d.property_name.clone(), val.clone()));
          }
        }
      }
    }

    cache
  }

  pub(crate) fn get_schema(&self, name: &str) -> Option<&ObjectSchema> {
    self.repository.get(name)
  }

  pub(crate) fn schema_names(&self) -> Vec<&String> {
    self.repository.names().collect()
  }

  pub(crate) fn spec(&self) -> &Spec {
    &self.spec
  }

  pub(crate) fn extract_ref_name(ref_string: &str) -> Option<String> {
    ref_string.strip_prefix(SCHEMA_REF_PREFIX).map(ToString::to_string)
  }

  pub(crate) fn extract_ref_name_from_ref(obj_ref: &ObjectOrReference<ObjectSchema>) -> Option<String> {
    match obj_ref {
      ObjectOrReference::Ref { ref_path, .. } => Self::extract_ref_name(ref_path),
      ObjectOrReference::Object(_) => None,
    }
  }

  pub(crate) fn build_dependencies(&mut self) {
    self.dependency_graph.build(&self.repository);
  }

  pub(crate) fn detect_cycles(&mut self) -> Vec<Vec<String>> {
    self.dependency_graph.detect_cycles()
  }

  pub(crate) fn is_cyclic(&self, schema_name: &str) -> bool {
    self.dependency_graph.is_cyclic(schema_name)
  }

  pub(crate) fn get_discriminator_mapping(&self, schema_name: &str) -> Option<&(String, String)> {
    self.discriminator_cache.get(schema_name)
  }

  pub(crate) fn get_operation_reachable_schemas(
    &self,
    operation_registry: &crate::generator::operation_registry::OperationRegistry,
  ) -> BTreeSet<String> {
    let mut reachable = BTreeSet::new();

    for (_stable_id, _method, _path, operation) in operation_registry.operations_with_details() {
      Self::collect_refs_from_operation(operation, &self.spec, &mut reachable);
    }

    self.expand_with_dependencies(&reachable)
  }

  fn collect_refs_from_operation(operation: &oas3::spec::Operation, spec: &Spec, refs: &mut BTreeSet<String>) {
    for param in &operation.parameters {
      if let Ok(resolved_param) = param.resolve(spec)
        && let Some(ref schema_ref) = resolved_param.schema
      {
        Self::collect_refs_from_schema_ref(schema_ref, refs);
      }
    }

    if let Some(ref request_body_ref) = operation.request_body
      && let Ok(request_body) = request_body_ref.resolve(spec)
    {
      for media_type in request_body.content.values() {
        if let Some(ref schema_ref) = media_type.schema {
          Self::collect_refs_from_schema_ref(schema_ref, refs);
        }
      }
    }

    if let Some(ref responses) = operation.responses {
      for response_ref in responses.values() {
        if let Ok(response) = response_ref.resolve(spec) {
          for media_type in response.content.values() {
            if let Some(ref schema_ref) = media_type.schema {
              Self::collect_refs_from_schema_ref(schema_ref, refs);
            }
          }
        }
      }
    }
  }

  fn collect_refs_from_schema_ref(schema_ref: &ObjectOrReference<ObjectSchema>, refs: &mut BTreeSet<String>) {
    if let Some(ref_name) = Self::extract_ref_name_from_ref(schema_ref) {
      refs.insert(ref_name);
    } else if let ObjectOrReference::Object(inline_schema) = schema_ref {
      let inline_refs = ReferenceExtractor::extract_from_schema(inline_schema);
      refs.extend(inline_refs);
    }
  }

  fn expand_with_dependencies(&self, initial_refs: &BTreeSet<String>) -> BTreeSet<String> {
    let mut expanded = BTreeSet::new();
    let mut to_visit: Vec<String> = initial_refs.iter().cloned().collect();

    while let Some(schema_name) = to_visit.pop() {
      if expanded.insert(schema_name.clone())
        && let Some(deps) = self.dependency_graph.dependencies.get(&schema_name)
      {
        for dep in deps {
          if !expanded.contains(dep) {
            to_visit.push(dep.clone());
          }
        }
      }
    }

    expanded
  }
}

#[cfg(test)]
mod tests {
  use std::collections::BTreeMap;

  use oas3::spec::{Components, Spec};

  use super::*;

  fn create_test_spec_with_schemas(schemas: BTreeMap<String, ObjectOrReference<ObjectSchema>>) -> Spec {
    Spec {
      openapi: "3.0.0".to_string(),
      info: oas3::spec::Info {
        title: "Test".to_string(),
        summary: None,
        version: "1.0.0".to_string(),
        description: None,
        terms_of_service: None,
        contact: None,
        license: None,
        extensions: BTreeMap::default(),
      },
      servers: Vec::new(),
      paths: Option::default(),
      webhooks: BTreeMap::default(),
      components: Some(Components {
        schemas,
        ..Default::default()
      }),
      security: Vec::new(),
      tags: Vec::new(),
      external_docs: None,
      extensions: BTreeMap::default(),
    }
  }

  fn create_simple_schema() -> ObjectSchema {
    ObjectSchema {
      schema_type: None,
      properties: BTreeMap::new(),
      ..Default::default()
    }
  }

  fn create_schema_with_ref(ref_name: &str) -> ObjectSchema {
    let mut properties = BTreeMap::new();
    properties.insert(
      "related".to_string(),
      ObjectOrReference::Ref {
        ref_path: format!("{SCHEMA_REF_PREFIX}{ref_name}"),
        summary: None,
        description: None,
      },
    );
    ObjectSchema {
      schema_type: None,
      properties,
      ..Default::default()
    }
  }

  #[test]
  fn test_schema_repository_from_spec() {
    let mut schemas = BTreeMap::new();
    schemas.insert("User".to_string(), ObjectOrReference::Object(create_simple_schema()));
    schemas.insert("Post".to_string(), ObjectOrReference::Object(create_simple_schema()));

    let spec = create_test_spec_with_schemas(schemas);
    let (repo, _warnings) = SchemaRepository::from_spec(&spec);

    assert!(repo.get("User").is_some());
    assert!(repo.get("Post").is_some());
    assert!(repo.get("NonExistent").is_none());
    assert_eq!(repo.names().count(), 2);
  }

  #[test]
  fn test_reference_extractor_simple_ref() {
    let schema = create_schema_with_ref("User");
    let refs = ReferenceExtractor::extract_from_schema(&schema);

    assert_eq!(refs.len(), 1);
    assert!(refs.contains("User"));
  }

  #[test]
  fn test_reference_extractor_multiple_refs() {
    let mut properties = BTreeMap::new();
    properties.insert(
      "author".to_string(),
      ObjectOrReference::Ref {
        ref_path: format!("{SCHEMA_REF_PREFIX}User"),
        summary: None,
        description: None,
      },
    );
    properties.insert(
      "category".to_string(),
      ObjectOrReference::Ref {
        ref_path: format!("{SCHEMA_REF_PREFIX}Category"),
        summary: None,
        description: None,
      },
    );

    let schema = ObjectSchema {
      schema_type: None,
      properties,
      ..Default::default()
    };
    let refs = ReferenceExtractor::extract_from_schema(&schema);

    assert_eq!(refs.len(), 2);
    assert!(refs.contains("User"));
    assert!(refs.contains("Category"));
  }

  #[test]
  fn test_reference_extractor_combinators() {
    let user_ref = ObjectOrReference::Ref {
      ref_path: format!("{SCHEMA_REF_PREFIX}User"),
      summary: None,
      description: None,
    };
    let post_ref = ObjectOrReference::Ref {
      ref_path: format!("{SCHEMA_REF_PREFIX}Post"),
      summary: None,
      description: None,
    };
    let comment_ref = ObjectOrReference::Ref {
      ref_path: format!("{SCHEMA_REF_PREFIX}Comment"),
      summary: None,
      description: None,
    };

    let schema = ObjectSchema {
      schema_type: None,
      one_of: vec![user_ref],
      any_of: vec![post_ref],
      all_of: vec![comment_ref],
      ..Default::default()
    };

    let refs = ReferenceExtractor::extract_from_schema(&schema);

    assert_eq!(refs.len(), 3);
    assert!(refs.contains("User"));
    assert!(refs.contains("Post"));
    assert!(refs.contains("Comment"));
  }

  #[test]
  fn test_cycle_detector_no_cycles() {
    let mut deps = BTreeMap::new();
    deps.insert("A".to_string(), BTreeSet::new());
    deps.insert("B".to_string(), BTreeSet::from(["A".to_string()]));
    deps.insert("C".to_string(), BTreeSet::from(["B".to_string()]));

    let mut detector = CycleDetector::new(&deps);
    let cycles = detector.find_all_cycles();

    assert!(cycles.is_empty());
  }

  #[test]
  fn test_cycle_detector_simple_cycle() {
    let mut deps = BTreeMap::new();
    deps.insert("A".to_string(), BTreeSet::from(["B".to_string()]));
    deps.insert("B".to_string(), BTreeSet::from(["A".to_string()]));

    let mut detector = CycleDetector::new(&deps);
    let cycles = detector.find_all_cycles();

    assert_eq!(cycles.len(), 1);
    assert!(!cycles[0].is_empty());
  }

  #[test]
  fn test_cycle_detector_self_reference() {
    let mut deps = BTreeMap::new();
    deps.insert("A".to_string(), BTreeSet::from(["A".to_string()]));

    let mut detector = CycleDetector::new(&deps);
    let cycles = detector.find_all_cycles();

    assert_eq!(cycles.len(), 1);
  }

  #[test]
  fn test_dependency_graph_build() {
    let mut schemas = BTreeMap::new();
    schemas.insert("User".to_string(), ObjectOrReference::Object(create_simple_schema()));
    schemas.insert(
      "Post".to_string(),
      ObjectOrReference::Object(create_schema_with_ref("User")),
    );

    let spec = create_test_spec_with_schemas(schemas);
    let (repo, _warnings) = SchemaRepository::from_spec(&spec);

    let mut graph = DependencyGraph::new();
    graph.build(&repo);

    assert_eq!(graph.dependencies.len(), 2);
    assert!(graph.dependencies.get("User").unwrap().is_empty());
    assert_eq!(graph.dependencies.get("Post").unwrap().len(), 1);
  }

  #[test]
  fn test_dependency_graph_detect_cycles() {
    let mut schemas = BTreeMap::new();
    let mut user_schema = create_simple_schema();
    user_schema.properties.insert(
      "posts".to_string(),
      ObjectOrReference::Ref {
        ref_path: format!("{SCHEMA_REF_PREFIX}Post"),
        summary: None,
        description: None,
      },
    );
    let mut post_schema = create_simple_schema();
    post_schema.properties.insert(
      "author".to_string(),
      ObjectOrReference::Ref {
        ref_path: format!("{SCHEMA_REF_PREFIX}User"),
        summary: None,
        description: None,
      },
    );

    schemas.insert("User".to_string(), ObjectOrReference::Object(user_schema));
    schemas.insert("Post".to_string(), ObjectOrReference::Object(post_schema));

    let spec = create_test_spec_with_schemas(schemas);
    let (repo, _warnings) = SchemaRepository::from_spec(&spec);

    let mut graph = DependencyGraph::new();
    graph.build(&repo);
    let cycles = graph.detect_cycles();

    assert!(!cycles.is_empty());
    assert!(graph.is_cyclic("User"));
    assert!(graph.is_cyclic("Post"));
  }

  #[test]
  fn test_schema_graph_integration() {
    let mut schemas = BTreeMap::new();
    schemas.insert("User".to_string(), ObjectOrReference::Object(create_simple_schema()));
    schemas.insert(
      "Post".to_string(),
      ObjectOrReference::Object(create_schema_with_ref("User")),
    );

    let spec = create_test_spec_with_schemas(schemas);
    let (mut graph, _warnings) = SchemaGraph::new(spec);

    assert!(graph.get_schema("User").is_some());
    assert!(graph.get_schema("Post").is_some());
    assert_eq!(graph.schema_names().len(), 2);

    graph.build_dependencies();
    let cycles = graph.detect_cycles();
    assert!(cycles.is_empty());
    assert!(!graph.is_cyclic("User"));
  }

  #[test]
  fn test_extract_ref_name() {
    assert_eq!(
      SchemaGraph::extract_ref_name("#/components/schemas/User"),
      Some("User".to_string())
    );
    assert_eq!(
      SchemaGraph::extract_ref_name("#/components/schemas/NestedSchema"),
      Some("NestedSchema".to_string())
    );
    assert_eq!(SchemaGraph::extract_ref_name("#/other/path"), None);
    assert_eq!(SchemaGraph::extract_ref_name("InvalidRef"), None);
  }
}

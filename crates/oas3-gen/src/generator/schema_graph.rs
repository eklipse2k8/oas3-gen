use std::{
  collections::{BTreeMap, BTreeSet},
  string::ToString,
};

use oas3::{
  Spec,
  spec::{ObjectOrReference, ObjectSchema, ParameterIn, Schema},
};

/// Graph structure for managing OpenAPI schemas and their dependencies
#[derive(Debug)]
pub(crate) struct SchemaGraph {
  /// All schemas from the OpenAPI spec
  schemas: BTreeMap<String, ObjectSchema>,
  /// Dependency graph: schema_name -> [schemas it references]
  dependencies: BTreeMap<String, BTreeSet<String>>,
  /// Schemas that are part of cycles
  cyclic_schemas: BTreeSet<String>,
  /// Collected HTTP header names from parameters
  headers: BTreeSet<String>,
  /// Reference to the original spec for resolution
  spec: Spec,
}

impl SchemaGraph {
  pub(crate) fn new(spec: Spec) -> anyhow::Result<Self> {
    let mut graph = Self {
      schemas: BTreeMap::new(),
      dependencies: BTreeMap::new(),
      cyclic_schemas: BTreeSet::new(),
      headers: BTreeSet::new(),
      spec,
    };

    if let Some(components) = &graph.spec.components {
      for (name, schema_ref) in &components.schemas {
        if let Ok(schema) = schema_ref.resolve(&graph.spec) {
          graph.schemas.insert(name.clone(), schema);
        }
      }
    }

    graph.extract_operations()?;

    Ok(graph)
  }

  /// Get a schema by name
  pub(crate) fn get_schema(&self, name: &str) -> Option<&ObjectSchema> {
    self.schemas.get(name)
  }

  /// Get all schema names
  pub(crate) fn schema_names(&self) -> Vec<&String> {
    self.schemas.keys().collect()
  }

  pub(crate) fn all_headers(&self) -> Vec<&String> {
    self.headers.iter().collect()
  }

  /// Get the spec reference
  pub(crate) fn spec(&self) -> &Spec {
    &self.spec
  }

  /// Extract schema name from a $ref string
  pub(crate) fn extract_ref_name(ref_string: &str) -> Option<String> {
    ref_string
      .strip_prefix("#/components/schemas/")
      .map(ToString::to_string)
  }

  /// Extract schema name from an ObjectOrReference if it's a $ref
  fn extract_ref_from_obj_ref(obj_ref: &ObjectOrReference<ObjectSchema>) -> Option<String> {
    match obj_ref {
      ObjectOrReference::Ref { ref_path, .. } => Self::extract_ref_name(ref_path),
      ObjectOrReference::Object(_) => None,
    }
  }

  fn extract_operations(&mut self) -> anyhow::Result<()> {
    for (_, _, operation) in self.spec.operations() {
      for parameter in &operation.parameters {
        let resolved = parameter.resolve(&self.spec)?;
        match resolved.location {
          ParameterIn::Header => {
            let header_name = resolved.name.to_lowercase();
            self.headers.insert(header_name);
          }
          ParameterIn::Path | ParameterIn::Query | ParameterIn::Cookie => {}
        }
      }
    }

    Ok(())
  }

  /// Build the dependency graph by analyzing all schema references
  pub(crate) fn build_dependencies(&mut self) {
    let schema_names: Vec<String> = self.schemas.keys().cloned().collect();

    for schema_name in schema_names {
      let mut deps = BTreeSet::new();
      if let Some(schema) = self.schemas.get(&schema_name) {
        self.collect_dependencies(schema, &mut deps);
      }
      self.dependencies.insert(schema_name, deps);
    }
  }

  /// Recursively collect all schema dependencies from a schema
  /// Extracts all $ref names that point to schemas in components/schemas
  fn collect_dependencies(&self, schema: &ObjectSchema, deps: &mut BTreeSet<String>) {
    for prop_schema in schema.properties.values() {
      self.extract_refs_from_schema_ref(prop_schema, deps);
    }

    for one_of_schema in &schema.one_of {
      self.extract_refs_from_schema_ref(one_of_schema, deps);
    }

    for any_of_schema in &schema.any_of {
      self.extract_refs_from_schema_ref(any_of_schema, deps);
    }

    for all_of_schema in &schema.all_of {
      self.extract_refs_from_schema_ref(all_of_schema, deps);
    }

    if let Some(ref items_box) = schema.items
      && let Schema::Object(ref schema_ref) = **items_box
    {
      self.extract_refs_from_schema_ref(schema_ref, deps);
    }
  }

  /// Extract all $ref names from a schema reference, recursively processing inline schemas
  fn extract_refs_from_schema_ref(&self, schema_ref: &ObjectOrReference<ObjectSchema>, deps: &mut BTreeSet<String>) {
    if let Some(ref_name) = Self::extract_ref_from_obj_ref(schema_ref) {
      deps.insert(ref_name);
    }

    if let ObjectOrReference::Object(inline_schema) = schema_ref {
      self.collect_dependencies(inline_schema, deps);
    }
  }

  /// Detect cycles in the schema dependency graph using DFS
  pub(crate) fn detect_cycles(&mut self) -> Vec<Vec<String>> {
    let mut visited = BTreeSet::new();
    let mut rec_stack = BTreeSet::new();
    let mut cycles = Vec::new();
    let mut path = Vec::new();

    let schema_names: Vec<String> = self.schemas.keys().cloned().collect();

    for schema_name in schema_names {
      if !visited.contains(&schema_name) {
        self.dfs_detect_cycle(&schema_name, &mut visited, &mut rec_stack, &mut path, &mut cycles);
      }
    }

    for cycle in &cycles {
      for schema_name in cycle {
        self.cyclic_schemas.insert(schema_name.clone());
      }
    }

    cycles
  }

  /// DFS helper for cycle detection
  fn dfs_detect_cycle(
    &self,
    node: &str,
    visited: &mut BTreeSet<String>,
    rec_stack: &mut BTreeSet<String>,
    path: &mut Vec<String>,
    cycles: &mut Vec<Vec<String>>,
  ) {
    visited.insert(node.to_string());
    rec_stack.insert(node.to_string());
    path.push(node.to_string());

    if let Some(deps) = self.dependencies.get(node) {
      for dep in deps {
        if !visited.contains(dep) {
          self.dfs_detect_cycle(dep, visited, rec_stack, path, cycles);
        } else if rec_stack.contains(dep)
          && let Some(cycle_start) = path.iter().position(|n| n == dep)
        {
          let cycle: Vec<String> = path[cycle_start..].to_vec();
          cycles.push(cycle);
        }
      }
    }

    path.pop();
    rec_stack.remove(node);
  }

  /// Check if a schema is part of a cycle
  pub(crate) fn is_cyclic(&self, schema_name: &str) -> bool {
    self.cyclic_schemas.contains(schema_name)
  }
}

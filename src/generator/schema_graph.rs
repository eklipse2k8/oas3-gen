//! Schema graph for managing OpenAPI schema dependencies
//!
//! This module handles schema storage, dependency tracking, and cycle detection.

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use oas3::{Spec, spec::ObjectSchema};

/// Graph structure for managing OpenAPI schemas and their dependencies
#[derive(Debug)]
pub struct SchemaGraph {
  /// All schemas from the OpenAPI spec
  schemas: BTreeMap<String, ObjectSchema>,
  /// Dependency graph: schema_name -> [schemas it references]
  dependencies: BTreeMap<String, BTreeSet<String>>,
  /// Schemas that are part of cycles
  cyclic_schemas: BTreeSet<String>,
  /// Reference to the original spec for resolution
  spec: Spec,
}

impl SchemaGraph {
  pub fn new(spec: Spec) -> anyhow::Result<Self> {
    let mut graph = Self {
      schemas: BTreeMap::new(),
      dependencies: BTreeMap::new(),
      cyclic_schemas: BTreeSet::new(),
      spec,
    };

    // Extract all schemas from components/schemas
    if let Some(components) = &graph.spec.components {
      for (name, schema_ref) in &components.schemas {
        if let Ok(schema) = schema_ref.resolve(&graph.spec) {
          graph.schemas.insert(name.clone(), schema);
        }
      }
    }

    Ok(graph)
  }

  /// Get a schema by name
  pub fn get_schema(&self, name: &str) -> Option<&ObjectSchema> {
    self.schemas.get(name)
  }

  /// Get all schema names
  pub fn schema_names(&self) -> Vec<&String> {
    self.schemas.keys().collect()
  }

  /// Get the spec reference
  pub fn spec(&self) -> &Spec {
    &self.spec
  }

  /// Extract schema name from a $ref string
  pub fn extract_ref_name(ref_string: &str) -> Option<String> {
    // Format: "#/components/schemas/SchemaName"
    ref_string.strip_prefix("#/components/schemas/").map(|s| s.to_string())
  }

  /// Build the dependency graph by analyzing all schema references
  pub fn build_dependencies(&mut self) {
    let schema_names: Vec<String> = self.schemas.keys().cloned().collect();

    for schema_name in schema_names {
      let mut deps = BTreeSet::new();
      if let Some(schema) = self.schemas.get(&schema_name) {
        self.collect_dependencies(schema, &mut deps);
      }
      self.dependencies.insert(schema_name, deps);
    }
  }

  /// Iteratively collect all schema dependencies from a schema
  /// Uses a work queue to avoid stack overflow on deeply nested schemas
  fn collect_dependencies(&self, schema: &ObjectSchema, deps: &mut BTreeSet<String>) {
    // Use a work queue and visited set to avoid deep recursion
    let mut queue = VecDeque::new();
    let mut visited = BTreeSet::new();

    // Start with the initial schema
    queue.push_back(schema.clone());

    while let Some(current_schema) = queue.pop_front() {
      // Generate a simple hash/key for this schema to track if we've visited it
      // We'll use a combination of its properties to create a unique identifier
      let schema_key = format!(
        "{:?}_{:?}_{:?}_{:?}",
        current_schema.title,
        current_schema.properties.len(),
        current_schema.one_of.len(),
        current_schema.any_of.len()
      );

      // Skip if we've already processed this exact schema structure
      if visited.contains(&schema_key) {
        continue;
      }
      visited.insert(schema_key);

      // Check properties
      for prop_schema in current_schema.properties.values() {
        if let Ok(resolved) = prop_schema.resolve(&self.spec) {
          if let Some(ref title) = resolved.title {
            deps.insert(title.clone());
          }
          queue.push_back(resolved);
        }
      }

      // Check oneOf
      for one_of_schema in &current_schema.one_of {
        if let Ok(resolved) = one_of_schema.resolve(&self.spec) {
          if let Some(ref title) = resolved.title {
            deps.insert(title.clone());
          }
          queue.push_back(resolved);
        }
      }

      // Check anyOf
      for any_of_schema in &current_schema.any_of {
        if let Ok(resolved) = any_of_schema.resolve(&self.spec) {
          if let Some(ref title) = resolved.title {
            deps.insert(title.clone());
          }
          queue.push_back(resolved);
        }
      }

      // Check allOf
      for all_of_schema in &current_schema.all_of {
        if let Ok(resolved) = all_of_schema.resolve(&self.spec) {
          if let Some(ref title) = resolved.title {
            deps.insert(title.clone());
          }
          queue.push_back(resolved);
        }
      }
    }
  }

  /// Detect cycles in the schema dependency graph using DFS
  pub fn detect_cycles(&mut self) -> Vec<Vec<String>> {
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

    // Mark all schemas involved in cycles
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
        } else if rec_stack.contains(dep) {
          // Found a cycle! Extract the cycle from the path
          if let Some(cycle_start) = path.iter().position(|n| n == dep) {
            let cycle: Vec<String> = path[cycle_start..].to_vec();
            cycles.push(cycle);
          }
        }
      }
    }

    path.pop();
    rec_stack.remove(node);
  }

  /// Check if a schema is part of a cycle
  pub fn is_cyclic(&self, schema_name: &str) -> bool {
    self.cyclic_schemas.contains(schema_name)
  }

  /// Get dependencies of a schema
  pub fn get_dependencies(&self, schema_name: &str) -> Option<&BTreeSet<String>> {
    self.dependencies.get(schema_name)
  }
}

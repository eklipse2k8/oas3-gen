use std::{
  collections::{BTreeMap, BTreeSet, HashMap, HashSet},
  string::ToString,
};

use oas3::{
  Spec,
  spec::{Discriminator, ObjectOrReference, ObjectSchema, Schema, SchemaTypeSet},
};

use super::orchestrator::GenerationWarning;
use crate::generator::{
  converter::SchemaExt, naming::identifiers::to_rust_type_name, operation_registry::OperationRegistry,
};

const SCHEMA_REF_PREFIX: &str = "#/components/schemas/";

type UnionFingerprints = HashMap<BTreeSet<String>, String>;

#[derive(Debug, Clone)]
pub(crate) struct MergedSchema {
  pub schema: ObjectSchema,
  pub discriminator_parent: Option<String>,
}

#[derive(Debug)]
pub(crate) struct ReferenceExtractor;

impl ReferenceExtractor {
  pub(crate) fn extract_ref_name_from_string(ref_string: &str) -> Option<String> {
    ref_string.strip_prefix(SCHEMA_REF_PREFIX).map(ToString::to_string)
  }

  pub(crate) fn extract_ref_name_from_obj_ref(obj_ref: &ObjectOrReference<ObjectSchema>) -> Option<String> {
    match obj_ref {
      ObjectOrReference::Ref { ref_path, .. } => Self::extract_ref_name_from_string(ref_path),
      ObjectOrReference::Object(_) => None,
    }
  }

  pub(crate) fn extract_from_schema(
    schema: &ObjectSchema,
    fingerprints: Option<&UnionFingerprints>,
  ) -> BTreeSet<String> {
    let mut refs = BTreeSet::new();

    Self::collect_from_properties(schema, &mut refs, fingerprints);
    Self::collect_from_combinators(schema, &mut refs, fingerprints);
    Self::collect_from_items(schema, &mut refs, fingerprints);

    refs
  }

  pub(crate) fn extract_from_schema_ref(
    schema_ref: &ObjectOrReference<ObjectSchema>,
    refs: &mut BTreeSet<String>,
    fingerprints: Option<&UnionFingerprints>,
  ) {
    if let Some(ref_name) = Self::extract_ref_name_from_obj_ref(schema_ref) {
      refs.insert(ref_name);
    }

    if let ObjectOrReference::Object(inline_schema) = schema_ref {
      let inline_refs = Self::extract_from_schema(inline_schema, fingerprints);
      refs.extend(inline_refs);
    }
  }

  fn collect_from_properties(
    schema: &ObjectSchema,
    refs: &mut BTreeSet<String>,
    fingerprints: Option<&UnionFingerprints>,
  ) {
    for prop_schema in schema.properties.values() {
      Self::extract_from_schema_ref(prop_schema, refs, fingerprints);
    }
  }

  fn collect_from_combinators(
    schema: &ObjectSchema,
    refs: &mut BTreeSet<String>,
    fingerprints: Option<&UnionFingerprints>,
  ) {
    for schema_ref in schema.one_of.iter().chain(&schema.any_of).chain(&schema.all_of) {
      Self::extract_from_schema_ref(schema_ref, refs, fingerprints);
    }

    if let Some(map) = fingerprints {
      Self::insert_union_fingerprint_ref(&schema.one_of, refs, map);
      Self::insert_union_fingerprint_ref(&schema.any_of, refs, map);
    }
  }

  fn insert_union_fingerprint_ref(
    variants: &[ObjectOrReference<ObjectSchema>],
    refs: &mut BTreeSet<String>,
    fingerprints: &UnionFingerprints,
  ) {
    if !variants.is_empty() {
      let fp = SchemaRegistry::extract_fingerprint(variants);
      if let Some(name) = fingerprints.get(&fp) {
        refs.insert(name.clone());
      }
    }
  }

  fn collect_from_items(schema: &ObjectSchema, refs: &mut BTreeSet<String>, fingerprints: Option<&UnionFingerprints>) {
    if let Some(ref items_box) = schema.items
      && let Schema::Object(ref schema_ref) = **items_box
    {
      Self::extract_from_schema_ref(schema_ref, refs, fingerprints);
    }
  }
}

/// Central registry for OpenAPI schemas with dependency tracking and analysis.
///
/// This structure serves as a comprehensive index for all schemas defined in an OpenAPI
/// specification. It provides:
///
/// - Schema storage and retrieval by name
/// - Dependency graph construction and traversal
/// - Cycle detection using depth-first search
/// - Discriminator metadata caching for polymorphic types
/// - Union type fingerprinting for implicit dependencies
/// - Operation reachability analysis to determine which schemas are actually used
///
/// The registry uses BTreeMap for deterministic ordering, ensuring consistent code
/// generation across runs regardless of schema definition order in the OpenAPI spec.
#[derive(Debug)]
pub(crate) struct SchemaRegistry {
  schemas: BTreeMap<String, ObjectSchema>,
  merged_schemas: BTreeMap<String, MergedSchema>,
  discriminator_parents: BTreeMap<String, (String, String, String)>,
  dependencies: BTreeMap<String, BTreeSet<String>>,
  cyclic_schemas: BTreeSet<String>,
  discriminator_cache: BTreeMap<String, (String, String)>,
  inheritance_depths: HashMap<String, usize>,
  spec: Spec,
  union_fingerprints: UnionFingerprints,
  cached_schema_names: HashSet<String>,
}

impl SchemaRegistry {
  /// Creates a new schema registry from an OpenAPI specification.
  ///
  /// This constructor performs the following initialization:
  /// 1. Resolves all schema references from the spec components
  /// 2. Builds a discriminator cache for polymorphic type handling
  /// 3. Generates union fingerprints to track implicit dependencies
  ///
  /// Returns a tuple of (registry, warnings) where warnings contain any schemas
  /// that failed to resolve.
  pub(crate) fn new(spec: Spec) -> (Self, Vec<GenerationWarning>) {
    let mut schemas = BTreeMap::new();
    let mut warnings = vec![];

    if let Some(components) = &spec.components {
      for (name, schema_ref) in &components.schemas {
        match schema_ref.resolve(&spec) {
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

    let discriminator_cache = Self::build_discriminator_cache(&schemas);
    let union_fingerprints = Self::build_union_fingerprints(&schemas);

    let cached_schema_names = schemas
      .keys()
      .flat_map(|schema_name| {
        let rust_name = to_rust_type_name(schema_name);
        [schema_name.clone(), rust_name]
      })
      .collect();

    (
      Self {
        schemas,
        merged_schemas: BTreeMap::new(),
        discriminator_parents: BTreeMap::new(),
        dependencies: BTreeMap::new(),
        cyclic_schemas: BTreeSet::new(),
        discriminator_cache,
        inheritance_depths: HashMap::new(),
        spec,
        union_fingerprints,
        cached_schema_names,
      },
      warnings,
    )
  }

  /// Builds a cache mapping child schema names to their discriminator metadata.
  ///
  /// In OpenAPI, discriminators define polymorphic relationships where a parent schema
  /// uses a discriminator field to identify which child schema variant is present.
  /// This cache maps each child schema to its (discriminator_field, discriminator_value)
  /// tuple, enabling efficient lookup during code generation.
  ///
  /// Returns a map where keys are child schema names and values are tuples of
  /// (discriminator field name, discriminator value for this child).
  fn build_discriminator_cache(schemas: &BTreeMap<String, ObjectSchema>) -> BTreeMap<String, (String, String)> {
    let mut cache = BTreeMap::new();

    for candidate_schema in schemas.values() {
      if let Some(d) = &candidate_schema.discriminator
        && let Some(mapping) = &d.mapping
      {
        for (val, ref_path) in mapping {
          if let Some(schema_name) = ReferenceExtractor::extract_ref_name_from_string(ref_path) {
            cache.insert(schema_name, (d.property_name.clone(), val.clone()));
          }
        }
      }
    }

    cache
  }

  /// Builds a fingerprint map for union types to detect implicit dependencies.
  ///
  /// When multiple schemas use the same set of variants in oneOf/anyOf, they represent
  /// the same logical union type. This method creates fingerprints (sets of variant names)
  /// and maps them to the first schema name that declares each unique union. This enables
  /// the dependency tracker to recognize when a schema implicitly depends on a named union
  /// type even without an explicit $ref.
  ///
  /// Only unions with 2+ variants are fingerprinted to avoid false positives.
  ///
  /// Returns a map from variant name sets to the schema name that first declared that union.
  fn build_union_fingerprints(schemas: &BTreeMap<String, ObjectSchema>) -> UnionFingerprints {
    let mut map = UnionFingerprints::new();
    for (name, schema) in schemas {
      let fp_one = Self::extract_fingerprint(&schema.one_of);
      if fp_one.len() >= 2 {
        map.entry(fp_one).or_insert(name.clone());
      }

      let fp_any = Self::extract_fingerprint(&schema.any_of);
      if fp_any.len() >= 2 {
        map.entry(fp_any).or_insert(name.clone());
      }
    }
    map
  }

  /// Extracts a fingerprint (set of schema names) from union variants.
  ///
  /// A fingerprint is the set of all schema names referenced in a oneOf/anyOf/allOf
  /// construct. Two unions with the same fingerprint represent the same logical type.
  pub(crate) fn extract_fingerprint(variants: &[ObjectOrReference<ObjectSchema>]) -> BTreeSet<String> {
    variants
      .iter()
      .filter_map(ReferenceExtractor::extract_ref_name_from_obj_ref)
      .collect()
  }

  /// Retrieves a schema by name.
  pub(crate) fn get_schema(&self, name: &str) -> Option<&ObjectSchema> {
    self.schemas.get(name)
  }

  /// Checks if a name corresponds to a known schema in the graph.
  pub(crate) fn is_schema_name(&self, name: &str) -> bool {
    self.cached_schema_names.contains(name)
  }

  /// Returns all schema names in sorted order.
  pub(crate) fn schema_names(&self) -> Vec<&String> {
    self.schemas.keys().collect()
  }

  /// Returns the original OpenAPI specification.
  pub(crate) fn spec(&self) -> &Spec {
    &self.spec
  }

  /// Extracts the schema name from a reference string.
  ///
  /// Reference strings have the format "#/components/schemas/{name}".
  /// This method strips the prefix and returns the schema name if valid.
  pub(crate) fn extract_ref_name(ref_string: &str) -> Option<String> {
    ReferenceExtractor::extract_ref_name_from_string(ref_string)
  }

  /// Builds the dependency graph by analyzing all schema references.
  ///
  /// This method scans each schema to extract all schemas it depends on, including:
  /// - Direct references via $ref in properties
  /// - References in oneOf/anyOf/allOf combinators
  /// - References in array items
  /// - Implicit dependencies via union fingerprints
  ///
  /// The dependency graph is stored as an adjacency list where each schema name maps
  /// to the set of schema names it depends on. This graph is used for topological
  /// sorting and cycle detection.
  pub(crate) fn build_dependencies(&mut self) {
    for schema_name in self.schemas.keys() {
      let deps = self
        .schemas
        .get(schema_name)
        .map(|s| ReferenceExtractor::extract_from_schema(s, Some(&self.union_fingerprints)))
        .unwrap_or_default();

      self.dependencies.insert(schema_name.clone(), deps);
    }

    self.compute_all_inheritance_depths();
    self.build_merged_schemas();
    self.build_discriminator_parents();
  }

  fn compute_all_inheritance_depths(&mut self) {
    let schema_names: Vec<_> = self.schemas.keys().cloned().collect();
    for name in schema_names {
      self.compute_depth_recursive(&name);
    }
  }

  fn compute_depth_recursive(&mut self, schema_name: &str) -> usize {
    if let Some(&depth) = self.inheritance_depths.get(schema_name) {
      return depth;
    }

    let parent_names: Vec<String> = self
      .schemas
      .get(schema_name)
      .map(|schema| {
        schema
          .all_of
          .iter()
          .filter_map(ReferenceExtractor::extract_ref_name_from_obj_ref)
          .collect()
      })
      .unwrap_or_default();

    let depth = if parent_names.is_empty() {
      0
    } else {
      parent_names
        .into_iter()
        .map(|parent| self.compute_depth_recursive(&parent))
        .max()
        .unwrap_or(0)
        + 1
    };

    self.inheritance_depths.insert(schema_name.to_string(), depth);
    depth
  }

  fn build_merged_schemas(&mut self) {
    let mut sorted_names: Vec<_> = self.schemas.keys().cloned().collect();
    sorted_names.sort_by_key(|name| self.get_inheritance_depth(name));

    for schema_name in sorted_names {
      let Some(schema) = self.schemas.get(&schema_name).cloned() else {
        continue;
      };

      let merged = self.merge_schema(&schema);
      self.merged_schemas.insert(schema_name, merged);
    }
  }

  fn build_discriminator_parents(&mut self) {
    let mut map = BTreeMap::new();

    for (child_name, merged) in &self.merged_schemas {
      if let Some(parent_name) = &merged.discriminator_parent
        && let Some((field, value)) = self.discriminator_cache.get(child_name)
      {
        map.insert(child_name.clone(), (parent_name.clone(), field.clone(), value.clone()));
      }
    }

    self.discriminator_parents = map;
  }

  fn merge_schema(&self, schema: &ObjectSchema) -> MergedSchema {
    if schema.all_of.is_empty() {
      return MergedSchema {
        schema: schema.clone(),
        discriminator_parent: None,
      };
    }

    let (merged_schema, parent_name) = self.do_merge_all_of(schema);

    MergedSchema {
      schema: merged_schema,
      discriminator_parent: parent_name,
    }
  }

  fn do_merge_all_of(&self, schema: &ObjectSchema) -> (ObjectSchema, Option<String>) {
    let mut merged_properties = BTreeMap::new();
    let mut merged_required = BTreeSet::new();
    let mut merged_discriminator: Option<Discriminator> = None;
    let mut merged_schema_type: Option<SchemaTypeSet> = None;
    let mut discriminator_parent = None;

    for all_of_ref in &schema.all_of {
      match all_of_ref {
        ObjectOrReference::Ref { ref_path, .. } => {
          if let Some(parent_name) = Self::extract_ref_name(ref_path) {
            let parent_schema = self
              .merged_schemas
              .get(&parent_name)
              .map(|m| &m.schema)
              .or_else(|| self.schemas.get(&parent_name));

            if let Some(parent) = parent_schema {
              if parent.discriminator.is_some() && parent.is_discriminated_base_type() {
                discriminator_parent = Some(parent_name.clone());
              }

              Self::merge_properties_from(
                parent,
                &mut merged_properties,
                &mut merged_required,
                &mut merged_discriminator,
                &mut merged_schema_type,
              );
            }
          }
        }
        ObjectOrReference::Object(inline_schema) => {
          Self::merge_properties_from(
            inline_schema,
            &mut merged_properties,
            &mut merged_required,
            &mut merged_discriminator,
            &mut merged_schema_type,
          );
        }
      }
    }

    // Merge properties from anyOf (intersection behavior)
    for any_of_ref in &schema.any_of {
      self.merge_optional_properties(any_of_ref, &mut merged_properties);
    }

    // Merge properties from oneOf (intersection behavior)
    for one_of_ref in &schema.one_of {
      self.merge_optional_properties(one_of_ref, &mut merged_properties);
    }

    Self::merge_properties_from(
      schema,
      &mut merged_properties,
      &mut merged_required,
      &mut merged_discriminator,
      &mut merged_schema_type,
    );

    let mut merged = schema.clone();
    merged.properties = merged_properties;
    merged.required = merged_required.into_iter().collect();
    merged.discriminator = merged_discriminator;
    if merged_schema_type.is_some() {
      merged.schema_type = merged_schema_type;
    }
    merged.all_of.clear();

    if merged.additional_properties.is_none() {
      for all_of_ref in &schema.all_of {
        if let Ok(parent) = all_of_ref.resolve(&self.spec)
          && parent.additional_properties.is_some()
        {
          merged.additional_properties.clone_from(&parent.additional_properties);
          break;
        }
      }
    }

    (merged, discriminator_parent)
  }

  fn merge_properties_from(
    source: &ObjectSchema,
    properties: &mut BTreeMap<String, ObjectOrReference<ObjectSchema>>,
    required: &mut BTreeSet<String>,
    discriminator: &mut Option<Discriminator>,
    schema_type: &mut Option<SchemaTypeSet>,
  ) {
    for (name, prop) in &source.properties {
      properties.insert(name.clone(), prop.clone());
    }
    required.extend(source.required.iter().cloned());
    if source.discriminator.is_some() {
      discriminator.clone_from(&source.discriminator);
    }
    if source.schema_type.is_some() {
      schema_type.clone_from(&source.schema_type);
    }
  }

  fn merge_optional_properties(
    &self,
    schema_ref: &ObjectOrReference<ObjectSchema>,
    properties: &mut BTreeMap<String, ObjectOrReference<ObjectSchema>>,
  ) {
    let schema = match schema_ref {
      ObjectOrReference::Ref { ref_path, .. } => {
        if let Some(name) = Self::extract_ref_name(ref_path) {
          self
            .merged_schemas
            .get(&name)
            .map(|m| &m.schema)
            .or_else(|| self.schemas.get(&name))
        } else {
          None
        }
      }
      ObjectOrReference::Object(s) => Some(s),
    };

    if let Some(source) = schema {
      for (name, prop) in &source.properties {
        // Only insert if not already present (prioritize allOf and base schema)
        properties.entry(name.clone()).or_insert_with(|| prop.clone());
      }
    }
  }

  pub(crate) fn get_merged_schema(&self, name: &str) -> Option<&MergedSchema> {
    self.merged_schemas.get(name)
  }

  pub(crate) fn get_effective_schema(&self, name: &str) -> Option<&ObjectSchema> {
    self
      .merged_schemas
      .get(name)
      .map(|m| &m.schema)
      .or_else(|| self.schemas.get(name))
  }

  pub(crate) fn merge_inline_all_of(&self, schema: &ObjectSchema) -> ObjectSchema {
    if schema.all_of.is_empty() {
      return schema.clone();
    }
    let (merged, _) = self.do_merge_all_of(schema);
    merged
  }

  pub(crate) fn get_discriminator_parent(&self, name: &str) -> Option<&(String, String, String)> {
    self.discriminator_parents.get(name)
  }

  pub(crate) fn merged_schemas_ref(&self) -> &BTreeMap<String, MergedSchema> {
    &self.merged_schemas
  }

  /// Detects cycles in the schema dependency graph using depth-first search.
  ///
  /// Algorithm: Modified DFS with a recursion stack to detect back edges.
  /// - Maintains a visited set to track completed nodes
  /// - Uses a recursion stack to track the current DFS path
  /// - When a back edge is found (edge to a node in recursion stack), a cycle exists
  /// - Records the cyclic path from the back edge point to the current node
  ///
  /// Cyclic schemas require special handling in code generation (e.g., Box<T> to break
  /// the cycle in Rust's type system).
  ///
  /// Returns a vector of cycles, where each cycle is a vector of schema names forming
  /// the cycle path. All schemas appearing in any cycle are marked in cyclic_schemas.
  pub(crate) fn detect_cycles(&mut self) -> Vec<Vec<String>> {
    let mut visited = BTreeSet::new();
    let mut recursion_stack = BTreeSet::new();
    let mut path = vec![];
    let mut cycles = vec![];

    let nodes: Vec<String> = self.dependencies.keys().cloned().collect();

    for node in nodes {
      if !visited.contains(&node) {
        Self::visit_for_cycles(
          &node,
          &self.dependencies,
          &mut visited,
          &mut recursion_stack,
          &mut path,
          &mut cycles,
        );
      }
    }

    for cycle in &cycles {
      for schema_name in cycle {
        self.cyclic_schemas.insert(schema_name.clone());
      }
    }

    cycles
  }

  /// DFS helper for cycle detection.
  ///
  /// This recursive function implements the cycle detection algorithm:
  /// 1. Mark current node as visited and add to recursion stack
  /// 2. For each dependency:
  ///    - If unvisited, recursively visit it
  ///    - If in recursion stack (back edge), we found a cycle
  /// 3. Remove from recursion stack when returning (backtracking)
  ///
  /// The path vector maintains the current DFS path for cycle reconstruction.
  fn visit_for_cycles(
    node: &str,
    dependencies: &BTreeMap<String, BTreeSet<String>>,
    visited: &mut BTreeSet<String>,
    recursion_stack: &mut BTreeSet<String>,
    path: &mut Vec<String>,
    cycles: &mut Vec<Vec<String>>,
  ) {
    visited.insert(node.to_string());
    recursion_stack.insert(node.to_string());
    path.push(node.to_string());

    if let Some(deps) = dependencies.get(node) {
      for dep in deps {
        if !visited.contains(dep) {
          Self::visit_for_cycles(dep, dependencies, visited, recursion_stack, path, cycles);
        } else if recursion_stack.contains(dep)
          && let Some(start_pos) = path.iter().position(|n| n == dep)
        {
          let cycle: Vec<String> = path[start_pos..].to_vec();
          cycles.push(cycle);
        }
      }
    }

    path.pop();
    recursion_stack.remove(node);
  }

  /// Checks if a schema is part of any dependency cycle.
  ///
  /// Cyclic schemas require special code generation (e.g., Box<T> in Rust).
  pub(crate) fn is_cyclic(&self, schema_name: &str) -> bool {
    self.cyclic_schemas.contains(schema_name)
  }

  /// Returns the inheritance depth of a schema (0 if no allOf, 1+ for each level).
  ///
  /// Depths are precomputed during `build_dependencies()` for O(1) lookup.
  pub(crate) fn get_inheritance_depth(&self, schema_name: &str) -> usize {
    self.inheritance_depths.get(schema_name).copied().unwrap_or(0)
  }

  /// Retrieves the discriminator metadata for a child schema.
  ///
  /// Returns None if the schema is not a discriminated child, or Some((field, value))
  /// where field is the discriminator property name and value is this schema's
  /// discriminator value.
  pub(crate) fn get_discriminator_mapping(&self, schema_name: &str) -> Option<&(String, String)> {
    self.discriminator_cache.get(schema_name)
  }

  /// Looks up a schema name by its union fingerprint.
  ///
  /// A union fingerprint is the set of schema names referenced in oneOf/anyOf variants.
  /// This provides O(1) lookup instead of iterating all schemas.
  pub(crate) fn lookup_union_by_fingerprint(&self, fingerprint: &BTreeSet<String>) -> Option<&String> {
    self.union_fingerprints.get(fingerprint)
  }

  /// Computes all schemas reachable from operations and their transitive dependencies.
  ///
  /// Algorithm: Two-phase reachability analysis
  /// 1. Direct Phase: Scan all operations to find directly referenced schemas
  ///    - Parameters, request bodies, responses
  ///    - Both explicit $ref and inline schemas
  /// 2. Expansion Phase: Transitively expand using the dependency graph
  ///    - For each reachable schema, add all its dependencies
  ///    - Continue until no new schemas are discovered
  ///
  /// This enables --only-operations mode where we generate code only for schemas
  /// actually used by API operations, reducing generated code size.
  ///
  /// Returns the complete set of reachable schema names in sorted order.
  pub(crate) fn get_operation_reachable_schemas(&self, operation_registry: &OperationRegistry) -> BTreeSet<String> {
    let mut reachable = BTreeSet::new();

    for (_, _, _, operation, _) in operation_registry.operations_with_details() {
      Self::collect_refs_from_operation(operation, &self.spec, &mut reachable, &self.union_fingerprints);
    }

    self.expand_with_dependencies(&reachable)
  }

  /// Extracts all schema references from a single operation.
  ///
  /// Scans parameters, request body, and all response types for schema references.
  fn collect_refs_from_operation(
    operation: &oas3::spec::Operation,
    spec: &Spec,
    refs: &mut BTreeSet<String>,
    fingerprints: &UnionFingerprints,
  ) {
    for param in &operation.parameters {
      if let Ok(resolved_param) = param.resolve(spec)
        && let Some(ref schema_ref) = resolved_param.schema
      {
        ReferenceExtractor::extract_from_schema_ref(schema_ref, refs, Some(fingerprints));
      }
    }

    if let Some(ref request_body_ref) = operation.request_body
      && let Ok(request_body) = request_body_ref.resolve(spec)
    {
      for media_type in request_body.content.values() {
        if let Some(ref schema_ref) = media_type.schema {
          ReferenceExtractor::extract_from_schema_ref(schema_ref, refs, Some(fingerprints));
        }
      }
    }

    if let Some(ref responses) = operation.responses {
      for response_ref in responses.values() {
        if let Ok(response) = response_ref.resolve(spec) {
          for media_type in response.content.values() {
            if let Some(ref schema_ref) = media_type.schema {
              ReferenceExtractor::extract_from_schema_ref(schema_ref, refs, Some(fingerprints));
            }
          }
        }
      }
    }
  }

  /// Expands a set of schemas with their transitive dependencies.
  ///
  /// Algorithm: Iterative graph traversal
  /// - Start with initial set of schema names
  /// - For each schema, look up its dependencies
  /// - Add unvisited dependencies to the worklist
  /// - Continue until worklist is empty
  ///
  /// Time complexity: O(V + E) where V = number of schemas, E = number of dependencies
  /// Space complexity: O(V) for the expanded set and worklist
  fn expand_with_dependencies(&self, initial_refs: &BTreeSet<String>) -> BTreeSet<String> {
    let mut expanded = BTreeSet::new();
    let mut to_visit: Vec<String> = initial_refs.iter().cloned().collect();

    while let Some(schema_name) = to_visit.pop() {
      if expanded.insert(schema_name.clone())
        && let Some(deps) = self.dependencies.get(&schema_name)
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

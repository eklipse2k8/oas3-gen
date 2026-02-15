use std::collections::{BTreeMap, BTreeSet};

use oas3::{
  Spec,
  spec::{Discriminator, ObjectOrReference, ObjectSchema, Operation, Schema, SchemaTypeSet},
};
use petgraph::{algo::kosaraju_scc, graphmap::DiGraphMap, visit::Dfs};

use crate::{
  generator::{
    metrics::{GenerationStats, GenerationWarning},
    naming::name_index::{ScanResult, TypeNameIndex},
    operation_registry::OperationRegistry,
  },
  utils::{
    SchemaExt, UnionFingerprints, extract_schema_ref_name, extract_union_fingerprint, parse_schema_ref_path,
    schema_ext::SchemaExtIters,
  },
};

/// Identifies how a schema maps to a discriminator value in a polymorphic hierarchy.
///
/// In OpenAPI discriminator mappings, child schemas are identified by a specific
/// property value. This structure captures that relationship for code generation
/// of tagged unions.
#[derive(Debug, Clone)]
pub(crate) struct DiscriminatorMapping {
  /// The name of the property used as the discriminator (e.g., `"petType"`).
  pub field_name: String,
  /// The string value that identifies this schema in the discriminator (e.g., `"cat"`).
  pub field_value: String,
}

/// A flattened schema resulting from merging inheritance hierarchies.
///
/// OpenAPI schemas may use `allOf` to compose types from multiple parent schemas.
/// This structure represents the final merged result with all inherited properties
/// resolved and combined.
#[derive(Debug, Clone)]
pub(crate) struct MergedSchema {
  /// The merged schema containing all properties from the hierarchy.
  pub schema: ObjectSchema,
  /// The parent schema name if this schema participates in a discriminated union.
  pub discriminator_parent: Option<String>,
}

/// Accumulates properties during schema inheritance merging.
///
/// Used internally to progressively combine properties, required fields, and
/// discriminators from parent schemas when flattening `allOf` hierarchies.
#[derive(Default)]
struct MergeAccumulator {
  properties: BTreeMap<String, ObjectOrReference<ObjectSchema>>,
  required: BTreeSet<String>,
  discriminator: Option<Discriminator>,
  schema_type: Option<SchemaTypeSet>,
  additional_properties: Option<Schema>,
  discriminator_parent: Option<String>,
}

impl MergeAccumulator {
  /// Merges all fields from the source schema into the accumulator.
  ///
  /// Properties are overwritten, required fields are extended, and
  /// discriminator/type information is preserved from the source.
  fn merge_from(&mut self, source: &ObjectSchema) {
    for (name, prop) in &source.properties {
      self.properties.insert(name.clone(), prop.clone());
    }
    self.required.extend(source.required.iter().cloned());
    if source.discriminator.is_some() {
      self.discriminator.clone_from(&source.discriminator);
    }
    if source.schema_type.is_some() {
      self.schema_type.clone_from(&source.schema_type);
    }
    if self.additional_properties.is_none() && source.additional_properties.is_some() {
      self.additional_properties.clone_from(&source.additional_properties);
    }
  }

  /// Merges optional fields from the source schema into the accumulator.
  ///
  /// Properties are only added if they do not already exist, making this
  /// suitable for `anyOf` composition where fields may be optional.
  fn merge_optional_from(&mut self, source: &ObjectSchema) {
    for (name, prop) in &source.properties {
      self.properties.entry(name.clone()).or_insert_with(|| prop.clone());
    }
  }

  /// Produces a merged schema from the accumulated state.
  ///
  /// Combines the accumulated properties and metadata into a final
  /// [`ObjectSchema`], preserving the base schema's identity while
  /// replacing its properties with the merged set.
  fn into_schema(self, base: &ObjectSchema) -> ObjectSchema {
    let mut result = base.clone();
    result.properties = self.properties;
    result.required = self.required.into_iter().collect();
    result.discriminator = self.discriminator;
    if self.schema_type.is_some() {
      result.schema_type = self.schema_type;
    }
    result.all_of.clear();
    if result.additional_properties.is_none() {
      result.additional_properties = self.additional_properties;
    }
    result
  }
}

#[derive(Debug)]
pub(crate) struct SchemaRegistry {
  schemas: BTreeMap<String, ObjectSchema>,
  merged_schemas: BTreeMap<String, MergedSchema>,
  discriminator_parents: BTreeMap<String, String>,
  dependencies: BTreeMap<String, BTreeSet<String>>,
  cyclic_schemas: BTreeSet<String>,
  discriminator_cache: BTreeMap<String, DiscriminatorMapping>,
  inheritance_depths: BTreeMap<String, usize>,
  spec: Spec,
}

impl SchemaRegistry {
  /// Creates a new schema registry from an OpenAPI specification.
  ///
  /// Resolves all schema references in `components.schemas` and populates
  /// the registry with raw schema definitions. Records warnings for any
  /// schemas that fail to resolve.
  ///
  /// The returned registry requires initialization via [`initialize`]
  /// before being used for code generation.
  ///
  /// [`initialize`]: SchemaRegistry::initialize
  pub(crate) fn new(spec: &Spec, stats: &mut GenerationStats) -> Self {
    let mut schemas = BTreeMap::new();

    if let Some(components) = &spec.components {
      for (name, schema_ref) in &components.schemas {
        match schema_ref.resolve(spec) {
          Ok(schema) => {
            schemas.insert(name.clone(), schema);
          }
          Err(error) => {
            stats.record_warning(GenerationWarning::SchemaConversionFailed {
              schema_name: name.clone(),
              error: error.to_string(),
            });
          }
        }
      }
    }

    Self {
      schemas: schemas.clone(),
      merged_schemas: BTreeMap::new(),
      discriminator_parents: BTreeMap::new(),
      dependencies: BTreeMap::new(),
      cyclic_schemas: BTreeSet::new(),
      discriminator_cache: Self::build_discriminator_cache(&schemas, stats),
      inheritance_depths: BTreeMap::new(),
      spec: spec.clone(),
    }
  }

  /// Fully initializes the registry for code generation.
  ///
  /// Builds the dependency graph, detects cycles, and optionally computes
  /// which schemas are reachable from API operations. This method must be
  /// called after construction and before the registry is used for
  /// type resolution.
  ///
  /// Returns a tuple containing:
  /// - A list of detected dependency cycles (each cycle is a list of schema names)
  /// - An optional set of reachable schema names (if `include_all` is false)
  pub(crate) fn initialize(
    &mut self,
    operation_registry: &OperationRegistry,
    include_all: bool,
    union_fingerprints: &UnionFingerprints,
  ) -> (Vec<Vec<String>>, Option<BTreeSet<String>>) {
    self.build_dependencies(union_fingerprints);
    let cycle_details = self.detect_cycles();
    let reachable = if include_all {
      None
    } else {
      Some(self.reachable(operation_registry, union_fingerprints))
    };
    (cycle_details, reachable)
  }

  /// Returns a reference to the OpenAPI specification.
  pub(crate) fn spec(&self) -> &Spec {
    &self.spec
  }

  /// Returns a raw schema by name.
  ///
  /// Returns the original schema as defined in the OpenAPI specification,
  /// without any inheritance flattening applied.
  pub(crate) fn get(&self, name: &str) -> Option<&ObjectSchema> {
    self.schemas.get(name)
  }

  /// Returns whether a schema with the given name exists in the registry.
  pub(crate) fn contains(&self, name: &str) -> bool {
    self.schemas.contains_key(name)
  }

  /// Returns all schema names in the registry.
  ///
  /// Returns a vector of references to the names of all schemas defined
  /// in the OpenAPI specification.
  pub(crate) fn keys(&self) -> Vec<&String> {
    self.schemas.keys().collect()
  }

  /// Returns all raw schemas in the registry.
  ///
  /// Provides access to the underlying map of schema names to their
  /// original [`ObjectSchema`] definitions.
  pub(crate) fn schemas(&self) -> &BTreeMap<String, ObjectSchema> {
    &self.schemas
  }

  /// Returns the merged schema for a given name.
  ///
  /// Returns the flattened version of the schema with all `all_of`
  /// inheritance resolved, if it exists.
  pub(crate) fn merged(&self, name: &str) -> Option<&MergedSchema> {
    self.merged_schemas.get(name)
  }

  /// Returns the best available schema for a given name.
  ///
  /// Returns the merged schema if available; otherwise returns the
  /// raw schema definition. This is the primary method for retrieving
  /// schema definitions for code generation.
  pub(crate) fn resolved(&self, name: &str) -> Option<&ObjectSchema> {
    self.merged(name).map(|m| &m.schema).or_else(|| self.schemas.get(name))
  }

  /// Returns the discriminator parent for a schema.
  ///
  /// For schemas that are variants of discriminated unions, returns
  /// the name of the parent schema that serves as the polymorphic base.
  pub(crate) fn parent(&self, name: &str) -> Option<&str> {
    self.discriminator_parents.get(name).map(String::as_str)
  }

  /// Returns the discriminator mapping for a schema.
  ///
  /// Returns the property name and value that identifies this schema
  /// in a discriminated union, if it participates in one.
  pub(crate) fn mapping(&self, schema_name: &str) -> Option<&DiscriminatorMapping> {
    self.discriminator_cache.get(schema_name)
  }

  /// Returns the effective discriminator mapping for a schema in OpenAPI format.
  ///
  /// If the schema has an explicit `mapping`, returns it directly. Otherwise,
  /// reconstructs the mapping from the discriminator cache (which may contain
  /// mappings synthesized from `const` values). The returned map is in the
  /// format expected by OpenAPI: `(discriminator_value -> $ref_path)`.
  ///
  /// Returns `None` if no mapping (explicit or implicit) is available.
  pub(crate) fn effective_mapping(&self, schema: &ObjectSchema) -> Option<BTreeMap<String, String>> {
    let discriminator = schema.discriminator.as_ref()?;

    if let Some(mapping) = &discriminator.mapping {
      return Some(mapping.clone());
    }

    let synthesized = schema.union_variants().try_fold(BTreeMap::new(), |mut acc, variant| {
      let ObjectOrReference::Ref { ref_path, .. } = variant else {
        return Some(acc);
      };
      let name = parse_schema_ref_path(ref_path)?;
      let dm = self.discriminator_cache.get(&name)?;
      (dm.field_name == discriminator.property_name).then(|| {
        acc.insert(dm.field_value.clone(), ref_path.clone());
        acc
      })
    })?;

    (!synthesized.is_empty()).then_some(synthesized)
  }

  /// Checks if a schema participates in a dependency cycle.
  pub(crate) fn is_cyclic(&self, schema_name: &str) -> bool {
    self.cyclic_schemas.contains(schema_name)
  }

  /// Flattens an `all_of` inheritance hierarchy into a single schema.
  ///
  /// Returns the merged schema containing all properties from the
  /// schema and its parent schemas.
  pub(crate) fn merge_all_of(&self, schema: &ObjectSchema) -> ObjectSchema {
    self.merge_schema(schema).schema
  }

  /// Recursively merges inline schemas with `all_of` composition.
  ///
  /// Similar to [`merge_all_of`] but specifically designed for inline
  /// (anonymous) schemas that may contain nested references. Recursively
  /// resolves and merges all referenced schemas.
  ///
  /// Returns an error if schema resolution fails for any reference.
  pub(crate) fn merge_inline(&self, schema: &ObjectSchema) -> anyhow::Result<ObjectSchema> {
    if schema.all_of.is_empty() {
      return Ok(schema.clone());
    }

    let mut acc = MergeAccumulator::default();

    for all_of_ref in &schema.all_of {
      match all_of_ref {
        ObjectOrReference::Ref { ref_path, .. } => {
          if let Some(name) = parse_schema_ref_path(ref_path)
            && let Some(merged) = self.merged_schemas.get(&name)
          {
            acc.merge_from(&merged.schema);
            continue;
          }

          let resolved = all_of_ref
            .resolve(&self.spec)
            .map_err(|e| anyhow::anyhow!("Schema resolution failed for inline allOf reference: {e}"))?;
          acc.merge_from(&resolved);
        }
        ObjectOrReference::Object(inline) => {
          let inner_merged = self.merge_inline(inline)?;
          acc.merge_from(&inner_merged);
        }
      }
    }

    acc.merge_from(schema);

    if acc.additional_properties.is_none() {
      acc.additional_properties.clone_from(&schema.additional_properties);
    }

    Ok(acc.into_schema(schema))
  }

  /// Recursively collects all named schema references from a schema.
  ///
  /// Traverses properties, composition keywords (`all_of`, `one_of`, `any_of`),
  /// array items, and inline objects to identify every schema that the
  /// given schema depends on.
  ///
  /// Union fingerprints are used to identify named union types that may
  /// be referenced via `one_of` or `any_of` composition.
  #[allow(clippy::self_only_used_in_recursion)]
  pub(crate) fn collect(&self, schema: &ObjectSchema, union_fingerprints: &UnionFingerprints) -> BTreeSet<String> {
    let mut refs = BTreeSet::new();

    for prop_schema in schema.properties.values() {
      if let Some(ref_name) = extract_schema_ref_name(prop_schema) {
        refs.insert(ref_name);
      }
      if let ObjectOrReference::Object(inline) = prop_schema {
        refs.extend(self.collect(inline, union_fingerprints));
      }
    }

    for schema_ref in schema.union_variants().chain(&schema.all_of) {
      if let Some(ref_name) = extract_schema_ref_name(schema_ref) {
        refs.insert(ref_name);
      }
      if let ObjectOrReference::Object(inline) = schema_ref {
        refs.extend(self.collect(inline, union_fingerprints));
      }
    }

    for variants in [&schema.one_of, &schema.any_of] {
      let fingerprint = extract_union_fingerprint(variants);
      if !fingerprint.is_empty()
        && let Some(name) = union_fingerprints.get(&fingerprint)
      {
        refs.insert(name.clone());
      }
    }

    if let Some(ref items_box) = schema.items
      && let Schema::Object(ref schema_ref) = **items_box
    {
      if let Some(ref_name) = extract_schema_ref_name(schema_ref) {
        refs.insert(ref_name);
      }
      if let ObjectOrReference::Object(inline) = &**schema_ref {
        refs.extend(self.collect(inline, union_fingerprints));
      }
    }

    refs
  }

  /// Collects named references from a schema reference.
  ///
  /// If the reference points to a named schema, returns that name.
  /// If it contains an inline object, recursively collects references
  /// from within that object.
  pub(crate) fn collect_ref(
    &self,
    schema_ref: &ObjectOrReference<ObjectSchema>,
    union_fingerprints: &UnionFingerprints,
  ) -> BTreeSet<String> {
    let mut refs = BTreeSet::new();
    if let Some(ref_name) = extract_schema_ref_name(schema_ref) {
      refs.insert(ref_name);
    }
    if let ObjectOrReference::Object(inline_schema) = schema_ref {
      refs.extend(self.collect(inline_schema, union_fingerprints));
    }
    refs
  }

  /// Detects cycles in the schema dependency graph.
  ///
  /// Uses Kosaraju's algorithm to find strongly connected components
  /// in the dependency graph. Any component with more than one node or
  /// a self-loop represents a cycle.
  ///
  /// Updates the internal `cyclic_schemas` set with all schemas
  /// participating in cycles.
  ///
  /// Returns a list of detected cycles, where each cycle is a list
  /// of schema names involved in that cycle.
  pub(crate) fn detect_cycles(&mut self) -> Vec<Vec<String>> {
    let mut graph = DiGraphMap::<&str, ()>::new();
    for (node, deps) in &self.dependencies {
      graph.add_node(node.as_str());
      for dep in deps {
        graph.add_edge(node.as_str(), dep.as_str(), ());
      }
    }

    let cycles: Vec<Vec<String>> = kosaraju_scc(&graph)
      .into_iter()
      .filter(|scc| scc.len() > 1 || graph.contains_edge(scc[0], scc[0]))
      .map(|scc| scc.into_iter().map(String::from).collect())
      .collect();

    self.cyclic_schemas.extend(cycles.iter().flatten().cloned());
    cycles
  }

  /// Determines which schemas are reachable from API operations.
  ///
  /// Performs a depth-first search starting from schemas referenced
  /// in operation parameters, request bodies, and responses. Returns
  /// the complete set of schemas that are transitively reachable.
  ///
  /// Unreachable schemas can be excluded from code generation to
  /// reduce output size.
  pub(crate) fn reachable(
    &self,
    operation_registry: &OperationRegistry,
    union_fingerprints: &UnionFingerprints,
  ) -> BTreeSet<String> {
    let initial_refs = operation_registry
      .operations()
      .flat_map(|entry| self.collect_refs_from_operation(&entry.operation, union_fingerprints))
      .collect::<BTreeSet<_>>();

    let graph = DiGraphMap::<&str, ()>::from_edges(
      self
        .dependencies
        .iter()
        .flat_map(|(node, deps)| deps.iter().map(move |dep| (node.as_str(), dep.as_str()))),
    );

    let mut expanded = initial_refs.clone();
    for start in &initial_refs {
      if graph.contains_node(start.as_str()) {
        let mut dfs = Dfs::new(&graph, start.as_str());
        while let Some(node) = dfs.next(&graph) {
          expanded.insert(node.to_string());
        }
      }
    }
    expanded
  }

  /// Computes final Rust names for all schemas and their variants.
  ///
  /// Delegates to [`TypeNameIndex`] to handle name collision resolution,
  /// case conversion, and variant naming for discriminated unions.
  pub(crate) fn scan_and_compute_names(&self) -> anyhow::Result<ScanResult> {
    let index = TypeNameIndex::new(&self.schemas, &self.spec);
    index.scan_and_compute_names()
  }

  /// Builds the complete dependency graph and merges inheritance hierarchies.
  ///
  /// Computes dependencies for all schemas, determines inheritance depths,
  /// and creates merged schemas by flattening `all_of` hierarchies.
  /// Also builds the discriminator parent mappings for polymorphic types.
  pub(crate) fn build_dependencies(&mut self, union_fingerprints: &UnionFingerprints) {
    for (name, schema) in &self.schemas {
      self
        .dependencies
        .insert(name.clone(), self.collect(schema, union_fingerprints));
    }

    self.compute_inheritance_depths();
    self.build_merged_schemas();
    self.build_discriminator_parents();
  }

  /// Computes inheritance depths for all schemas.
  ///
  /// Determines how deep each schema is in its `all_of` inheritance chain.
  /// Schemas with no parents have depth 0; children have depth equal to
  /// the maximum parent depth plus one. Results are memoized to avoid
  /// redundant computation in deep hierarchies.
  fn compute_inheritance_depths(&mut self) {
    fn compute_depth(registry: &mut SchemaRegistry, name: &str) -> usize {
      if let Some(&depth) = registry.inheritance_depths.get(name) {
        return depth;
      }

      let parent_names = registry
        .schemas
        .get(name)
        .map(|s| s.all_of.iter().filter_map(extract_schema_ref_name).collect::<Vec<_>>())
        .unwrap_or_default();

      let depth = if parent_names.is_empty() {
        0
      } else {
        parent_names
          .iter()
          .map(|p| compute_depth(registry, p))
          .max()
          .unwrap_or(0)
          + 1
      };

      registry.inheritance_depths.insert(name.to_string(), depth);
      depth
    }

    let names = self.schemas.keys().cloned().collect::<Vec<_>>();
    for name in names {
      compute_depth(self, &name);
    }
  }

  /// Creates merged schemas by flattening all inheritance hierarchies.
  ///
  /// Processes schemas in order of increasing inheritance depth, ensuring
  /// parent schemas are merged before their children. For each schema with
  /// `all_of` references, combines properties from all parents into a
  /// single flattened schema definition.
  fn build_merged_schemas(&mut self) {
    let mut sorted_names = self.schemas.keys().cloned().collect::<Vec<_>>();
    sorted_names.sort_by_key(|name| self.inheritance_depths.get(name).copied().unwrap_or(0));

    for name in sorted_names {
      let Some(schema) = self.schemas.get(&name).cloned() else {
        continue;
      };
      let merged = self.merge_schema(&schema);
      self.merged_schemas.insert(name, merged);
    }
  }

  /// Resolves a schema reference to its underlying schema definition.
  ///
  /// For references (`$ref`), looks up the merged schema first, falling
  /// back to the raw schema if no merged version exists. For inline objects,
  /// returns the object directly.
  fn resolve_schema_ref<'a>(&'a self, schema_ref: &'a ObjectOrReference<ObjectSchema>) -> Option<&'a ObjectSchema> {
    match schema_ref {
      ObjectOrReference::Ref { ref_path, .. } => {
        let name = parse_schema_ref_path(ref_path)?;
        self
          .merged_schemas
          .get(&name)
          .map(|m| &m.schema)
          .or_else(|| self.schemas.get(&name))
      }
      ObjectOrReference::Object(s) => Some(s),
    }
  }

  /// Searches for `additionalProperties` in a schema's inheritance chain.
  ///
  /// Traverses the `all_of` references and returns the first
  /// `additionalProperties` definition found in a parent schema.
  fn find_additional_properties(&self, schema: &ObjectSchema) -> Option<Schema> {
    schema
      .all_of
      .iter()
      .resolve_all(&self.spec)
      .find_map(|parent| parent.additional_properties.clone())
  }

  /// Builds a cache of discriminator mappings from all schemas.
  ///
  /// Scans every schema for discriminator definitions and builds a reverse
  /// lookup from child schema name to its discriminator property and value.
  /// This enables code generation for tagged unions with proper variant
  /// identification.
  ///
  /// Handles two cases:
  /// 1. Explicit mappings from the discriminator's `mapping` field
  /// 2. Implicit mappings inferred from `const` values on the discriminator
  ///    property in each `oneOf`/`anyOf` variant schema
  fn build_discriminator_cache(
    schemas: &BTreeMap<String, ObjectSchema>,
    stats: &mut GenerationStats,
  ) -> BTreeMap<String, DiscriminatorMapping> {
    let mut cache = BTreeMap::new();

    for (parent_name, schema) in schemas {
      let Some(d) = &schema.discriminator else {
        continue;
      };

      if let Some(mapping) = &d.mapping {
        for (val, ref_path) in mapping {
          if let Some(schema_name) = parse_schema_ref_path(ref_path) {
            cache.insert(
              schema_name,
              DiscriminatorMapping {
                field_name: d.property_name.clone(),
                field_value: val.clone(),
              },
            );
          }
        }
        continue;
      }

      Self::synthesize_implicit_mappings(parent_name, schema, d, schemas, stats, &mut cache);
    }

    cache
  }

  /// Synthesizes discriminator mappings from `const` values on variant schemas.
  ///
  /// When a discriminator has no explicit `mapping`, examines each `oneOf`/`anyOf`
  /// variant's discriminator property for a `const` value. All variants must have
  /// a unique string `const` value for synthesis to succeed. Records a warning
  /// and skips synthesis on any failure (missing const, non-string const, duplicates).
  fn synthesize_implicit_mappings(
    parent_name: &str,
    schema: &ObjectSchema,
    discriminator: &Discriminator,
    schemas: &BTreeMap<String, ObjectSchema>,
    stats: &mut GenerationStats,
    cache: &mut BTreeMap<String, DiscriminatorMapping>,
  ) {
    let mut staged = BTreeMap::new();
    let mut seen_values = BTreeSet::new();

    for variant_name in schema.union_variants().filter_map(extract_schema_ref_name) {
      let warn = |msg: String| GenerationWarning::DiscriminatorMappingFailed {
        schema_name: parent_name.to_owned(),
        message: msg,
      };

      let Some(variant_schema) = schemas.get(&variant_name) else {
        stats.record_warning(warn(format!(
          "cannot build implicit discriminator mapping: variant '{variant_name}' not found in schemas"
        )));
        return;
      };

      let Some(const_value) = Self::extract_const_discriminator_value(variant_schema, &discriminator.property_name)
      else {
        stats.record_warning(warn(format!(
          "cannot build implicit discriminator mapping: variant '{variant_name}' has no string const value for property '{}'",
          discriminator.property_name
        )));
        return;
      };

      if !seen_values.insert(const_value.clone()) {
        stats.record_warning(warn(format!(
          "cannot build implicit discriminator mapping: duplicate const value '{const_value}' on variant '{variant_name}'"
        )));
        return;
      }

      staged.insert(
        variant_name,
        DiscriminatorMapping {
          field_name: discriminator.property_name.clone(),
          field_value: const_value,
        },
      );
    }

    cache.extend(staged);
  }

  /// Extracts the string `const` value from a schema's discriminator property.
  ///
  /// Returns `None` if the property doesn't exist, has no `const_value`,
  /// or the `const_value` is not a string.
  fn extract_const_discriminator_value(schema: &ObjectSchema, property_name: &str) -> Option<String> {
    let prop_ref = schema.properties.get(property_name)?;
    let ObjectOrReference::Object(prop_schema) = prop_ref else {
      return None;
    };
    prop_schema.const_value.as_ref()?.as_str().map(String::from)
  }

  /// Builds mappings from child schemas to their discriminator parents.
  ///
  /// Identifies schemas that are variants of discriminated unions and
  /// records which parent schema serves as the polymorphic base.
  fn build_discriminator_parents(&mut self) {
    self.discriminator_parents = self
      .merged_schemas
      .iter()
      .filter_map(|(child_name, merged)| {
        merged
          .discriminator_parent
          .as_ref()
          .filter(|_| self.discriminator_cache.contains_key(child_name))
          .map(|parent_name| (child_name.clone(), parent_name.clone()))
      })
      .collect();
  }

  /// Merges a schema with all its `all_of` parents.
  ///
  /// Combines properties, required fields, discriminators, and other
  /// metadata from the schema and all schemas in its `all_of` chain.
  /// Also handles `any_of` and `one_of` composition for optional fields.
  fn merge_schema(&self, schema: &ObjectSchema) -> MergedSchema {
    if schema.all_of.is_empty() {
      return MergedSchema {
        schema: schema.clone(),
        discriminator_parent: None,
      };
    }

    let mut acc = MergeAccumulator::default();

    for all_of_ref in &schema.all_of {
      if let Some(parent_name) = extract_schema_ref_name(all_of_ref)
        && let Some(parent) = self.resolve_schema_ref(all_of_ref)
        && parent.is_discriminated_base_type()
      {
        acc.discriminator_parent = Some(parent_name.clone());
      }

      if let Some(parent) = self.resolve_schema_ref(all_of_ref) {
        acc.merge_from(parent);
      }
    }

    for schema_ref in schema.any_of.iter().chain(&schema.one_of) {
      if let Some(source) = self.resolve_schema_ref(schema_ref) {
        acc.merge_optional_from(source);
      }
    }

    acc.merge_from(schema);

    if acc.additional_properties.is_none() {
      acc.additional_properties = self.find_additional_properties(schema);
    }

    let discriminator_parent = acc.discriminator_parent.take();
    MergedSchema {
      schema: acc.into_schema(schema),
      discriminator_parent,
    }
  }

  /// Collects schema references from an operation.
  ///
  /// Scans parameters, request bodies, and responses to identify all
  /// schema references that an operation directly depends on.
  fn collect_refs_from_operation(
    &self,
    operation: &Operation,
    union_fingerprints: &UnionFingerprints,
  ) -> BTreeSet<String> {
    let mut refs = BTreeSet::new();

    for param in &operation.parameters {
      if let Ok(resolved_param) = param.resolve(&self.spec)
        && let Some(ref schema_ref) = resolved_param.schema
      {
        refs.extend(self.collect_ref(schema_ref, union_fingerprints));
      }
    }

    if let Some(ref request_body_ref) = operation.request_body
      && let Ok(request_body) = request_body_ref.resolve(&self.spec)
    {
      for media_type in request_body.content.values() {
        if let Some(ref schema_ref) = media_type.schema {
          refs.extend(self.collect_ref(schema_ref, union_fingerprints));
        }
      }
    }

    if let Some(ref responses) = operation.responses {
      for response_ref in responses.values() {
        if let Ok(response) = response_ref.resolve(&self.spec) {
          for media_type in response.content.values() {
            if let Some(ref schema_ref) = media_type.schema {
              refs.extend(self.collect_ref(schema_ref, union_fingerprints));
            }
          }
        }
      }
    }

    refs
  }
}

use std::{
  collections::{BTreeMap, BTreeSet},
  string::ToString,
};

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
  utils::SchemaExt,
};

const SCHEMA_REF_PREFIX: &str = "#/components/schemas/";

pub(crate) type UnionFingerprints = BTreeMap<BTreeSet<String>, String>;

#[derive(Debug, Clone)]
pub(crate) struct DiscriminatorMapping {
  pub field_name: String,
  pub field_value: String,
}

#[derive(Debug, Clone)]
pub(crate) struct MergedSchema {
  pub schema: ObjectSchema,
  pub discriminator_parent: Option<String>,
}

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

  fn merge_optional_from(&mut self, source: &ObjectSchema) {
    for (name, prop) in &source.properties {
      self.properties.entry(name.clone()).or_insert_with(|| prop.clone());
    }
  }

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

fn detect_cycles(dependencies: &BTreeMap<String, BTreeSet<String>>) -> Vec<Vec<String>> {
  let mut graph = DiGraphMap::<&str, ()>::new();
  for (node, deps) in dependencies {
    graph.add_node(node.as_str());
    for dep in deps {
      graph.add_edge(node.as_str(), dep.as_str(), ());
    }
  }

  kosaraju_scc(&graph)
    .into_iter()
    .filter(|scc| scc.len() > 1 || graph.contains_edge(scc[0], scc[0]))
    .map(|scc| scc.into_iter().map(String::from).collect())
    .collect()
}

fn collect_refs_from_operation(
  operation: &Operation,
  spec: &Spec,
  registry: &SchemaRegistry,
  refs: &mut BTreeSet<String>,
  union_fingerprints: &UnionFingerprints,
) {
  for param in &operation.parameters {
    if let Ok(resolved_param) = param.resolve(spec)
      && let Some(ref schema_ref) = resolved_param.schema
    {
      registry.collect_ref(schema_ref, refs, union_fingerprints);
    }
  }

  if let Some(ref request_body_ref) = operation.request_body
    && let Ok(request_body) = request_body_ref.resolve(spec)
  {
    for media_type in request_body.content.values() {
      if let Some(ref schema_ref) = media_type.schema {
        registry.collect_ref(schema_ref, refs, union_fingerprints);
      }
    }
  }

  if let Some(ref responses) = operation.responses {
    for response_ref in responses.values() {
      if let Ok(response) = response_ref.resolve(spec) {
        for media_type in response.content.values() {
          if let Some(ref schema_ref) = media_type.schema {
            registry.collect_ref(schema_ref, refs, union_fingerprints);
          }
        }
      }
    }
  }
}

fn expand_with_dependencies(
  initial_refs: &BTreeSet<String>,
  dependencies: &BTreeMap<String, BTreeSet<String>>,
) -> BTreeSet<String> {
  let graph = DiGraphMap::<&str, ()>::from_edges(
    dependencies
      .iter()
      .flat_map(|(node, deps)| deps.iter().map(move |dep| (node.as_str(), dep.as_str()))),
  );

  let mut expanded = initial_refs.clone();
  for start in initial_refs {
    if graph.contains_node(start.as_str()) {
      let mut dfs = Dfs::new(&graph, start.as_str());
      while let Some(node) = dfs.next(&graph) {
        expanded.insert(node.to_string());
      }
    }
  }
  expanded
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
  pub(crate) fn parse_ref(ref_string: &str) -> Option<String> {
    ref_string.strip_prefix(SCHEMA_REF_PREFIX).map(ToString::to_string)
  }

  pub(crate) fn parse_schema_ref(obj_ref: &ObjectOrReference<ObjectSchema>) -> Option<String> {
    match obj_ref {
      ObjectOrReference::Ref { ref_path, .. } => Self::parse_ref(ref_path),
      ObjectOrReference::Object(_) => None,
    }
  }

  pub(crate) fn collect(&self, schema: &ObjectSchema, union_fingerprints: &UnionFingerprints) -> BTreeSet<String> {
    let mut refs = BTreeSet::new();
    self.collect_all(schema, &mut refs, union_fingerprints);
    refs
  }

  pub(crate) fn collect_ref(
    &self,
    schema_ref: &ObjectOrReference<ObjectSchema>,
    refs: &mut BTreeSet<String>,
    union_fingerprints: &UnionFingerprints,
  ) {
    if let Some(ref_name) = Self::parse_schema_ref(schema_ref) {
      refs.insert(ref_name);
    }
    if let ObjectOrReference::Object(inline_schema) = schema_ref {
      refs.extend(self.collect(inline_schema, union_fingerprints));
    }
  }

  fn collect_all(&self, schema: &ObjectSchema, refs: &mut BTreeSet<String>, union_fingerprints: &UnionFingerprints) {
    let collect_from = |schema_ref: &ObjectOrReference<ObjectSchema>, refs: &mut BTreeSet<String>| {
      if let Some(ref_name) = Self::parse_schema_ref(schema_ref) {
        refs.insert(ref_name);
      }
      if let ObjectOrReference::Object(inline_schema) = schema_ref {
        refs.extend(self.collect(inline_schema, union_fingerprints));
      }
    };

    for prop_schema in schema.properties.values() {
      collect_from(prop_schema, refs);
    }

    for schema_ref in schema.one_of.iter().chain(&schema.any_of).chain(&schema.all_of) {
      collect_from(schema_ref, refs);
    }

    for variants in [&schema.one_of, &schema.any_of] {
      let fingerprint: BTreeSet<String> = variants.iter().filter_map(Self::parse_schema_ref).collect();
      if !fingerprint.is_empty()
        && let Some(name) = union_fingerprints.get(&fingerprint)
      {
        refs.insert(name.clone());
      }
    }

    if let Some(ref items_box) = schema.items
      && let Schema::Object(ref schema_ref) = **items_box
    {
      collect_from(schema_ref, refs);
    }
  }

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
      discriminator_cache: Self::build_discriminator_cache(&schemas),
      inheritance_depths: BTreeMap::new(),
      spec: spec.clone(),
    }
  }

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

  fn build_discriminator_cache(schemas: &BTreeMap<String, ObjectSchema>) -> BTreeMap<String, DiscriminatorMapping> {
    let mut cache = BTreeMap::new();

    for candidate_schema in schemas.values() {
      if let Some(d) = &candidate_schema.discriminator
        && let Some(mapping) = &d.mapping
      {
        for (val, ref_path) in mapping {
          if let Some(schema_name) = Self::parse_ref(ref_path) {
            cache.insert(
              schema_name,
              DiscriminatorMapping {
                field_name: d.property_name.clone(),
                field_value: val.clone(),
              },
            );
          }
        }
      }
    }

    cache
  }

  pub(crate) fn get(&self, name: &str) -> Option<&ObjectSchema> {
    self.schemas.get(name)
  }

  pub(crate) fn keys(&self) -> Vec<&String> {
    self.schemas.keys().collect()
  }

  pub(crate) fn schemas(&self) -> &BTreeMap<String, ObjectSchema> {
    &self.schemas
  }

  pub(crate) fn spec(&self) -> &Spec {
    &self.spec
  }

  pub(crate) fn build_dependencies(&mut self, union_fingerprints: &UnionFingerprints) {
    for schema_name in self.schemas.keys() {
      let deps = self
        .schemas
        .get(schema_name)
        .map(|s| self.collect(s, union_fingerprints))
        .unwrap_or_default();

      self.dependencies.insert(schema_name.clone(), deps);
    }

    self.compute_all_inheritance_depths();
    self.build_merged_schemas();
    self.build_discriminator_parents();
  }

  fn compute_all_inheritance_depths(&mut self) {
    let schema_names = self.schemas.keys().cloned().collect::<Vec<_>>();
    for name in schema_names {
      self.compute_depth_recursive(&name);
    }
  }

  fn compute_depth_recursive(&mut self, schema_name: &str) -> usize {
    if let Some(&depth) = self.inheritance_depths.get(schema_name) {
      return depth;
    }

    let parent_names = self
      .schemas
      .get(schema_name)
      .map(|schema| {
        schema
          .all_of
          .iter()
          .filter_map(Self::parse_schema_ref)
          .collect::<Vec<_>>()
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
    let mut sorted_names = self.schemas.keys().cloned().collect::<Vec<_>>();
    sorted_names.sort_by_key(|name| self.depth(name));

    for schema_name in sorted_names {
      let Some(schema) = self.schemas.get(&schema_name).cloned() else {
        continue;
      };

      let merged_schema = self.merge_schema(&schema);
      self.merged_schemas.insert(schema_name, merged_schema);
    }
  }

  fn merge_schema(&self, schema: &ObjectSchema) -> MergedSchema {
    if schema.all_of.is_empty() {
      return MergedSchema {
        schema: schema.clone(),
        discriminator_parent: None,
      };
    }

    let mut acc = MergeAccumulator::default();
    self.process_all_of(schema, &mut acc);
    self.process_optional_combinators(schema, &mut acc);
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

  fn process_all_of(&self, schema: &ObjectSchema, acc: &mut MergeAccumulator) {
    for all_of_ref in &schema.all_of {
      if let ObjectOrReference::Ref { ref_path, .. } = all_of_ref
        && let Some(parent_name) = Self::parse_ref(ref_path)
        && let Some(parent) = self.resolve_schema_ref(all_of_ref)
        && parent.discriminator.is_some()
        && parent.is_discriminated_base_type()
      {
        acc.discriminator_parent = Some(parent_name.clone());
      }

      if let Some(parent) = self.resolve_schema_ref(all_of_ref) {
        acc.merge_from(parent);
      }
    }
  }

  fn process_optional_combinators(&self, schema: &ObjectSchema, acc: &mut MergeAccumulator) {
    for schema_ref in schema.any_of.iter().chain(&schema.one_of) {
      if let Some(source) = self.resolve_schema_ref(schema_ref) {
        acc.merge_optional_from(source);
      }
    }
  }

  fn resolve_schema_ref<'a>(&'a self, schema_ref: &'a ObjectOrReference<ObjectSchema>) -> Option<&'a ObjectSchema> {
    match schema_ref {
      ObjectOrReference::Ref { ref_path, .. } => {
        let name = Self::parse_ref(ref_path)?;
        self
          .merged_schemas
          .get(&name)
          .map(|m| &m.schema)
          .or_else(|| self.schemas.get(&name))
      }
      ObjectOrReference::Object(s) => Some(s),
    }
  }

  fn find_additional_properties(&self, schema: &ObjectSchema) -> Option<Schema> {
    schema
      .all_of
      .iter()
      .filter_map(|r| r.resolve(&self.spec).ok())
      .find_map(|parent| parent.additional_properties.clone())
  }

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

  pub(crate) fn merged(&self, name: &str) -> Option<&MergedSchema> {
    self.merged_schemas.get(name)
  }

  pub(crate) fn resolved(&self, name: &str) -> Option<&ObjectSchema> {
    self.merged(name).map(|m| &m.schema).or_else(|| self.schemas.get(name))
  }

  pub(crate) fn parent(&self, name: &str) -> Option<&str> {
    self.discriminator_parents.get(name).map(String::as_str)
  }

  pub(crate) fn merge_all_of(&self, schema: &ObjectSchema) -> ObjectSchema {
    self.merge_schema(schema).schema
  }

  pub(crate) fn merge_inline(&self, schema: &ObjectSchema) -> anyhow::Result<ObjectSchema> {
    if schema.all_of.is_empty() {
      return Ok(schema.clone());
    }

    let mut acc = MergeAccumulator::default();

    for all_of_ref in &schema.all_of {
      match all_of_ref {
        ObjectOrReference::Ref { ref_path, .. } => {
          if let Some(name) = Self::parse_ref(ref_path)
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

  pub(crate) fn detect_cycles(&mut self) -> Vec<Vec<String>> {
    let cycles = detect_cycles(&self.dependencies);
    self.cyclic_schemas.extend(cycles.iter().flatten().cloned());
    cycles
  }

  pub(crate) fn is_cyclic(&self, schema_name: &str) -> bool {
    self.cyclic_schemas.contains(schema_name)
  }

  pub(crate) fn depth(&self, schema_name: &str) -> usize {
    self.inheritance_depths.get(schema_name).copied().unwrap_or(0)
  }

  pub(crate) fn mapping(&self, schema_name: &str) -> Option<&DiscriminatorMapping> {
    self.discriminator_cache.get(schema_name)
  }

  pub(crate) fn reachable(
    &self,
    operation_registry: &OperationRegistry,
    union_fingerprints: &UnionFingerprints,
  ) -> BTreeSet<String> {
    let mut refs = BTreeSet::new();

    for entry in operation_registry.operations() {
      collect_refs_from_operation(&entry.operation, &self.spec, self, &mut refs, union_fingerprints);
    }

    expand_with_dependencies(&refs, &self.dependencies)
  }

  pub(crate) fn scan_and_compute_names(&self) -> anyhow::Result<ScanResult> {
    let index = TypeNameIndex::new(&self.schemas, &self.spec);
    index.scan_and_compute_names()
  }
}

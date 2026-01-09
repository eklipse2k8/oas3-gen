use std::{
  collections::{BTreeMap, BTreeSet, HashMap, HashSet},
  string::ToString,
};

use oas3::{
  Spec,
  spec::{Discriminator, ObjectOrReference, ObjectSchema, Operation, Schema, SchemaTypeSet},
};

use super::orchestrator::GenerationWarning;
use crate::{
  generator::{
    naming::{
      identifiers::to_rust_type_name,
      name_index::{ScanResult, TypeNameIndex},
    },
    operation_registry::OperationRegistry,
  },
  utils::SchemaExt,
};

const SCHEMA_REF_PREFIX: &str = "#/components/schemas/";

type UnionFingerprints = HashMap<BTreeSet<String>, String>;

#[derive(Debug, Clone)]
pub(crate) struct DiscriminatorMapping {
  pub field_name: String,
  pub field_value: String,
}

impl DiscriminatorMapping {
  pub fn as_tuple(&self) -> (String, String) {
    (self.field_name.clone(), self.field_value.clone())
  }
}

#[derive(Debug, Clone)]
pub(crate) struct ParentInfo {
  pub parent_name: String,
}

#[derive(Debug, Clone)]
pub(crate) struct MergedSchema {
  pub schema: ObjectSchema,
  pub discriminator_parent: Option<String>,
}

pub(crate) struct ParseResult {
  pub registry: SchemaRegistry,
  pub warnings: Vec<GenerationWarning>,
}

#[derive(Debug)]
pub(crate) struct RefCollector<'a> {
  fingerprints: Option<&'a UnionFingerprints>,
}

impl<'a> RefCollector<'a> {
  pub(crate) fn new(fingerprints: Option<&'a UnionFingerprints>) -> Self {
    Self { fingerprints }
  }

  pub(crate) fn parse_ref(ref_string: &str) -> Option<String> {
    ref_string.strip_prefix(SCHEMA_REF_PREFIX).map(ToString::to_string)
  }

  pub(crate) fn parse_schema_ref(obj_ref: &ObjectOrReference<ObjectSchema>) -> Option<String> {
    match obj_ref {
      ObjectOrReference::Ref { ref_path, .. } => Self::parse_ref(ref_path),
      ObjectOrReference::Object(_) => None,
    }
  }

  pub(crate) fn collect(&self, schema: &ObjectSchema) -> BTreeSet<String> {
    let mut refs = BTreeSet::new();
    self.collect_all(schema, &mut refs);
    refs
  }

  fn collect_all(&self, schema: &ObjectSchema, refs: &mut BTreeSet<String>) {
    self.collect_from_properties(schema, refs);
    self.collect_from_combinators(schema, refs);
    self.collect_from_items(schema, refs);
  }

  pub(crate) fn collect_from(&self, schema_ref: &ObjectOrReference<ObjectSchema>, refs: &mut BTreeSet<String>) {
    if let Some(ref_name) = Self::parse_schema_ref(schema_ref) {
      refs.insert(ref_name);
    }

    if let ObjectOrReference::Object(inline_schema) = schema_ref {
      let inline_refs = self.collect(inline_schema);
      refs.extend(inline_refs);
    }
  }

  fn collect_from_properties(&self, schema: &ObjectSchema, refs: &mut BTreeSet<String>) {
    for prop_schema in schema.properties.values() {
      self.collect_from(prop_schema, refs);
    }
  }

  fn collect_from_combinators(&self, schema: &ObjectSchema, refs: &mut BTreeSet<String>) {
    for schema_ref in schema.one_of.iter().chain(&schema.any_of).chain(&schema.all_of) {
      self.collect_from(schema_ref, refs);
    }

    if let Some(map) = self.fingerprints {
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
      let fp = Self::fingerprint(variants);
      if let Some(name) = fingerprints.get(&fp) {
        refs.insert(name.clone());
      }
    }
  }

  fn collect_from_items(&self, schema: &ObjectSchema, refs: &mut BTreeSet<String>) {
    if let Some(ref items_box) = schema.items
      && let Schema::Object(ref schema_ref) = **items_box
    {
      self.collect_from(schema_ref, refs);
    }
  }

  pub(crate) fn fingerprint(variants: &[ObjectOrReference<ObjectSchema>]) -> BTreeSet<String> {
    variants.iter().filter_map(Self::parse_schema_ref).collect()
  }
}

struct SchemaMerger<'a> {
  schemas: &'a BTreeMap<String, ObjectSchema>,
  merged_schemas: &'a BTreeMap<String, MergedSchema>,
  spec: &'a Spec,
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

impl<'a> SchemaMerger<'a> {
  fn new(
    schemas: &'a BTreeMap<String, ObjectSchema>,
    merged_schemas: &'a BTreeMap<String, MergedSchema>,
    spec: &'a Spec,
  ) -> Self {
    Self {
      schemas,
      merged_schemas,
      spec,
    }
  }

  fn merge(&self, schema: &ObjectSchema) -> MergedSchema {
    if schema.all_of.is_empty() {
      return MergedSchema {
        schema: schema.clone(),
        discriminator_parent: None,
      };
    }

    let mut acc = MergeAccumulator::default();
    self.process_all_of(schema, &mut acc);
    self.process_any_of(schema, &mut acc);
    self.process_one_of(schema, &mut acc);
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

  fn merge_inline(&self, schema: &ObjectSchema) -> anyhow::Result<ObjectSchema> {
    if schema.all_of.is_empty() {
      return Ok(schema.clone());
    }

    let mut acc = MergeAccumulator::default();

    for all_of_ref in &schema.all_of {
      match all_of_ref {
        ObjectOrReference::Ref { ref_path, .. } => {
          if let Some(name) = RefCollector::parse_ref(ref_path)
            && let Some(merged) = self.merged_schemas.get(&name)
          {
            acc.merge_from(&merged.schema);
            continue;
          }

          let resolved = all_of_ref
            .resolve(self.spec)
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

  fn process_all_of(&self, schema: &ObjectSchema, acc: &mut MergeAccumulator) {
    for all_of_ref in &schema.all_of {
      match all_of_ref {
        ObjectOrReference::Ref { ref_path, .. } => {
          if let Some(parent_name) = RefCollector::parse_ref(ref_path) {
            let parent_schema = self
              .merged_schemas
              .get(&parent_name)
              .map(|m| &m.schema)
              .or_else(|| self.schemas.get(&parent_name));

            if let Some(parent) = parent_schema {
              if parent.discriminator.is_some() && parent.is_discriminated_base_type() {
                acc.discriminator_parent = Some(parent_name.clone());
              }
              acc.merge_from(parent);
            }
          }
        }
        ObjectOrReference::Object(inline_schema) => {
          acc.merge_from(inline_schema);
        }
      }
    }
  }

  fn process_any_of(&self, schema: &ObjectSchema, acc: &mut MergeAccumulator) {
    for any_of_ref in &schema.any_of {
      self.merge_optional(any_of_ref, acc);
    }
  }

  fn process_one_of(&self, schema: &ObjectSchema, acc: &mut MergeAccumulator) {
    for one_of_ref in &schema.one_of {
      self.merge_optional(one_of_ref, acc);
    }
  }

  fn merge_optional(&self, schema_ref: &ObjectOrReference<ObjectSchema>, acc: &mut MergeAccumulator) {
    let schema = match schema_ref {
      ObjectOrReference::Ref { ref_path, .. } => {
        if let Some(name) = RefCollector::parse_ref(ref_path) {
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
      acc.merge_optional_from(source);
    }
  }

  fn find_additional_properties(&self, schema: &ObjectSchema) -> Option<Schema> {
    for all_of_ref in &schema.all_of {
      if let Ok(parent) = all_of_ref.resolve(self.spec)
        && parent.additional_properties.is_some()
      {
        return parent.additional_properties.clone();
      }
    }
    None
  }
}

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
      path: vec![],
      cycles: vec![],
    }
  }

  fn detect(mut self) -> Vec<Vec<String>> {
    let nodes: Vec<String> = self.dependencies.keys().cloned().collect();
    for node in nodes {
      if !self.visited.contains(&node) {
        self.visit(&node);
      }
    }
    self.cycles
  }

  fn visit(&mut self, node: &str) {
    self.visited.insert(node.to_string());
    self.recursion_stack.insert(node.to_string());
    self.path.push(node.to_string());

    if let Some(deps) = self.dependencies.get(node) {
      for dep in deps.clone() {
        if !self.visited.contains(&dep) {
          self.visit(&dep);
        } else if self.recursion_stack.contains(&dep)
          && let Some(start_pos) = self.path.iter().position(|n| n == &dep)
        {
          let cycle: Vec<String> = self.path[start_pos..].to_vec();
          self.cycles.push(cycle);
        }
      }
    }

    self.path.pop();
    self.recursion_stack.remove(node);
  }
}

struct ReachabilityAnalyzer<'a> {
  spec: &'a Spec,
  fingerprints: &'a UnionFingerprints,
  dependencies: &'a BTreeMap<String, BTreeSet<String>>,
}

impl<'a> ReachabilityAnalyzer<'a> {
  fn new(
    spec: &'a Spec,
    fingerprints: &'a UnionFingerprints,
    dependencies: &'a BTreeMap<String, BTreeSet<String>>,
  ) -> Self {
    Self {
      spec,
      fingerprints,
      dependencies,
    }
  }

  fn compute_reachable(&self, operation_registry: &OperationRegistry) -> BTreeSet<String> {
    let mut reachable = BTreeSet::new();
    let collector = RefCollector::new(Some(self.fingerprints));

    for entry in operation_registry.operations() {
      self.collect_from_operation(&entry.operation, &collector, &mut reachable);
    }

    self.expand_with_dependencies(&reachable)
  }

  fn collect_from_operation(&self, operation: &Operation, collector: &RefCollector, refs: &mut BTreeSet<String>) {
    for param in &operation.parameters {
      if let Ok(resolved_param) = param.resolve(self.spec)
        && let Some(ref schema_ref) = resolved_param.schema
      {
        collector.collect_from(schema_ref, refs);
      }
    }

    if let Some(ref request_body_ref) = operation.request_body
      && let Ok(request_body) = request_body_ref.resolve(self.spec)
    {
      for media_type in request_body.content.values() {
        if let Some(ref schema_ref) = media_type.schema {
          collector.collect_from(schema_ref, refs);
        }
      }
    }

    if let Some(ref responses) = operation.responses {
      for response_ref in responses.values() {
        if let Ok(response) = response_ref.resolve(self.spec) {
          for media_type in response.content.values() {
            if let Some(ref schema_ref) = media_type.schema {
              collector.collect_from(schema_ref, refs);
            }
          }
        }
      }
    }
  }

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

#[derive(Debug)]
pub(crate) struct SchemaRegistry {
  schemas: BTreeMap<String, ObjectSchema>,
  merged_schemas: BTreeMap<String, MergedSchema>,
  discriminator_parents: BTreeMap<String, ParentInfo>,
  dependencies: BTreeMap<String, BTreeSet<String>>,
  cyclic_schemas: BTreeSet<String>,
  discriminator_cache: BTreeMap<String, DiscriminatorMapping>,
  inheritance_depths: HashMap<String, usize>,
  spec: Spec,
  union_fingerprints: UnionFingerprints,
  cached_schema_names: HashSet<String>,
}

impl SchemaRegistry {
  pub(crate) fn from_spec(spec: Spec) -> ParseResult {
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

    ParseResult {
      registry: Self {
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
    }
  }

  fn build_discriminator_cache(schemas: &BTreeMap<String, ObjectSchema>) -> BTreeMap<String, DiscriminatorMapping> {
    let mut cache = BTreeMap::new();

    for candidate_schema in schemas.values() {
      if let Some(d) = &candidate_schema.discriminator
        && let Some(mapping) = &d.mapping
      {
        for (val, ref_path) in mapping {
          if let Some(schema_name) = RefCollector::parse_ref(ref_path) {
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

  fn build_union_fingerprints(schemas: &BTreeMap<String, ObjectSchema>) -> UnionFingerprints {
    let mut map = UnionFingerprints::new();
    for (name, schema) in schemas {
      let fp_one = RefCollector::fingerprint(&schema.one_of);
      if fp_one.len() >= 2 {
        map.entry(fp_one).or_insert(name.clone());
      }

      let fp_any = RefCollector::fingerprint(&schema.any_of);
      if fp_any.len() >= 2 {
        map.entry(fp_any).or_insert(name.clone());
      }
    }
    map
  }

  pub(crate) fn get(&self, name: &str) -> Option<&ObjectSchema> {
    self.schemas.get(name)
  }

  pub(crate) fn contains(&self, name: &str) -> bool {
    self.cached_schema_names.contains(name)
  }

  pub(crate) fn keys(&self) -> Vec<&String> {
    self.schemas.keys().collect()
  }

  pub(crate) fn spec(&self) -> &Spec {
    &self.spec
  }

  pub(crate) fn parse_ref(ref_string: &str) -> Option<String> {
    RefCollector::parse_ref(ref_string)
  }

  pub(crate) fn build_dependencies(&mut self) {
    let collector = RefCollector::new(Some(&self.union_fingerprints));

    for schema_name in self.schemas.keys() {
      let deps = self
        .schemas
        .get(schema_name)
        .map(|s| collector.collect(s))
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
          .filter_map(RefCollector::parse_schema_ref)
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
    sorted_names.sort_by_key(|name| self.depth(name));

    for schema_name in sorted_names {
      let Some(schema) = self.schemas.get(&schema_name).cloned() else {
        continue;
      };

      let merger = SchemaMerger::new(&self.schemas, &self.merged_schemas, &self.spec);
      let merged_schema = merger.merge(&schema);
      self.merged_schemas.insert(schema_name, merged_schema);
    }
  }

  fn build_discriminator_parents(&mut self) {
    let mut map = BTreeMap::new();

    for (child_name, merged) in &self.merged_schemas {
      if let Some(parent_name) = &merged.discriminator_parent
        && self.discriminator_cache.contains_key(child_name)
      {
        map.insert(
          child_name.clone(),
          ParentInfo {
            parent_name: parent_name.clone(),
          },
        );
      }
    }

    self.discriminator_parents = map;
  }

  pub(crate) fn merged(&self, name: &str) -> Option<&MergedSchema> {
    self.merged_schemas.get(name)
  }

  pub(crate) fn resolved(&self, name: &str) -> Option<&ObjectSchema> {
    self
      .merged_schemas
      .get(name)
      .map(|m| &m.schema)
      .or_else(|| self.schemas.get(name))
  }

  pub(crate) fn merge_all_of(&self, schema: &ObjectSchema) -> ObjectSchema {
    if schema.all_of.is_empty() {
      return schema.clone();
    }
    let merger = SchemaMerger::new(&self.schemas, &self.merged_schemas, &self.spec);
    let result = merger.merge(schema);
    result.schema
  }

  pub(crate) fn parent(&self, name: &str) -> Option<&ParentInfo> {
    self.discriminator_parents.get(name)
  }

  pub(crate) fn merge_inline(&self, schema: &ObjectSchema) -> anyhow::Result<ObjectSchema> {
    let merger = SchemaMerger::new(&self.schemas, &self.merged_schemas, &self.spec);
    merger.merge_inline(schema)
  }

  pub(crate) fn detect_cycles(&mut self) -> Vec<Vec<String>> {
    let detector = CycleDetector::new(&self.dependencies);
    let cycles = detector.detect();

    for cycle in &cycles {
      for schema_name in cycle {
        self.cyclic_schemas.insert(schema_name.clone());
      }
    }

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

  pub(crate) fn find_union(&self, fingerprint: &BTreeSet<String>) -> Option<&String> {
    self.union_fingerprints.get(fingerprint)
  }

  pub(crate) fn reachable(&self, operation_registry: &OperationRegistry) -> BTreeSet<String> {
    let analyzer = ReachabilityAnalyzer::new(&self.spec, &self.union_fingerprints, &self.dependencies);
    analyzer.compute_reachable(operation_registry)
  }

  pub(crate) fn scan_and_compute_names(&self) -> anyhow::Result<ScanResult> {
    let index = TypeNameIndex::new(&self.schemas);
    index.scan_and_compute_names()
  }
}

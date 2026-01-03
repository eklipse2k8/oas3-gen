use std::collections::{BTreeMap, BTreeSet};

use oas3::spec::ObjectSchema;

use super::{hashing::CanonicalSchema, struct_summaries::StructSummary};
use crate::generator::{
  ast::{EnumToken, RustType, StructToken},
  naming::{
    identifiers::{ensure_unique, to_rust_type_name},
    inference::InferenceExt,
  },
};

#[derive(Default, Debug, Clone)]
struct TypeIdentityCache {
  schema_to_type: BTreeMap<CanonicalSchema, String>,
  enum_to_type: BTreeMap<Vec<String>, String>,
  union_refs_to_type: BTreeMap<(BTreeSet<String>, Option<String>), String>,
  used_names: BTreeSet<String>,
  precomputed_names: BTreeMap<CanonicalSchema, String>,
  precomputed_enum_names: BTreeMap<Vec<String>, String>,
}

impl TypeIdentityCache {
  fn set_precomputed_names(
    &mut self,
    names: BTreeMap<CanonicalSchema, String>,
    enum_names: BTreeMap<Vec<String>, String>,
  ) {
    self.precomputed_names = names;
    self.precomputed_enum_names = enum_names;
  }

  fn schema_type(&self, canonical: &CanonicalSchema) -> Option<String> {
    self.schema_to_type.get(canonical).cloned()
  }

  fn enum_type(&self, values: &[String]) -> Option<String> {
    self
      .enum_to_type
      .get(values)
      .or_else(|| self.precomputed_enum_names.get(values))
      .cloned()
  }

  fn generated_enum_type(&self, values: &[String]) -> Option<String> {
    self.enum_to_type.get(values).cloned()
  }

  fn enum_registered(&self, values: &[String]) -> bool {
    self.enum_to_type.contains_key(values)
  }

  fn register_enum(&mut self, values: Vec<String>, name: String) {
    self.enum_to_type.insert(values, name);
  }

  fn union_type(&self, refs: &BTreeSet<String>, discriminator: Option<&str>) -> Option<String> {
    self
      .union_refs_to_type
      .get(&(refs.clone(), discriminator.map(String::from)))
      .cloned()
  }

  fn register_union(&mut self, refs: BTreeSet<String>, discriminator: Option<String>, name: String) {
    self.union_refs_to_type.insert((refs, discriminator), name);
  }

  fn mark_used(&mut self, name: String) {
    self.used_names.insert(name);
  }

  fn is_used(&self, name: &str) -> bool {
    self.used_names.contains(name)
  }

  fn record_schema_name(&mut self, canonical: CanonicalSchema, name: String) {
    self.schema_to_type.insert(canonical, name);
  }

  fn has_schema_name(&self, canonical: &CanonicalSchema, name: &str) -> bool {
    self.schema_to_type.get(canonical).is_some_and(|n| n == name)
  }

  fn preferred_name(&self, canonical: &CanonicalSchema, base_name: &str) -> String {
    self
      .precomputed_names
      .get(canonical)
      .cloned()
      .unwrap_or_else(|| self.make_unique_name(base_name))
  }

  fn make_unique_name(&self, base: &str) -> String {
    let rust_name = to_rust_type_name(base);
    ensure_unique(&rust_name, &self.used_names)
  }
}

#[derive(Default, Debug, Clone)]
pub(crate) struct GeneratedTypeStore {
  pub(crate) generated_types: Vec<RustType>,
}

impl GeneratedTypeStore {
  fn push(&mut self, mut nested_types: Vec<RustType>, type_def: RustType) {
    self.generated_types.append(&mut nested_types);
    self.generated_types.push(type_def);
  }

  fn into_inner(self) -> Vec<RustType> {
    self.generated_types
  }
}

#[derive(Default, Debug, Clone)]
struct StructSummaryIndex {
  summaries: BTreeMap<String, StructSummary>,
}

impl StructSummaryIndex {
  fn register(&mut self, type_name: &str, summary: StructSummary) {
    self.summaries.insert(type_name.to_string(), summary);
  }

  fn get(&self, type_name: &str) -> Option<&StructSummary> {
    self.summaries.get(type_name)
  }
}

/// Cache for sharing generated Rust types across the schema graph.
///
/// Prevents duplication of structs and enums by hashing schemas and storing mapping.
#[derive(Debug, Clone)]
pub(crate) struct SharedSchemaCache {
  identity: TypeIdentityCache,
  pub(crate) generated: GeneratedTypeStore,
  summaries: StructSummaryIndex,
}

impl SharedSchemaCache {
  /// Creates a new empty cache.
  pub(crate) fn new() -> Self {
    Self {
      identity: TypeIdentityCache::default(),
      generated: GeneratedTypeStore::default(),
      summaries: StructSummaryIndex::default(),
    }
  }

  /// Sets precomputed names for schemas, useful for deterministic naming or overrides.
  pub(crate) fn set_precomputed_names(
    &mut self,
    names: BTreeMap<CanonicalSchema, String>,
    enum_names: BTreeMap<Vec<String>, String>,
  ) {
    self.identity.set_precomputed_names(names, enum_names);
  }

  /// Retrieves a cached type name for a schema, if it exists.
  pub(crate) fn get_type_name(&self, schema: &ObjectSchema) -> anyhow::Result<Option<String>> {
    let canonical = CanonicalSchema::from_schema(schema)?;
    Ok(self.identity.schema_type(&canonical))
  }

  /// Retrieves a cached name for an enum based on its values.
  pub(crate) fn get_enum_name(&self, values: &[String]) -> Option<String> {
    self.identity.enum_type(values)
  }

  /// Looks up a cached enum name for a schema, if it has enum values and isn't a relaxed pattern.
  pub(crate) fn lookup_enum_name(&self, schema: &ObjectSchema) -> Option<String> {
    if schema.is_relaxed_enum_pattern() {
      return None;
    }
    let values = schema.extract_enum_values()?;
    self.get_enum_name(&values)
  }

  /// Checks if an enum with the given values has already been generated.
  pub(crate) fn is_enum_generated(&self, values: &[String]) -> bool {
    self.identity.enum_registered(values)
  }

  /// Registers an enum name for a set of values.
  pub(crate) fn register_enum(&mut self, values: Vec<String>, name: String) {
    self.identity.register_enum(values, name);
  }

  /// Retrieves a cached name for a union enum based on its variant refs and discriminator.
  pub(crate) fn get_union_name(&self, refs: &BTreeSet<String>, discriminator: Option<&str>) -> Option<String> {
    self.identity.union_type(refs, discriminator)
  }

  /// Registers a union enum name for a set of variant refs and discriminator.
  pub(crate) fn register_union(&mut self, refs: BTreeSet<String>, discriminator: Option<String>, name: String) {
    self.identity.register_union(refs, discriminator, name);
  }

  /// Marks a type name as used to prevent collisions.
  pub(crate) fn mark_name_used(&mut self, name: String) {
    self.identity.mark_used(name);
  }

  /// Pre-registers a top-level schema so inline schemas with identical structure reuse it.
  pub(crate) fn register_top_level_schema(&mut self, schema: &ObjectSchema, name: &str) -> anyhow::Result<()> {
    let canonical = CanonicalSchema::from_schema(schema)?;
    let rust_name = to_rust_type_name(name);
    self.identity.record_schema_name(canonical, rust_name.clone());
    self.identity.mark_used(rust_name);
    Ok(())
  }

  /// Gets a preferred name for a schema, using precomputed names or generating a unique one.
  pub(crate) fn get_preferred_name(&self, schema: &ObjectSchema, base_name: &str) -> anyhow::Result<String> {
    let canonical = CanonicalSchema::from_schema(schema)?;
    Ok(self.identity.preferred_name(&canonical, base_name))
  }

  /// Registers a new type definition in the cache.
  ///
  /// Handles name collisions, enum reuse, and stores the generated Rust type.
  pub(crate) fn register_type(
    &mut self,
    schema: &ObjectSchema,
    base_name: &str,
    nested_types: Vec<RustType>,
    type_def: RustType,
  ) -> anyhow::Result<String> {
    let canonical = CanonicalSchema::from_schema(schema)?;

    if !schema.is_relaxed_enum_pattern()
      && let Some(values) = schema.extract_enum_values()
      && let Some(existing_name) = self.identity.generated_enum_type(&values)
    {
      self.identity.record_schema_name(canonical, existing_name.clone());
      return Ok(existing_name);
    }

    let mut name = base_name.to_string();

    if self.identity.is_used(&name) {
      if let Some(existing_name) = self.identity.schema_type(&canonical) {
        return Ok(existing_name);
      }
      name = self.identity.make_unique_name(&name);
    }

    self.identity.mark_used(name.clone());
    self.identity.record_schema_name(canonical, name.clone());

    if let Some(values) = schema.extract_enum_values() {
      self.identity.register_enum(values, name.clone());
    }

    let mut final_type_def = type_def;
    match &mut final_type_def {
      RustType::Struct(s) => s.name = StructToken::from(name.clone()),
      RustType::Enum(e) => e.name = EnumToken::new(&name),
      _ => {}
    }

    self.generated.push(nested_types, final_type_def);

    Ok(name)
  }

  /// Generates a unique name based on a base name, ensuring no collisions with used names.
  pub(crate) fn make_unique_name(&self, base: &str) -> String {
    self.identity.make_unique_name(base)
  }

  /// Checks if a name is already used by a different schema.
  ///
  /// Returns true if the name is in use AND the schema hash doesn't match any existing entry.
  pub(crate) fn name_conflicts_with_different_schema(&self, name: &str, schema: &ObjectSchema) -> anyhow::Result<bool> {
    if !self.identity.is_used(name) {
      return Ok(false);
    }
    let canonical = CanonicalSchema::from_schema(schema)?;
    Ok(!self.identity.has_schema_name(&canonical, name))
  }

  /// Consumes the cache and returns all generated Rust types.
  pub(crate) fn into_types(self) -> Vec<RustType> {
    self.generated.into_inner()
  }

  /// Stores a struct summary for enum helper generation.
  pub(crate) fn register_struct_summary(&mut self, type_name: &str, summary: StructSummary) {
    self.summaries.register(type_name, summary);
  }

  /// Retrieves a cached struct summary by type name.
  pub(crate) fn get_struct_summary(&self, type_name: &str) -> Option<&StructSummary> {
    self.summaries.get(type_name)
  }
}

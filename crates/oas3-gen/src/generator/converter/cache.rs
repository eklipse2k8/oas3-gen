use std::collections::{BTreeMap, BTreeSet};

use oas3::spec::ObjectSchema;

use super::hashing;
use crate::generator::{
  ast::{EnumToken, FieldNameToken, RustType, StructDef, StructToken, TypeRef},
  naming::{
    identifiers::{ensure_unique, to_rust_type_name},
    inference::{extract_enum_values, is_relaxed_enum_pattern},
  },
};

/// Lightweight summary of struct characteristics for enum helper generation.
///
/// This avoids re-converting entire schemas when determining if a struct
/// is eligible for helper constructor generation.
#[derive(Debug, Clone)]
pub(crate) struct StructSummary {
  /// Whether the struct can derive Default.
  pub has_default: bool,
  /// Required fields (name and type) that must be provided in constructors.
  pub required_fields: Vec<(FieldNameToken, TypeRef)>,
  /// User-facing fields (name and type), excluding const/doc_hidden fields.
  pub user_fields: Vec<(FieldNameToken, TypeRef)>,
}

impl From<&StructDef> for StructSummary {
  fn from(s: &StructDef) -> Self {
    Self {
      has_default: s.has_default(),
      required_fields: s
        .required_fields()
        .map(|f| (f.name.clone(), f.rust_type.clone()))
        .collect(),
      user_fields: s
        .fields
        .iter()
        .filter(|f| !f.doc_hidden)
        .map(|f| (f.name.clone(), f.rust_type.clone()))
        .collect(),
    }
  }
}

/// Cache for sharing generated Rust types across the schema graph.
///
/// Prevents duplication of structs and enums by hashing schemas and storing mapping.
pub(crate) struct SharedSchemaCache {
  schema_to_type: BTreeMap<String, String>,
  enum_to_type: BTreeMap<Vec<String>, String>,
  union_refs_to_type: BTreeMap<(BTreeSet<String>, Option<String>), String>,
  generated_types: Vec<RustType>,
  used_names: BTreeSet<String>,
  precomputed_names: BTreeMap<String, String>,
  precomputed_enum_names: BTreeMap<Vec<String>, String>,
  /// Cached struct summaries for enum helper generation.
  struct_summaries: BTreeMap<String, StructSummary>,
}

impl SharedSchemaCache {
  /// Creates a new empty cache.
  pub(crate) fn new() -> Self {
    Self {
      schema_to_type: BTreeMap::new(),
      enum_to_type: BTreeMap::new(),
      union_refs_to_type: BTreeMap::new(),
      generated_types: vec![],
      used_names: BTreeSet::new(),
      precomputed_names: BTreeMap::new(),
      precomputed_enum_names: BTreeMap::new(),
      struct_summaries: BTreeMap::new(),
    }
  }

  /// Sets precomputed names for schemas, useful for deterministic naming or overrides.
  pub(crate) fn set_precomputed_names(
    &mut self,
    names: BTreeMap<String, String>,
    enum_names: BTreeMap<Vec<String>, String>,
  ) {
    self.precomputed_names = names;
    self.precomputed_enum_names = enum_names;
  }

  /// Retrieves a cached type name for a schema, if it exists.
  pub(crate) fn get_type_name(&self, schema: &ObjectSchema) -> anyhow::Result<Option<String>> {
    let schema_hash = hashing::hash_schema(schema)?;
    Ok(self.schema_to_type.get(&schema_hash).cloned())
  }

  /// Retrieves a cached name for an enum based on its values.
  pub(crate) fn get_enum_name(&self, values: &[String]) -> Option<String> {
    self
      .enum_to_type
      .get(values)
      .or_else(|| self.precomputed_enum_names.get(values))
      .cloned()
  }

  /// Checks if an enum with the given values has already been generated.
  pub(crate) fn is_enum_generated(&self, values: &[String]) -> bool {
    self.enum_to_type.contains_key(values)
  }

  /// Registers an enum name for a set of values.
  pub(crate) fn register_enum(&mut self, values: Vec<String>, name: String) {
    self.enum_to_type.insert(values, name);
  }

  /// Retrieves a cached name for a union enum based on its variant refs and discriminator.
  pub(crate) fn get_union_name(&self, refs: &BTreeSet<String>, discriminator: Option<&str>) -> Option<String> {
    self
      .union_refs_to_type
      .get(&(refs.clone(), discriminator.map(String::from)))
      .cloned()
  }

  /// Registers a union enum name for a set of variant refs and discriminator.
  pub(crate) fn register_union(&mut self, refs: BTreeSet<String>, discriminator: Option<String>, name: String) {
    self.union_refs_to_type.insert((refs, discriminator), name);
  }

  /// Marks a type name as used to prevent collisions.
  pub(crate) fn mark_name_used(&mut self, name: String) {
    self.used_names.insert(name);
  }

  /// Pre-registers a top-level schema so inline schemas with identical structure reuse it.
  pub(crate) fn register_top_level_schema(&mut self, schema: &ObjectSchema, name: &str) -> anyhow::Result<()> {
    let schema_hash = hashing::hash_schema(schema)?;
    let rust_name = to_rust_type_name(name);
    self.schema_to_type.insert(schema_hash, rust_name.clone());
    self.used_names.insert(rust_name);
    Ok(())
  }

  /// Gets a preferred name for a schema, using precomputed names or generating a unique one.
  pub(crate) fn get_preferred_name(&self, schema: &ObjectSchema, base_name: &str) -> anyhow::Result<String> {
    let schema_hash = hashing::hash_schema(schema)?;
    if let Some(name) = self.precomputed_names.get(&schema_hash) {
      return Ok(name.clone());
    }
    Ok(self.make_unique_name(base_name))
  }

  /// Registers a new type definition in the cache.
  ///
  /// Handles name collisions, enum reuse, and stores the generated Rust type.
  pub(crate) fn register_type(
    &mut self,
    schema: &ObjectSchema,
    base_name: &str,
    mut nested_types: Vec<RustType>,
    type_def: RustType,
  ) -> anyhow::Result<String> {
    let schema_hash = hashing::hash_schema(schema)?;

    if !is_relaxed_enum_pattern(schema)
      && let Some(values) = extract_enum_values(schema)
      && let Some(existing_name) = self.enum_to_type.get(&values)
    {
      self.schema_to_type.insert(schema_hash, existing_name.clone());
      return Ok(existing_name.clone());
    }

    let mut name = base_name.to_string();

    if self.used_names.contains(&name) {
      if let Some(existing_name) = self.schema_to_type.get(&schema_hash) {
        return Ok(existing_name.clone());
      }
      name = self.make_unique_name(&name);
    }

    self.used_names.insert(name.clone());
    self.schema_to_type.insert(schema_hash, name.clone());

    // If this is an enum, register its values too (if not already)
    if let Some(values) = extract_enum_values(schema) {
      self.enum_to_type.insert(values, name.clone());
    }

    // Update the name in the struct/enum definition if we renamed it
    let mut final_type_def = type_def;
    match &mut final_type_def {
      RustType::Struct(s) => s.name = StructToken::from(name.clone()),
      RustType::Enum(e) => e.name = EnumToken::new(&name),
      _ => {}
    }

    self.generated_types.append(&mut nested_types);
    self.generated_types.push(final_type_def);

    Ok(name)
  }

  /// Generates a unique name based on a base name, ensuring no collisions with used names.
  pub(crate) fn make_unique_name(&self, base: &str) -> String {
    let rust_name = to_rust_type_name(base);
    ensure_unique(&rust_name, &self.used_names)
  }

  /// Checks if a name is already used by a different schema.
  ///
  /// Returns true if the name is in use AND the schema hash doesn't match any existing entry.
  pub(crate) fn name_conflicts_with_different_schema(&self, name: &str, schema: &ObjectSchema) -> anyhow::Result<bool> {
    if !self.used_names.contains(name) {
      return Ok(false);
    }
    let schema_hash = hashing::hash_schema(schema)?;
    let same_schema = self.schema_to_type.get(&schema_hash).is_some_and(|n| n == name);
    Ok(!same_schema)
  }

  /// Consumes the cache and returns all generated Rust types.
  pub(crate) fn into_types(self) -> Vec<RustType> {
    self.generated_types
  }

  /// Stores a struct summary for enum helper generation.
  pub(crate) fn register_struct_summary(&mut self, type_name: &str, summary: StructSummary) {
    self.struct_summaries.insert(type_name.to_string(), summary);
  }

  /// Retrieves a cached struct summary by type name.
  pub(crate) fn get_struct_summary(&self, type_name: &str) -> Option<&StructSummary> {
    self.struct_summaries.get(type_name)
  }
}

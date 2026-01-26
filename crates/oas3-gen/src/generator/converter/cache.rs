use std::collections::{BTreeMap, BTreeSet};

use oas3::spec::ObjectSchema;

use super::hashing::CanonicalSchema;
use crate::{
  generator::{
    ast::{EnumToken, RustType, StructDef, StructToken, TypeRef},
    naming::identifiers::{ensure_unique, to_rust_type_name},
  },
  utils::SchemaExt,
};

#[derive(Default, Debug, Clone)]
struct NameRegistry {
  used_names: BTreeSet<String>,
}

impl NameRegistry {
  /// Converts a base string to a valid Rust type name and appends a numeric suffix if
  /// the name already exists in the registry.
  ///
  /// The returned name is not reserved; call [`reserve`] to mark it as used.
  fn make_unique(&self, base: &str) -> String {
    let rust_name = to_rust_type_name(base);
    ensure_unique(&rust_name, &self.used_names)
  }

  /// Returns the preferred name if it is available and not yet allocated, otherwise
  /// generates a unique name from the fallback base string.
  fn determine_name(&self, preferred: Option<&str>, fallback_base: &str) -> String {
    if let Some(pref) = preferred
      && !self.used_names.contains(pref)
    {
      return pref.to_string();
    }
    self.make_unique(fallback_base)
  }

  /// Returns `true` if the given name has already been reserved in this registry.
  fn is_allocated(&self, name: &str) -> bool {
    self.used_names.contains(name)
  }

  /// Marks a name as used, preventing it from being assigned to future types.
  fn reserve(&mut self, name: String) {
    self.used_names.insert(name);
  }
}

#[derive(Default, Debug, Clone)]
struct SchemaIdentity {
  schema_to_type: BTreeMap<CanonicalSchema, String>,
  precomputed: BTreeMap<CanonicalSchema, String>,
}

impl SchemaIdentity {
  /// Stores a mapping of canonical schemas to their precomputed type names for
  /// deterministic naming across code generation runs.
  fn set_precomputed(&mut self, names: BTreeMap<CanonicalSchema, String>) {
    self.precomputed = names;
  }

  /// Returns the type name previously assigned to this canonical schema, or `None`
  /// if the schema has not been registered.
  fn lookup(&self, canonical: &CanonicalSchema) -> Option<&str> {
    self.schema_to_type.get(canonical).map(String::as_str)
  }

  /// Returns the precomputed type name for this canonical schema if one was set
  /// during initialization.
  fn get_precomputed(&self, canonical: &CanonicalSchema) -> Option<&str> {
    self.precomputed.get(canonical).map(String::as_str)
  }

  /// Records that a canonical schema has been assigned the given type name,
  /// enabling deduplication of identical schema structures.
  fn record(&mut self, canonical: CanonicalSchema, type_name: String) {
    self.schema_to_type.insert(canonical, type_name);
  }

  /// Returns `true` if this canonical schema is mapped to exactly the given name.
  fn has_mapping(&self, canonical: &CanonicalSchema, expected_name: &str) -> bool {
    self.schema_to_type.get(canonical).is_some_and(|n| n == expected_name)
  }
}

#[derive(Default, Debug, Clone)]
struct EnumRegistry {
  value_sets_to_type: BTreeMap<Vec<String>, String>,
  precomputed: BTreeMap<Vec<String>, String>,
}

impl EnumRegistry {
  /// Stores a mapping of enum value sets to their precomputed type names for
  /// deterministic deduplication of identical enums.
  fn set_precomputed(&mut self, names: BTreeMap<Vec<String>, String>) {
    self.precomputed = names;
  }

  /// Returns the type name for an enum with the given values, checking the runtime
  /// cache first, then falling back to precomputed names.
  fn lookup(&self, values: &[String]) -> Option<&str> {
    self
      .value_sets_to_type
      .get(values)
      .or_else(|| self.precomputed.get(values))
      .map(String::as_str)
  }

  /// Returns `true` if an enum with these values has been registered at runtime
  /// (as opposed to only existing in precomputed names).
  fn is_registered(&self, values: &[String]) -> bool {
    self.value_sets_to_type.contains_key(values)
  }

  /// Records that an enum with the given values has been generated with the specified
  /// type name.
  fn register(&mut self, values: Vec<String>, type_name: String) {
    self.value_sets_to_type.insert(values, type_name);
  }
}

#[derive(PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Clone)]
struct UnionKey {
  refs: BTreeSet<String>,
  discriminator: Option<String>,
}

impl UnionKey {
  /// Creates a composite key for union deduplication from variant schema references
  /// and an optional discriminator property name.
  fn new(refs: BTreeSet<String>, discriminator: Option<String>) -> Self {
    Self { refs, discriminator }
  }
}

#[derive(Default, Debug, Clone)]
struct UnionRegistry {
  union_keys_to_type: BTreeMap<UnionKey, String>,
}

impl UnionRegistry {
  /// Returns the type name for a union with the given variant references and
  /// discriminator, or `None` if no such union has been registered.
  fn lookup(&self, refs: &BTreeSet<String>, discriminator: Option<&str>) -> Option<&str> {
    let key = UnionKey::new(refs.clone(), discriminator.map(String::from));
    self.union_keys_to_type.get(&key).map(String::as_str)
  }

  /// Records that a union with the given variant references and discriminator has
  /// been generated with the specified type name.
  fn register(&mut self, refs: BTreeSet<String>, discriminator: Option<String>, type_name: String) {
    let key = UnionKey::new(refs, discriminator);
    self.union_keys_to_type.insert(key, type_name);
  }
}

#[derive(Default, Debug, Clone)]
pub(crate) struct TypeCollector {
  pub(crate) types: Vec<RustType>,
}

impl TypeCollector {
  /// Appends nested dependency types followed by the main type, ensuring dependencies
  /// appear before their dependents in the output.
  fn add(&mut self, mut nested: Vec<RustType>, main: RustType) {
    self.types.append(&mut nested);
    self.types.push(main);
  }

  /// Consumes the collector and returns all accumulated types in insertion order.
  fn into_types(self) -> Vec<RustType> {
    self.types
  }
}

#[derive(Default, Debug, Clone)]
struct StructIndex {
  structs: BTreeMap<String, StructDef>,
}

impl StructIndex {
  /// Stores a struct definition indexed by its type name for later retrieval.
  fn register(&mut self, type_name: String, def: StructDef) {
    self.structs.insert(type_name, def);
  }

  /// Returns the struct definition for the given type name, or `None` if not registered.
  fn get(&self, type_name: &str) -> Option<&StructDef> {
    self.structs.get(type_name)
  }
}

#[derive(Default, Debug, Clone)]
struct TypeRefRegistry {
  resolved_types: BTreeMap<CanonicalSchema, TypeRef>,
}

impl TypeRefRegistry {
  /// Returns the cached TypeRef for a canonical schema, or `None` if not yet resolved.
  fn lookup(&self, canonical: &CanonicalSchema) -> Option<&TypeRef> {
    self.resolved_types.get(canonical)
  }

  /// Caches the resolved TypeRef for a canonical schema to avoid redundant resolution.
  fn register(&mut self, canonical: CanonicalSchema, type_ref: TypeRef) {
    self.resolved_types.insert(canonical, type_ref);
  }
}

pub(crate) struct TypeRegistration {
  pub(crate) assigned_name: String,
  pub(crate) canonical: CanonicalSchema,
  pub(crate) should_register_enum: bool,
  pub(crate) enum_values: Option<Vec<String>>,
}

#[derive(Debug, Clone)]
pub(crate) struct SharedSchemaCache {
  names: NameRegistry,
  schemas: SchemaIdentity,
  enums: EnumRegistry,
  unions: UnionRegistry,
  pub(crate) types: TypeCollector,
  structs: StructIndex,
  type_refs: TypeRefRegistry,
}

impl SharedSchemaCache {
  /// Creates an empty schema cache with no registered types, names, or precomputed mappings.
  pub(crate) fn new() -> Self {
    Self {
      names: NameRegistry::default(),
      schemas: SchemaIdentity::default(),
      enums: EnumRegistry::default(),
      unions: UnionRegistry::default(),
      types: TypeCollector::default(),
      structs: StructIndex::default(),
      type_refs: TypeRefRegistry::default(),
    }
  }

  /// Configures precomputed type names for schemas and enums, enabling deterministic
  /// naming that persists across code generation runs.
  pub(crate) fn set_precomputed_names(
    &mut self,
    schema_names: BTreeMap<CanonicalSchema, String>,
    enum_names: BTreeMap<Vec<String>, String>,
  ) {
    self.schemas.set_precomputed(schema_names);
    self.enums.set_precomputed(enum_names);
  }

  /// Returns the Rust type name assigned to this schema, or `None` if the schema
  /// has not been registered.
  pub(crate) fn get_type_name(&self, schema: &ObjectSchema) -> anyhow::Result<Option<String>> {
    let canonical = CanonicalSchema::from_schema(schema)?;
    Ok(self.schemas.lookup(&canonical).map(String::from))
  }

  /// Returns the type name for an enum with the given values, or `None` if no such
  /// enum has been registered.
  pub(crate) fn get_enum_name(&self, values: &[String]) -> Option<String> {
    self.enums.lookup(values).map(String::from)
  }

  /// Returns `true` if an enum with these values has already been generated
  /// (not just precomputed).
  pub(crate) fn is_enum_generated(&self, values: &[String]) -> bool {
    self.enums.is_registered(values)
  }

  /// Records that an enum with the given values has been generated with the
  /// specified type name.
  pub(crate) fn register_enum(&mut self, values: Vec<String>, name: String) {
    self.enums.register(values, name);
  }

  /// Returns the type name for a union with the given variant references and
  /// discriminator, or `None` if no such union has been registered.
  pub(crate) fn get_union_name(&self, refs: &BTreeSet<String>, discriminator: Option<&str>) -> Option<String> {
    self.unions.lookup(refs, discriminator).map(String::from)
  }

  /// Records that a union with the given variant references and discriminator has
  /// been generated with the specified type name.
  pub(crate) fn register_union(&mut self, refs: BTreeSet<String>, discriminator: Option<String>, name: String) {
    self.unions.register(refs, discriminator, name);
  }

  /// Reserves a type name without associating it with a schema, preventing it from
  /// being assigned to future types.
  pub(crate) fn mark_name_used(&mut self, name: String) {
    self.names.reserve(name);
  }

  /// Registers a top-level schema from `components/schemas` so that inline schemas
  /// with identical structure will reference this type instead of creating duplicates.
  pub(crate) fn register_top_level_schema(&mut self, schema: &ObjectSchema, name: &str) -> anyhow::Result<()> {
    let canonical = CanonicalSchema::from_schema(schema)?;
    let rust_name = to_rust_type_name(name);
    self.schemas.record(canonical, rust_name.clone());
    self.names.reserve(rust_name);
    Ok(())
  }

  /// Returns the precomputed name for this schema if one exists, otherwise generates
  /// a unique name from the base string.
  pub(crate) fn get_preferred_name(&self, schema: &ObjectSchema, base_name: &str) -> anyhow::Result<String> {
    let canonical = CanonicalSchema::from_schema(schema)?;
    if let Some(precomputed) = self.schemas.get_precomputed(&canonical) {
      Ok(precomputed.to_string())
    } else {
      Ok(self.make_unique_name(base_name))
    }
  }

  /// Prepares type registration by checking for enum reuse, resolving name conflicts,
  /// and determining whether this schema needs enum registration.
  ///
  /// The `enum_cache_key` should be provided for schemas with enum values to enable
  /// deduplication across schemas with identical value sets. Does not mutate cache
  /// state; call [`commit_registration`] to finalize.
  pub(crate) fn prepare_registration(
    &self,
    schema: &ObjectSchema,
    base_name: &str,
    enum_cache_key: Option<Vec<String>>,
  ) -> anyhow::Result<TypeRegistration> {
    let canonical = CanonicalSchema::from_schema(schema)?;

    if !schema.is_relaxed_enum_pattern()
      && let Some(ref values) = enum_cache_key
      && let Some(existing_name) = self.enums.lookup(values)
    {
      let should_register_enum = !self.enums.is_registered(values);
      return Ok(TypeRegistration {
        assigned_name: existing_name.to_string(),
        canonical,
        should_register_enum,
        enum_values: should_register_enum.then(|| values.clone()),
      });
    }

    let assigned_name = if let Some(existing_name) = self.schemas.lookup(&canonical) {
      existing_name.to_string()
    } else {
      let preferred = self.schemas.get_precomputed(&canonical);
      self.names.determine_name(preferred, base_name)
    };

    let (should_register_enum, enum_values) = if schema.has_relaxed_anyof_enum() {
      (false, None)
    } else if let Some(values) = enum_cache_key {
      (true, Some(values))
    } else {
      (false, None)
    };

    Ok(TypeRegistration {
      assigned_name,
      canonical,
      should_register_enum,
      enum_values,
    })
  }

  /// Commits a prepared registration by reserving the name, recording schema and
  /// enum mappings, and storing the type definitions.
  pub(crate) fn commit_registration(
    &mut self,
    registration: TypeRegistration,
    nested_types: Vec<RustType>,
    type_def: RustType,
  ) {
    self.names.reserve(registration.assigned_name.clone());
    self
      .schemas
      .record(registration.canonical, registration.assigned_name.clone());

    if registration.should_register_enum
      && let Some(values) = registration.enum_values
    {
      self.enums.register(values, registration.assigned_name.clone());
    }

    self.types.add(nested_types, type_def);
  }

  /// Generates a unique type name from a base string without reserving it.
  ///
  /// The returned name will not conflict with any currently reserved names.
  pub(crate) fn make_unique_name(&self, base: &str) -> String {
    self.names.make_unique(base)
  }

  /// Returns `true` if the given name is already allocated to a structurally different
  /// schema, indicating a naming conflict that requires a unique suffix.
  pub(crate) fn name_conflicts_with_different_schema(&self, name: &str, schema: &ObjectSchema) -> anyhow::Result<bool> {
    if !self.names.is_allocated(name) {
      return Ok(false);
    }
    let canonical = CanonicalSchema::from_schema(schema)?;
    Ok(!self.schemas.has_mapping(&canonical, name))
  }

  /// Consumes the cache and returns all accumulated type definitions.
  pub(crate) fn into_types(self) -> Vec<RustType> {
    self.types.into_types()
  }

  /// Stores a struct definition indexed by type name for later retrieval when
  /// building type references or validating field access.
  pub(crate) fn register_struct_def(&mut self, type_name: &str, struct_def: StructDef) {
    self.structs.register(type_name.to_string(), struct_def);
  }

  /// Returns the struct definition for the given type name, or `None` if not registered.
  pub(crate) fn get_struct_def(&self, type_name: &str) -> Option<&StructDef> {
    self.structs.get(type_name)
  }

  /// Returns the cached TypeRef for this schema, or `None` if not yet resolved.
  pub(crate) fn get_type_ref(&self, schema: &ObjectSchema) -> anyhow::Result<Option<TypeRef>> {
    let canonical = CanonicalSchema::from_schema(schema)?;
    Ok(self.type_refs.lookup(&canonical).cloned())
  }

  /// Caches a resolved TypeRef for this schema to avoid redundant resolution on
  /// subsequent encounters.
  pub(crate) fn register_type_ref(&mut self, schema: &ObjectSchema, type_ref: TypeRef) -> anyhow::Result<()> {
    let canonical = CanonicalSchema::from_schema(schema)?;
    self.type_refs.register(canonical, type_ref);
    Ok(())
  }

  /// Updates the name field of a struct or enum definition to the given name,
  /// leaving other type variants unchanged.
  pub(crate) fn apply_name_to_type(mut type_def: RustType, name: &str) -> RustType {
    match &mut type_def {
      RustType::Struct(s) => s.name = StructToken::from(name.to_string()),
      RustType::Enum(e) => e.name = EnumToken::new(name),
      _ => {}
    }
    type_def
  }
}

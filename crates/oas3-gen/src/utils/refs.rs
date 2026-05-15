use indexmap::{IndexMap, IndexSet};
use oas3::spec::{ObjectOrReference, ObjectSchema, Ref, Schema};

/// Parses a schema `$ref` path and extracts the referenced schema name.
///
/// Returns the schema name if the path references a valid internal component
/// (i.e., paths starting with `#/components`). Returns `None` for external
/// references or invalid paths.
///
/// In OpenAPI specifications, the `$ref` keyword uses JSON Pointer syntax
/// to reference reusable components. Internal references within the same
/// document use the `#/components/` prefix followed by the component type and
/// name (e.g., `#/components/schemas/User`).
pub fn parse_schema_ref_path(ref_path: &str) -> Option<String> {
  if !ref_path.starts_with("#/components") {
    return None;
  }

  match ref_path.parse::<Ref>() {
    Ok(component) => Some(component.name),
    Err(_) => None,
  }
}

/// Extracts the schema name from a schema reference.
///
/// Returns [`Some`] with the schema name if the object is a reference
/// ([`ObjectOrReference::Ref`]) to an internal component. Returns [`None`]
/// if the object is an inline schema ([`ObjectOrReference::Object`]) or if
/// the reference points to an external document.
pub trait SchemaRefName {
  fn schema_ref_name(&self) -> Option<String>;
}

impl SchemaRefName for Schema {
  fn schema_ref_name(&self) -> Option<String> {
    match self {
      Schema::Object(schema_ref) => schema_ref.schema_ref_name(),
      Schema::Boolean(_) => None,
    }
  }
}

impl SchemaRefName for ObjectOrReference<ObjectSchema> {
  fn schema_ref_name(&self) -> Option<String> {
    match self {
      ObjectOrReference::Ref { ref_path, .. } => parse_schema_ref_path(ref_path),
      ObjectOrReference::Object(_) => None,
    }
  }
}

/// Decomposes a [`Schema`] into its inline object or `$ref` path without forcing the caller
/// through the nested `Schema::Object(Box<ObjectOrReference<_>>)` shape that `oas3` exposes.
pub trait SchemaInspect {
  /// Returns the inline [`ObjectSchema`] when this schema is a `Schema::Object`
  /// holding an `ObjectOrReference::Object`. Returns `None` for boolean schemas
  /// and `$ref` references.
  fn as_inline(&self) -> Option<&ObjectSchema>;

  /// Returns the `$ref` path when this schema is a `Schema::Object` holding an
  /// `ObjectOrReference::Ref`. Returns `None` for boolean and inline schemas.
  fn ref_path(&self) -> Option<&str>;
}

impl SchemaInspect for Schema {
  fn as_inline(&self) -> Option<&ObjectSchema> {
    let Schema::Object(obj_ref) = self else {
      return None;
    };
    match obj_ref.as_ref() {
      ObjectOrReference::Object(schema) => Some(schema),
      ObjectOrReference::Ref { .. } => None,
    }
  }

  fn ref_path(&self) -> Option<&str> {
    let Schema::Object(obj_ref) = self else {
      return None;
    };
    match obj_ref.as_ref() {
      ObjectOrReference::Ref { ref_path, .. } => Some(ref_path),
      ObjectOrReference::Object(_) => None,
    }
  }
}

impl SchemaInspect for ObjectOrReference<ObjectSchema> {
  fn as_inline(&self) -> Option<&ObjectSchema> {
    match self {
      ObjectOrReference::Object(schema) => Some(schema),
      ObjectOrReference::Ref { .. } => None,
    }
  }

  fn ref_path(&self) -> Option<&str> {
    match self {
      ObjectOrReference::Ref { ref_path, .. } => Some(ref_path),
      ObjectOrReference::Object(_) => None,
    }
  }
}

/// Extracts a union fingerprint from a slice of schema references.
///
/// Collects named schema references in declaration order for union deduplication.
/// This is used to identify union types (oneOf/anyOf) that share the same ordered variants.
pub fn extract_union_fingerprint<T: SchemaRefName>(variants: &[T]) -> UnionFingerprint {
  variants
    .iter()
    .filter_map(SchemaRefName::schema_ref_name)
    .collect::<IndexSet<_>>()
    .into_iter()
    .collect()
}

pub type SchemaMap = IndexMap<String, ObjectSchema>;
pub type SchemaSet = IndexSet<String>;
pub type UnionFingerprint = Vec<String>;

/// Maps union type fingerprints to generated type names.
///
/// Union types (e.g., `oneOf` or `anyOf` in OpenAPI) that contain the same ordered sequence of
/// schema references are identified by a fingerprint. This type maps those
/// fingerprints to stable names while preserving declaration order semantics.
pub type UnionFingerprints = IndexMap<UnionFingerprint, String>;

/// Builds union fingerprints from a collection of schemas.
///
/// Scans all schemas for `oneOf` and `anyOf` compositions and creates a mapping
/// from the ordered referenced schema names to the parent schema name. This enables
/// deduplication of union types that share the same ordered variants.
///
/// Only unions with 2 or more named references are included.
pub fn build_union_fingerprints(schemas: &SchemaMap) -> UnionFingerprints {
  let mut fingerprints = UnionFingerprints::new();
  for (name, schema) in schemas {
    for variants in [&schema.one_of, &schema.any_of] {
      let refs = extract_union_fingerprint(variants);
      if refs.len() >= 2 {
        fingerprints.insert(refs, name.clone());
      }
    }
  }
  fingerprints
}

use std::collections::{BTreeMap, BTreeSet};

use oas3::spec::{ObjectOrReference, ObjectSchema, Ref};

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

/// Extracts the schema name from an [`ObjectOrReference`] variant.
///
/// Returns [`Some`] with the schema name if the object is a reference
/// ([`ObjectOrReference::Ref`]) to an internal component. Returns [`None`]
/// if the object is an inline schema ([`ObjectOrReference::Object`]) or if
/// the reference points to an external document.
///
/// This is a convenience wrapper around [`parse_schema_ref_path`] that
/// handles the common case where you have an [`ObjectOrReference<ObjectSchema>`]
/// and need to determine if it references a named component.
pub fn extract_schema_ref_name(obj_ref: &ObjectOrReference<ObjectSchema>) -> Option<String> {
  match obj_ref {
    ObjectOrReference::Ref { ref_path, .. } => parse_schema_ref_path(ref_path),
    ObjectOrReference::Object(_) => None,
  }
}

/// Extracts a union fingerprint from a slice of schema references.
///
/// Collects all named schema references into a sorted set for union deduplication.
/// This is used to identify union types (oneOf/anyOf) that share the same set of variants.
pub fn extract_union_fingerprint(variants: &[ObjectOrReference<ObjectSchema>]) -> BTreeSet<String> {
  variants.iter().filter_map(extract_schema_ref_name).collect()
}

/// Maps union type fingerprints to generated type names.
///
/// Union types (e.g., `oneOf` or `anyOf` in OpenAPI) that contain the same set of
/// schema references are identified by a fingerprint. This type maps those
/// fingerprints to stable names, ensuring consistent type generation across the API.
pub type UnionFingerprints = BTreeMap<BTreeSet<String>, String>;

/// Builds union fingerprints from a collection of schemas.
///
/// Scans all schemas for `oneOf` and `anyOf` compositions and creates a mapping
/// from the set of referenced schema names to the parent schema name. This enables
/// deduplication of union types that share the same set of variants.
///
/// Only unions with 2 or more named references are included.
pub fn build_union_fingerprints(schemas: &BTreeMap<String, ObjectSchema>) -> UnionFingerprints {
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

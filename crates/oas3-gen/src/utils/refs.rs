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

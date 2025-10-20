use std::{collections::HashSet, sync::LazyLock};

use any_ascii::any_ascii;
use inflections::Inflect;
use regex::Regex;

static FORBIDDEN_IDENTIFIERS: LazyLock<HashSet<&str>> = LazyLock::new(|| {
  [
    "as", "break", "const", "continue", "crate", "else", "enum", "extern", "false", "fn", "for", "if", "impl", "in",
    "let", "loop", "match", "mod", "move", "mut", "pub", "ref", "return", "static", "struct", "super", "trait", "true",
    "type", "unsafe", "use", "where", "while", "async", "await", "dyn", "try", "abstract", "become", "box", "do",
    "final", "macro", "override", "priv", "typeof", "unsized", "virtual", "yield", "gen",
    // 'self' is a special case for fields but is treated as a keyword here for simplicity.
    // The field-specific logic will handle the `self_` transformation.
    "self", "Self",
  ]
  .into_iter()
  .collect()
});

static RESERVED_PASCAL_CASE: LazyLock<HashSet<&str>> = LazyLock::new(|| {
  ["Clone", "Copy", "Display", "Self", "Send", "Sync", "Type", "Vec"]
    .into_iter()
    .collect()
});

/// A single, powerful sanitization function that handles the common base transformations.
/// It transliterates to ASCII, replaces invalid characters with underscores, collapses
/// consecutive underscores, and trims any leading or trailing underscores.
fn sanitize(input: &str) -> String {
  if input.is_empty() {
    return String::new();
  }

  // Compile static regexes only once.
  static INVALID_CHARS_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"[^A-Za-z0-9_]+").unwrap());
  static MULTI_UNDERSCORE_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"_+").unwrap());

  let ascii = any_ascii(input);
  let replaced = INVALID_CHARS_RE.replace_all(&ascii, "_");
  let collapsed = MULTI_UNDERSCORE_RE.replace_all(&replaced, "_");

  collapsed.trim_matches('_').to_string()
}

/// Converts a string into a valid Rust field name (`snake_case`).
///
/// # Rules:
/// 1. If the string starts with `-`, it's stripped and "negative_" is prepended to the result.
/// 2. Sanitizes the base string.
/// 3. Converts to `snake_case`.
/// 4. If the result is `self`, it becomes `self_`.
/// 5. If the result is a keyword, it gets a raw identifier prefix (`r#`).
/// 6. If the result starts with a digit, it's prefixed with `_`.
/// 7. If the result is empty, it becomes `_`.
pub(crate) fn to_rust_field_name(name: &str) -> String {
  // Check for leading `-` which indicates a "negative" or inverse meaning
  let (has_leading_minus, name_without_minus) = if let Some(stripped) = name.strip_prefix('-') {
    (true, stripped)
  } else {
    (false, name)
  };

  let mut ident = sanitize(name_without_minus).to_snake_case();

  if ident.is_empty() {
    return "_".to_string();
  }

  // Prepend "negative_" if original name started with `-`
  if has_leading_minus {
    ident = format!("negative_{}", ident);
  }

  if ident == "self" {
    return "self_".to_string();
  }

  if FORBIDDEN_IDENTIFIERS.contains(ident.as_str()) {
    return format!("r#{}", ident);
  }

  if ident.starts_with(|c: char| c.is_ascii_digit()) {
    ident.insert(0, '_');
  }

  ident
}

/// Converts a string into a valid Rust type name (`PascalCase`).
///
/// # Rules:
/// 1. If the string starts with `-`, it's stripped and "Negative" is prepended to the result.
/// 2. Sanitizes the base string.
/// 3. Converts to `PascalCase`.
/// 4. If the result is a reserved name (e.g., `Clone`, `Vec`), it gets a raw identifier prefix (`r#`).
/// 5. If the result starts with a digit, it's prefixed with `T`.
/// 6. If the result is empty, it becomes `Unnamed`.
pub(crate) fn to_rust_type_name(name: &str) -> String {
  // Check for leading `-` which indicates a "negative" or inverse meaning
  let (has_leading_minus, name_without_minus) = if let Some(stripped) = name.strip_prefix('-') {
    (true, stripped)
  } else {
    (false, name)
  };

  let sanitized = sanitize(name_without_minus);

  static DIGIT_TO_UPPER_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(\d)([A-Z])").expect("bad regex"));

  let preprocessed = DIGIT_TO_UPPER_RE.replace_all(&sanitized, "${1}_${2}");

  let mut ident = preprocessed.to_snake_case().to_pascal_case();

  if ident.is_empty() {
    return "Unnamed".to_string();
  }

  // Prepend "Negative" if original name started with `-`
  if has_leading_minus {
    ident = format!("Negative{}", ident);
  }

  if RESERVED_PASCAL_CASE.contains(ident.as_str()) {
    return format!("r#{}", ident);
  }

  if ident.starts_with(|c: char| c.is_ascii_digit()) {
    ident.insert(0, 'T');
  }

  ident
}

/// Converts a slice of string parts into a valid Rust constant name for a regex.
pub(crate) fn regex_const_name(key: &[&str]) -> String {
  let joined = key.iter().map(|part| sanitize(part)).collect::<Vec<_>>().join("_");

  let mut ident = joined.to_constant_case();

  if ident.starts_with(|c: char| c.is_ascii_digit()) {
    ident.insert(0, '_');
  }

  format!("REGEX_{}", ident)
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn test_field_names() {
    assert_eq!(to_rust_field_name("foo-bar"), "foo_bar");
    assert_eq!(to_rust_field_name("match"), "r#match");
    assert_eq!(to_rust_field_name("self"), "self_");
    assert_eq!(to_rust_field_name("123name"), "_123name");
    assert_eq!(to_rust_field_name(""), "_");
    assert_eq!(to_rust_field_name("  "), "_");
  }

  #[test]
  fn test_field_names_negative_prefix() {
    assert_eq!(to_rust_field_name("-created-date"), "negative_created_date");
    assert_eq!(to_rust_field_name("-id"), "negative_id");
    assert_eq!(to_rust_field_name("-modified-date"), "negative_modified_date");
    assert_eq!(to_rust_field_name("-"), "_");
  }

  #[test]
  fn test_type_names() {
    assert_eq!(to_rust_type_name("oAuth"), "OAuth");
    assert_eq!(to_rust_type_name("-INF"), "NegativeInf");
    assert_eq!(to_rust_type_name("123Response"), "T123Response");
    assert_eq!(to_rust_type_name(""), "Unnamed");
    assert_eq!(to_rust_type_name("  "), "Unnamed");
  }

  #[test]
  fn test_type_names_negative_prefix() {
    assert_eq!(to_rust_type_name("-created-date"), "NegativeCreatedDate");
    assert_eq!(to_rust_type_name("-id"), "NegativeId");
    assert_eq!(to_rust_type_name("-modified-date"), "NegativeModifiedDate");
    assert_eq!(to_rust_type_name("-child-position"), "NegativeChildPosition");
    assert_eq!(to_rust_type_name("-"), "Unnamed");
  }

  #[test]
  fn test_type_name_reserved_pascal() {
    assert_eq!(to_rust_type_name("clone"), "r#Clone");
    assert_eq!(to_rust_type_name("Vec"), "r#Vec");
  }

  #[test]
  fn test_const_name() {
    assert_eq!(regex_const_name(&["foo.bar", "baz"]), "REGEX_FOO_BAR_BAZ");
    assert_eq!(regex_const_name(&["1a", "2b"]), "REGEX__1A_2B");
  }
}

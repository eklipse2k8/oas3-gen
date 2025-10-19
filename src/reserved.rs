use any_ascii::any_ascii;
use inflections::Inflect;

/// A constant array of all strict and reserved keywords in Rust.
const RUST_KEYWORDS: &[&str] = &[
  "as", "break", "const", "continue", "crate", "else", "enum", "extern", "false", "fn", "for", "if", "impl", "in",
  "let", "loop", "match", "mod", "move", "mut", "pub", "ref", "return", "self", "Self", "static", "struct", "super",
  "trait", "true", "type", "unsafe", "use", "where", "while", "async", "await", "dyn", "abstract", "become", "box",
  "do", "final", "macro", "override", "priv", "typeof", "unsized", "virtual", "yield", "try", "gen",
];

/// Convert a schema name to a valid Rust identifier (for field names)
pub(crate) fn to_rust_field_name(name: &str) -> String {
  // Replace invalid identifier characters with underscores before case conversion
  let sanitized = name.replace(['.', '/', ' '], "_");
  let cleaned = any_ascii(&sanitized).to_snake_case();

  // Handle special keywords that can't use r# prefix
  if cleaned == "self" || cleaned == "Self" {
    return format!("{}_", cleaned);
  }

  RUST_KEYWORDS
    .iter()
    .find(|&&kw| kw == cleaned)
    .map(|_| format!("r#{}", cleaned))
    .unwrap_or(cleaned)
}

/// Convert a schema name to a valid Rust type name (PascalCase)
pub(crate) fn to_rust_type_name(name: &str) -> String {
  // Split on underscores and convert each part to PascalCase
  // This handles mixed cases like "api_publicApi" -> "ApiPublicApi"
  let cleaned = name
    .split('_')
    .filter(|s| !s.is_empty())
    .map(|part| any_ascii(part).to_pascal_case())
    .collect::<Vec<_>>()
    .join("");

  match cleaned.as_str() {
    "Clone" | "Copy" | "Display" | "Self" | "Send" | "Sync" | "Type" | "Vec" => format!("r#{}", cleaned),
    _ => cleaned,
  }
}

pub(crate) fn regex_const_name(key: &[&str]) -> String {
  let joined = key
    .iter()
    .map(|part| any_ascii::any_ascii(part))
    .collect::<Vec<_>>()
    .join("_");

  format!("REGEX_{}", joined.to_constant_case())
}

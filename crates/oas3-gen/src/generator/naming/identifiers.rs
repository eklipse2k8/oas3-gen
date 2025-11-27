use std::{
  char::{ToLowercase, ToUppercase},
  collections::{BTreeSet, HashSet},
  iter::Peekable,
  sync::LazyLock,
};

use any_ascii::any_ascii;
use inflections::Inflect;
use regex::Regex;

pub(crate) static FORBIDDEN_IDENTIFIERS: LazyLock<HashSet<&str>> = LazyLock::new(|| {
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

static INVALID_CHARS_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"[^A-Za-z0-9_]+").unwrap());
static MULTI_UNDERSCORE_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"_+").unwrap());

/// A single, powerful sanitization function that handles the common base transformations.
/// It transliterates to ASCII, replaces invalid characters with underscores, collapses
/// consecutive underscores, and trims any leading or trailing underscores.
pub(crate) fn sanitize(input: &str) -> String {
  if input.is_empty() {
    return String::new();
  }

  let ascii = any_ascii(input);
  let replaced = INVALID_CHARS_RE.replace_all(&ascii, "_");
  let collapsed = MULTI_UNDERSCORE_RE.replace_all(&replaced, "_");

  collapsed.trim_matches('_').to_string()
}

/// Splits a PascalCase string into words.
/// Handles adjacent uppercase letters correctly (e.g., `"XMLParser"` -> `["XML", "Parser"]`).
pub(crate) fn split_pascal_case(name: &str) -> Vec<String> {
  if name.is_empty() {
    return vec![];
  }

  let mut words = vec![];
  let mut current_word = String::new();
  let chars: Vec<char> = name.chars().collect();

  for (i, &ch) in chars.iter().enumerate() {
    if ch.is_uppercase() && !current_word.is_empty() {
      let prev_is_lower = i > 0 && chars[i - 1].is_lowercase();
      let next_is_lower = i + 1 < chars.len() && chars[i + 1].is_lowercase();

      if prev_is_lower || next_is_lower {
        words.push(std::mem::take(&mut current_word));
      }
    }
    current_word.push(ch);
  }

  if !current_word.is_empty() {
    words.push(current_word);
  }

  words
}

/// Ensures a name is unique within a set of used names, appending a numeric suffix if needed.
pub(crate) fn ensure_unique(base_name: &str, used_names: &BTreeSet<String>) -> String {
  if !used_names.contains(base_name) {
    return base_name.to_string();
  }
  let mut i = 2;
  loop {
    let new_name = format!("{base_name}{i}");
    if !used_names.contains(&new_name) {
      return new_name;
    }
    i += 1;
  }
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
  let has_leading_minus = name.starts_with('-');
  let name_without_minus = name.strip_prefix('-').unwrap_or(name);

  let mut ident = sanitize(name_without_minus).to_snake_case();

  if ident.is_empty() {
    return "_".to_string();
  }

  if has_leading_minus {
    ident = format!("negative_{ident}");
  }

  if ident == "self" {
    return "self_".to_string();
  }

  if FORBIDDEN_IDENTIFIERS.contains(ident.as_str()) {
    return format!("r#{ident}");
  }

  prefix_if_digit_start(&mut ident, '_');
  ident
}

pub(crate) fn to_rust_const_name(input: &str) -> String {
  let sanitized = sanitize(input);
  if sanitized.is_empty() {
    return "UNNAMED".to_string();
  }

  let mut ident = sanitized.to_constant_case();
  if ident.starts_with(|c: char| c.is_ascii_digit()) {
    ident.insert(0, '_');
  }
  ident
}

/// Converts a string into a valid Rust type name (`PascalCase`).
///
/// # Rules:
/// 1. If the string starts with `-`, it's stripped and "Negative" is prepended to the result.
/// 2. If the input already has mixed case (both upper and lowercase, no separators), preserve capitalization.
/// 3. Otherwise, sanitizes the base string and converts to `PascalCase` using capitalize_words.
/// 4. If the result is a reserved name (e.g., `Clone`, `Vec`), it gets a raw identifier prefix (`r#`).
/// 5. If the result starts with a digit, it's prefixed with `T`.
/// 6. If the result is empty, it becomes `Unnamed`.
pub(crate) fn to_rust_type_name(name: &str) -> String {
  let has_leading_minus = name.starts_with('-');
  let name_without_minus = name.strip_prefix('-').unwrap_or(name);

  let has_separators = name_without_minus.contains(['-', '_', '.', ' ']);
  let has_upper = name_without_minus.chars().any(|c| c.is_ascii_uppercase());
  let has_lower = name_without_minus.chars().any(|c| c.is_ascii_lowercase());
  let appears_mixed_case = !has_separators && has_upper && has_lower;

  let mut ident = if appears_mixed_case {
    let ascii = any_ascii(name_without_minus);
    let cleaned: String = ascii.chars().filter(char::is_ascii_alphanumeric).collect();

    if cleaned.is_empty() {
      cleaned
    } else {
      let mut chars = cleaned.chars();
      match chars.next() {
        None => String::new(),
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
      }
    }
  } else {
    let ascii = any_ascii(name_without_minus);

    ascii
      .chars()
      .capitalize_words_with_boundaries()
      .filter(char::is_ascii_alphanumeric)
      .collect()
  };

  if ident.is_empty() {
    return "Unnamed".to_string();
  }

  if has_leading_minus {
    ident = format!("Negative{ident}");
  }

  if RESERVED_PASCAL_CASE.contains(ident.as_str()) {
    return format!("r#{ident}");
  }

  prefix_if_digit_start(&mut ident, 'T');
  ident
}

fn prefix_if_digit_start(ident: &mut String, prefix: char) {
  if ident.starts_with(|c: char| c.is_ascii_digit()) {
    ident.insert(0, prefix);
  }
}

pub(crate) fn to_http_header_name(name: &str) -> String {
  name.to_ascii_lowercase()
}

/// An extension trait for char iterators to add word capitalization.
pub trait CapitalizeWordsExt: Iterator<Item = char> {
  fn capitalize_words_with_boundaries(self) -> CapitalizeWordsWithBoundaries<Self>
  where
    Self: Sized;
}

impl<I> CapitalizeWordsExt for I
where
  I: Iterator<Item = char>,
{
  fn capitalize_words_with_boundaries(self) -> CapitalizeWordsWithBoundaries<Self>
  where
    Self: Sized,
  {
    CapitalizeWordsWithBoundaries {
      iter: self.peekable(),
      capitalize_next: true,
      prev_was_lower: false,
      pending_upper: None,
      pending_lower: None,
    }
  }
}

pub struct CapitalizeWordsWithBoundaries<I>
where
  I: Iterator<Item = char>,
{
  iter: Peekable<I>,
  capitalize_next: bool,
  prev_was_lower: bool,
  pending_upper: Option<ToUppercase>,
  pending_lower: Option<ToLowercase>,
}

impl<I> Iterator for CapitalizeWordsWithBoundaries<I>
where
  I: Iterator<Item = char>,
{
  type Item = char;

  fn next(&mut self) -> Option<Self::Item> {
    if let Some(ref mut upper_iter) = self.pending_upper {
      if let Some(c) = upper_iter.next() {
        return Some(c);
      }
      self.pending_upper = None;
    }

    if let Some(ref mut lower_iter) = self.pending_lower {
      if let Some(c) = lower_iter.next() {
        return Some(c);
      }
      self.pending_lower = None;
    }

    let c = self.iter.next()?;

    if !c.is_ascii_alphanumeric() {
      self.capitalize_next = self.iter.peek().is_some_and(char::is_ascii_alphanumeric);
      self.prev_was_lower = false;
      return Some(c);
    }

    let is_lower = c.is_ascii_lowercase();
    let is_upper = c.is_ascii_uppercase();

    let should_capitalize = self.capitalize_next
      || (self.prev_was_lower && is_upper)
      || (is_upper && self.iter.peek().is_some_and(char::is_ascii_lowercase));

    self.prev_was_lower = is_lower;
    self.capitalize_next = false;

    if should_capitalize {
      self.pending_upper = Some(c.to_uppercase());
      self.pending_upper.as_mut().unwrap().next()
    } else {
      self.pending_lower = Some(c.to_lowercase());
      self.pending_lower.as_mut().unwrap().next()
    }
  }
}

use std::{
  collections::{HashMap, HashSet},
  hash::Hash,
};

use inflections::Inflect;
use oas3::spec::{ObjectOrReference, ObjectSchema};

use crate::generator::{
  ast::{EnumVariantToken, VariantDef},
  naming::{
    constants::VARIANT_KIND_SUFFIX,
    identifiers::{split_pascal_case, to_rust_type_name},
  },
  schema_registry::RefCollector,
};

pub(crate) struct CommonVariantName {
  pub(crate) name: String,
  pub(crate) has_suffix: bool,
}

impl CommonVariantName {
  pub(crate) fn union_name_or<S>(
    variants: &[ObjectOrReference<ObjectSchema>],
    suffix_part: S,
    fallback: impl FnOnce() -> String,
  ) -> String
  where
    S: AsRef<str>,
  {
    let Some(common) = extract_common_variant_prefix(variants) else {
      return fallback();
    };
    if common.has_suffix {
      format!("{}{VARIANT_KIND_SUFFIX}", common.name)
    } else {
      format!("{}{}", common.name, suffix_part.as_ref())
    }
  }
}

/// Extracts a semantic name from union variant references by combining the first
/// common prefix segment with the common suffix.
///
/// For variants like `BetaResponseCharLocationCitation`, `BetaResponseUrlCitation`,
/// `BetaResponseFileCitation`, this returns `CommonVariantName { name: "BetaCitation", has_suffix: true }`.
///
/// For variants like `BetaTool`, `BetaBashTool20241022`, this returns `CommonVariantName { name: "Beta", has_suffix: false }`.
///
/// The `has_suffix` field indicates whether a common suffix was found. When true, the extracted name
/// is semantically complete and should be used as-is. When false, the property name should
/// be appended for clarity.
///
/// Returns `None` if fewer than 2 variants have references or no common prefix exists.
pub(crate) fn extract_common_variant_prefix(variants: &[ObjectOrReference<ObjectSchema>]) -> Option<CommonVariantName> {
  let ref_names = variants
    .iter()
    .filter_map(RefCollector::parse_schema_ref)
    .collect::<Vec<String>>();

  if ref_names.len() < 2 {
    return None;
  }

  let segments = ref_names
    .iter()
    .map(|n| split_pascal_case(n))
    .collect::<Vec<Vec<String>>>();
  let first = segments.first().filter(|s| !s.is_empty())?;
  let rest = &segments[1..];

  let prefix_len = common_prefix_len(first, rest);
  if prefix_len == 0 {
    return None;
  }

  let suffix_len = common_suffix_len(first, rest);
  Some(build_common_variant_name(first, prefix_len, suffix_len))
}

/// Counts word segments shared at the start of all name lists.
///
/// # Example
/// ```text
/// first = ["User", "Create", "Request"]
/// rest  = [["User", "Update", "Request"], ["User", "Delete", "Request"]]
/// => 1 (only "User" is common prefix)
/// ```
///
#[must_use]
pub(crate) fn common_prefix_len<S: AsRef<str>>(first: &[S], rest: &[Vec<S>]) -> usize {
  first
    .iter()
    .enumerate()
    .take_while(|(i, seg)| {
      let seg_str = seg.as_ref();
      rest
        .iter()
        .all(|other| other.get(*i).map(AsRef::as_ref) == Some(seg_str))
    })
    .count()
}

/// Counts word segments shared at the end of all name lists.
///
/// # Example
/// ```text
/// first = ["Create", "User", "Response"]
/// rest  = [["Update", "User", "Response"], ["Delete", "User", "Response"]]
/// => 2 ("User", "Response" are common suffix)
/// ```
///
#[must_use]
pub(crate) fn common_suffix_len<S: AsRef<str>>(first: &[S], rest: &[Vec<S>]) -> usize {
  let min_len = std::iter::once(first.len())
    .chain(rest.iter().map(Vec::len))
    .min()
    .unwrap_or(0);

  (1..=min_len)
    .take_while(|&offset| {
      let seg = first[first.len() - offset].as_ref();
      rest.iter().all(|other| {
        other
          .get(other.len() - offset)
          .map(AsRef::as_ref)
          .is_some_and(|s| s == seg)
      })
    })
    .count()
}

/// Constructs a `CommonVariantName` from word segments using prefix/suffix lengths.
///
/// If a suffix exists, combines the first segment with the suffix (e.g., "Beta" + "Citation").
/// Otherwise, joins all prefix segments.
///
/// # Example
/// ```text
/// segments = ["Beta", "Response", "Url", "Citation"], prefix_len = 1, suffix_len = 1
/// => CommonVariantName { name: "BetaCitation", has_suffix: true }
///
/// segments = ["Beta", "Tool"], prefix_len = 1, suffix_len = 0
/// => CommonVariantName { name: "Beta", has_suffix: false }
/// ```
///
#[must_use]
fn build_common_variant_name(segments: &[String], prefix_len: usize, suffix_len: usize) -> CommonVariantName {
  if suffix_len > 0 {
    let suffix = segments[segments.len() - suffix_len..].join("");
    CommonVariantName {
      name: format!("{}{suffix}", segments[0]),
      has_suffix: true,
    }
  } else {
    CommonVariantName {
      name: segments[..prefix_len].join(""),
      has_suffix: false,
    }
  }
}

/// Holds the result of normalizing a schema value into a Rust identifier.
///
/// Converts strings, numbers, and booleans into PascalCase identifiers
/// suitable for enum variants, preserving original values for serde rename.
pub struct NormalizedVariant {
  /// The valid Rust identifier (e.g., "Value10_5").
  pub name: String,
  /// The original value string for serialization (e.g., "10.5").
  pub rename_value: String,
}

#[derive(Debug, Clone, Copy)]
pub struct UnsupportedJsonValue;

impl TryFrom<&serde_json::Value> for NormalizedVariant {
  type Error = UnsupportedJsonValue;

  fn try_from(value: &serde_json::Value) -> Result<Self, Self::Error> {
    match value {
      serde_json::Value::String(str_val) => Ok(NormalizedVariant {
        name: to_rust_type_name(str_val),
        rename_value: str_val.clone(),
      }),
      serde_json::Value::Number(num) => {
        let raw_str = if num.is_i64() {
          num.as_i64().unwrap().to_string()
        } else if num.is_f64() {
          num.as_f64().unwrap().to_string()
        } else {
          return Err(UnsupportedJsonValue);
        };
        let safe_name = raw_str.replace(['.', '-'], "_");
        Ok(NormalizedVariant {
          name: format!("Value{safe_name}"),
          rename_value: raw_str,
        })
      }
      serde_json::Value::Bool(bool_val) => Ok(NormalizedVariant {
        name: if *bool_val { "True".into() } else { "False".into() },
        rename_value: bool_val.to_string(),
      }),
      _ => Err(UnsupportedJsonValue),
    }
  }
}

/// Strips common PascalCase word segments from variant names to make them concise.
///
/// This function identifies word boundaries in PascalCase names, finds segments
/// shared by ALL variants at the beginning (prefix) and end (suffix), then removes
/// them. Changes are only applied if all resulting names remain non-empty and unique.
///
/// # Algorithm
///
/// 1. Split each name into PascalCase word segments
///    - `"CreateUserResponse"` -> `["Create", "User", "Response"]`
/// 2. Find the longest common prefix (word segments shared at the start)
/// 3. Find the longest common suffix (word segments shared at the end)
/// 4. Strip both from each name, rejoining the remaining segments
/// 5. Validate: abort if any name becomes empty or duplicates arise
///
/// # Examples
///
/// **Shared suffix:**
/// - Input: `["CreateUserResponse", "UpdateUserResponse", "DeleteUserResponse"]`
/// - Common prefix: 0 words (Create != Update != Delete)
/// - Common suffix: 2 words (User, Response)
/// - Output: `["Create", "Update", "Delete"]`
///
/// **Shared prefix and suffix:**
/// - Input: `["UserCreateRequest", "UserUpdateRequest", "UserDeleteRequest"]`
/// - Common prefix: 1 word (User)
/// - Common suffix: 1 word (Request)
/// - Output: `["Create", "Update", "Delete"]`
///
/// **No change (would create duplicates):**
/// - Input: `["GetUserRequest", "GetUserResponse"]`
/// - Common prefix: 2 words (Get, User)
/// - Common suffix: 0 words
/// - Stripped: `["Request", "Response"]` - valid, so applied
///
/// **No change (would empty a name):**
/// - Input: `["User", "UserProfile"]`
/// - Common prefix: 1 word (User)
/// - Stripping would empty the first variant, so no changes applied
pub fn strip_common_affixes(variants: Vec<VariantDef>) -> Vec<VariantDef> {
  if variants.len() < 2 {
    return variants;
  }

  let word_segments = variants
    .iter()
    .map(|v| split_pascal_case(&v.name.to_string()))
    .collect::<Vec<Vec<String>>>();
  let first = &word_segments[0];
  let rest = &word_segments[1..];

  let common_prefix_len = common_prefix_len(first, rest);
  let common_suffix_len = common_suffix_len(first, rest);

  let stripped_names = word_segments
    .iter()
    .map(|segments| extract_middle_segments(segments, common_prefix_len, common_suffix_len, ""))
    .collect::<Vec<String>>();

  if !all_non_empty_and_unique(&stripped_names) {
    return variants;
  }

  variants
    .into_iter()
    .zip(stripped_names)
    .map(|(mut variant, new_name)| {
      variant.name = EnumVariantToken::from(new_name);
      variant
    })
    .collect()
}

/// Joins word segments after stripping common prefix and suffix.
///
/// Returns the full joined string if stripping would produce an empty result.
///
/// # Example
/// ```text
/// segments = ["User", "Create", "Response"], prefix_len = 1, suffix_len = 1
/// => "Create"
///
/// segments = ["Response"], prefix_len = 1, suffix_len = 0
/// => "Response" (unchanged, stripping would empty it)
/// ```
///
#[must_use]
pub(crate) fn extract_middle_segments<S>(
  segments: &[S],
  prefix_len: usize,
  suffix_len: usize,
  separator: &str,
) -> String
where
  S: AsRef<str>,
{
  let end_idx = segments.len().saturating_sub(suffix_len);
  let parts = if prefix_len < end_idx {
    &segments[prefix_len..end_idx]
  } else {
    segments
  };
  parts.iter().map(AsRef::as_ref).collect::<Vec<_>>().join(separator)
}

/// Returns true if all strings are non-empty and unique.
///
/// Used to validate that affix stripping produces valid variant names.
///
/// # Example
/// ```text
/// ["Create", "Update", "Delete"] => true
/// ["Create", "Create"]           => false (duplicate)
/// ["Create", ""]                 => false (empty)
/// ```
///
#[must_use]
pub(crate) fn all_non_empty_and_unique<S>(names: &[S]) -> bool
where
  S: AsRef<str> + Eq + Hash,
{
  let mut seen = HashSet::with_capacity(names.len());
  names.iter().all(|s| !s.as_ref().is_empty() && seen.insert(s))
}

#[derive(Debug, Clone)]
struct Candidate {
  short: String,
  original: String,
}

/// Derives method names for multiple enum variants, ensuring they remain unique
/// after filtering out common words with the enum name.
///
/// # Algorithm
///
/// 1. Split enum name into words (e.g., `"MyEnum"` -> `["My", "Enum"]`).
/// 2. For each variant, split into words and filter out words present in the enum name.
/// 3. If the filtered result is unique across all variants, use it.
/// 4. Otherwise, fall back to the full variant name (snake_cased).
///
pub(crate) fn derive_method_names<S, V>(name: S, variants: &[V]) -> Vec<String>
where
  S: AsRef<str>,
  V: AsRef<str>,
{
  if variants.is_empty() {
    return vec![];
  }

  let exclusion_set = split_pascal_case(name.as_ref())
    .iter()
    .map(|w| w.to_lowercase())
    .collect::<HashSet<_>>();

  let mut short_counts = HashMap::new();

  let candidates = variants
    .iter()
    .map(|variant| {
      let variant = variant.as_ref();

      let parts = split_pascal_case(variant)
        .iter()
        .map(|w| w.to_lowercase())
        .filter(|w| !exclusion_set.contains(w))
        .collect::<Vec<_>>();

      let short_name = if parts.is_empty() {
        // Fallback: If all words were filtered out, use the full name as the "short" name.
        variant.to_snake_case()
      } else {
        parts.join("_")
      };

      *short_counts.entry(short_name.clone()).or_insert(0) += 1;

      Candidate {
        short: short_name,
        original: variant.to_string(),
      }
    })
    .collect::<Vec<_>>();

  candidates
    .iter()
    .map(|ctx| {
      // If the short name appears more than once, fall back to the full original name.
      if short_counts[&ctx.short] > 1 {
        ctx.original.to_snake_case()
      } else {
        ctx.short.clone()
      }
    })
    .collect()
}

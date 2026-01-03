use std::{
  collections::{BTreeSet, HashMap, HashSet},
  hash::Hash,
};

use inflections::Inflect;
use oas3::spec::{ObjectOrReference, ObjectSchema, SchemaType, SchemaTypeSet};

use crate::generator::{
  ast::{EnumVariantToken, VariantDef},
  naming::{
    constants::{REQUEST_BODY_SUFFIX, RESPONSE_PREFIX, RESPONSE_SUFFIX},
    identifiers::{sanitize, split_pascal_case, to_rust_type_name},
  },
  schema_registry::{RefCollector, SchemaRegistry},
};

pub(crate) trait InferenceExt {
  /// Returns an iterator over all union variants (`anyOf` and `oneOf`) in a schema.
  ///
  /// # Example
  /// ```text
  /// schema.any_of = [A, B], schema.one_of = [C] => yields A, B, C
  /// ```
  fn union_variants(&self) -> impl Iterator<Item = &ObjectOrReference<ObjectSchema>>;

  /// Returns the single `SchemaType` if exactly one is defined, or the non-null type
  /// from a two-type nullable set (e.g., `[string, null]` -> `string`).
  fn single_type_or_nullable(&self) -> Option<SchemaType>;

  /// Returns true if the schema is a string type (including nullable string).
  fn is_string_type(&self) -> bool;

  /// Returns true if schema is an unconstrained string type (no enum/const restrictions).
  ///
  /// # Example
  /// ```text
  /// { "type": "string" }                    => true
  /// { "type": "string", "enum": ["a"] }     => false
  /// { "type": "string", "const": "x" }      => false
  /// ```
  fn is_freeform_string(&self) -> bool;

  /// Returns true if schema has enum values or a const constraint.
  ///
  /// # Example
  /// ```text
  /// { "enum": ["a", "b"] }  => true
  /// { "const": "x" }        => true
  /// { "type": "string" }    => false
  /// ```
  fn is_constrained(&self) -> bool;

  /// Checks if a schema matches the "relaxed enum" pattern.
  ///
  /// A relaxed enum is defined as having a freeform string variant (no enum values, no const)
  /// alongside other variants that are constrained (enum values or const).
  fn is_relaxed_enum_pattern(&self) -> bool;

  /// Extracts enum values from a schema, handling standard enums, oneOf/anyOf patterns,
  /// and relaxed enum patterns (mixed freeform string and constants).
  ///
  /// Returns `None` if no valid enum values could be extracted.
  fn extract_enum_values(&self) -> Option<Vec<String>>;

  /// Extracts string values from a schema's direct `enum` field.
  ///
  /// # Example
  /// ```text
  /// { "enum": ["active", "pending", 123] } => Some(["active", "pending"])
  /// { "type": "string" }                   => None
  /// ```
  fn extract_standard_enum_values(&self) -> Option<Vec<String>>;

  /// Infers a variant name for an inline schema in a union.
  fn infer_variant_name(&self, index: usize) -> String;

  /// Infers a union variant label from the schema, checking const value, ref name, and title.
  fn infer_union_variant_label(&self, ref_name: Option<&str>, index: usize) -> String;

  /// Infers a variant name for an object schema based on its properties.
  fn infer_object_variant_name(&self) -> String;

  /// Infers a name from the schema's required fields if exactly one exists.
  fn infer_name_from_required_fields(&self) -> Option<String>;

  /// Infers a name from the schema's $ref properties if exactly one exists.
  fn infer_name_from_ref_properties(&self) -> Option<String>;

  /// Infers a name from the schema's properties if exactly one exists.
  fn infer_name_from_single_property(&self) -> Option<String>;

  /// Infers a name for an inline schema based on its context (path, operation).
  ///
  /// Checks in order: title, single property name, path segments.
  fn infer_name_from_context(&self, path: &str, context: &str) -> String;
}

impl InferenceExt for ObjectSchema {
  fn union_variants(&self) -> impl Iterator<Item = &ObjectOrReference<ObjectSchema>> {
    self.any_of.iter().chain(&self.one_of)
  }

  fn single_type_or_nullable(&self) -> Option<SchemaType> {
    match &self.schema_type {
      Some(SchemaTypeSet::Single(t)) => Some(*t),
      Some(SchemaTypeSet::Multiple(types)) if types.len() == 2 && types.contains(&SchemaType::Null) => {
        types.iter().find(|t| **t != SchemaType::Null).copied()
      }
      _ => None,
    }
  }

  fn is_string_type(&self) -> bool {
    matches!(self.single_type_or_nullable(), Some(SchemaType::String))
  }

  fn is_freeform_string(&self) -> bool {
    self.is_string_type() && self.enum_values.is_empty() && self.const_value.is_none()
  }

  fn is_constrained(&self) -> bool {
    !self.enum_values.is_empty() || self.const_value.is_some()
  }

  fn is_relaxed_enum_pattern(&self) -> bool {
    has_mixed_string_variants(self.union_variants())
  }

  fn extract_enum_values(&self) -> Option<Vec<String>> {
    if let Some(values) = self.extract_standard_enum_values() {
      return Some(values);
    }

    let variants: Vec<_> = self.union_variants().collect();
    if variants.is_empty() {
      return None;
    }

    let has_freeform = variants
      .iter()
      .any(|v| matches!(v, ObjectOrReference::Object(s) if s.is_freeform_string()));

    if has_freeform {
      return extract_relaxed_enum_values(&variants);
    }

    if !self.one_of.is_empty() {
      return extract_oneof_const_values(&self.one_of);
    }

    None
  }

  fn extract_standard_enum_values(&self) -> Option<Vec<String>> {
    if self.enum_values.is_empty() {
      return None;
    }

    let mut values: Vec<_> = self
      .enum_values
      .iter()
      .filter_map(|v| v.as_str().map(String::from))
      .collect();

    if values.is_empty() {
      return None;
    }

    values.sort();
    Some(values)
  }

  fn infer_variant_name(&self, index: usize) -> String {
    if !self.enum_values.is_empty() {
      return "Enum".to_string();
    }
    if let Some(typ) = self.single_type_or_nullable() {
      return match typ {
        SchemaType::String => "String".to_string(),
        SchemaType::Number => "Number".to_string(),
        SchemaType::Integer => "Integer".to_string(),
        SchemaType::Boolean => "Boolean".to_string(),
        SchemaType::Array => "Array".to_string(),
        SchemaType::Object => self.infer_object_variant_name(),
        SchemaType::Null => "Null".to_string(),
      };
    }
    if self.schema_type.is_some() {
      return "Mixed".to_string();
    }
    let variants = if self.one_of.is_empty() {
      &self.any_of
    } else {
      &self.one_of
    };

    extract_common_variant_prefix(variants).map_or_else(|| format!("Variant{index}"), |c| c.name)
  }

  fn infer_union_variant_label(&self, ref_name: Option<&str>, index: usize) -> String {
    if let Some(const_value) = &self.const_value
      && let Ok(normalized) = NormalizedVariant::try_from(const_value)
    {
      return normalized.name;
    }

    if let Some(schema_name) = ref_name {
      return to_rust_type_name(schema_name);
    }

    if let Some(title) = &self.title {
      return to_rust_type_name(title);
    }

    self.infer_variant_name(index)
  }

  fn infer_object_variant_name(&self) -> String {
    if self.properties.is_empty() {
      return "Object".to_string();
    }

    if let Some(name) = self.infer_name_from_required_fields() {
      return name;
    }

    if let Some(name) = self.infer_name_from_ref_properties() {
      return name;
    }

    if let Some(name) = self.infer_name_from_single_property() {
      return name;
    }

    "Object".to_string()
  }

  fn infer_name_from_required_fields(&self) -> Option<String> {
    if self.required.len() == 1 {
      return Some(self.required[0].to_pascal_case());
    }
    None
  }

  fn infer_name_from_ref_properties(&self) -> Option<String> {
    let mut ref_names = self.properties.values().filter_map(|prop| {
      if let ObjectOrReference::Ref { ref_path, .. } = prop {
        SchemaRegistry::parse_ref(ref_path)
      } else {
        None
      }
    });

    if let Some(first) = ref_names.next()
      && ref_names.next().is_none()
    {
      return Some(first.to_pascal_case());
    }

    None
  }

  fn infer_name_from_single_property(&self) -> Option<String> {
    if self.properties.len() == 1 {
      return self.properties.keys().next().map(|name| name.to_pascal_case());
    }
    None
  }

  fn infer_name_from_context(&self, path: &str, context: &str) -> String {
    let is_request = context == REQUEST_BODY_SUFFIX;

    let with_suffix = |base: &str| {
      let sanitized_base = sanitize(base);
      if is_request {
        format!("{sanitized_base}{REQUEST_BODY_SUFFIX}")
      } else {
        format!("{sanitized_base}{RESPONSE_SUFFIX}")
      }
    };

    let with_context_suffix = |base: &str| {
      let sanitized_base = sanitize(base);
      if is_request {
        format!("{sanitized_base}{REQUEST_BODY_SUFFIX}")
      } else {
        format!("{sanitized_base}{context}{RESPONSE_SUFFIX}")
      }
    };

    if let Some(title) = &self.title {
      return with_suffix(title);
    }

    if self.properties.len() == 1
      && let Some((prop_name, _)) = self.properties.iter().next()
    {
      let singular = cruet::to_singular(prop_name);
      return with_suffix(&singular);
    }

    let segments: Vec<_> = path
      .split('/')
      .filter(|s| !s.is_empty() && !s.starts_with('{'))
      .collect();

    segments
      .last()
      .map(|&s| with_context_suffix(&cruet::to_singular(s)))
      .or_else(|| segments.first().map(|&s| with_context_suffix(s)))
      .unwrap_or_else(|| {
        if is_request {
          REQUEST_BODY_SUFFIX.to_string()
        } else {
          format!("{RESPONSE_PREFIX}{context}")
        }
      })
  }
}

pub(crate) struct CommonVariantName {
  pub(crate) name: String,
  pub(crate) has_suffix: bool,
}

impl CommonVariantName {
  pub(crate) fn union_name(variants: &[ObjectOrReference<ObjectSchema>], suffix_part: &str) -> Option<String> {
    let common = extract_common_variant_prefix(variants)?;
    if common.has_suffix {
      Some(format!("{}Kind", common.name))
    } else {
      Some(format!("{}{suffix_part}", common.name))
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
  let ref_names: Vec<String> = variants.iter().filter_map(RefCollector::parse_schema_ref).collect();

  if ref_names.len() < 2 {
    return None;
  }

  let segments: Vec<Vec<String>> = ref_names.iter().map(|n| split_pascal_case(n)).collect();
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

/// Checks if variants contain both freeform strings and constrained strings.
///
/// Used to detect "relaxed enum" patterns where an API accepts known enum values
/// plus arbitrary strings for forward compatibility.
///
/// # Example
/// ```text
/// anyOf: [{ type: string }, { type: string, enum: ["a", "b"] }] => true
/// anyOf: [{ type: string, enum: ["a"] }, { type: string, enum: ["b"] }] => false
/// ```
///
pub(crate) fn has_mixed_string_variants<'a>(
  variants: impl Iterator<Item = &'a ObjectOrReference<ObjectSchema>>,
) -> bool {
  let mut has_freeform = false;
  let mut has_constrained = false;

  for v in variants {
    if let ObjectOrReference::Object(s) = v {
      if s.is_freeform_string() {
        has_freeform = true;
      } else if s.is_constrained() {
        has_constrained = true;
      }
    }

    if has_freeform && has_constrained {
      return true;
    }
  }

  false
}

/// Extracts all enum/const string values from relaxed enum variants.
///
/// Collects values from inline schemas' `enum` arrays and `const` fields,
/// ignoring `$ref` variants and freeform strings.
///
/// # Example
/// ```text
/// anyOf: [{ type: string }, { const: "a" }, { enum: ["b", "c"] }]
/// => Some(["a", "b", "c"])
/// ```
///
fn extract_relaxed_enum_values(variants: &[&ObjectOrReference<ObjectSchema>]) -> Option<Vec<String>> {
  let values: BTreeSet<_> = variants
    .iter()
    .filter_map(|variant| match variant {
      ObjectOrReference::Object(s) => {
        let enum_values = s.enum_values.iter().filter_map(|v| v.as_str().map(String::from));
        let const_value = s.const_value.as_ref().and_then(|v| v.as_str().map(String::from));
        Some(enum_values.chain(const_value))
      }
      ObjectOrReference::Ref { .. } => None,
    })
    .flatten()
    .collect();

  if values.is_empty() {
    None
  } else {
    Some(values.into_iter().collect())
  }
}

/// Extracts const values from a oneOf where all variants are const strings.
///
/// Returns `None` if any variant is a `$ref` or lacks a string const value.
///
/// # Example
/// ```text
/// oneOf: [{ const: "a" }, { const: "b" }] => Some(["a", "b"])
/// oneOf: [{ const: "a" }, { $ref: "..." }] => None
/// ```
///
/// # Complexity
/// O(n log n) where n = number of oneOf variants (BTreeSet insertion).
fn extract_oneof_const_values(one_of: &[ObjectOrReference<ObjectSchema>]) -> Option<Vec<String>> {
  let mut const_values = BTreeSet::new();

  for variant in one_of {
    match variant {
      ObjectOrReference::Object(s) => {
        let const_str = s.const_value.as_ref().and_then(|v| v.as_str())?;
        const_values.insert(const_str.to_string());
      }
      ObjectOrReference::Ref { .. } => return None,
    }
  }

  if const_values.is_empty() {
    None
  } else {
    Some(const_values.into_iter().collect())
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

  let word_segments: Vec<Vec<String>> = variants
    .iter()
    .map(|v| split_pascal_case(&v.name.to_string()))
    .collect();
  let first = &word_segments[0];
  let rest = &word_segments[1..];

  let common_prefix_len = common_prefix_len(first, rest);
  let common_suffix_len = common_suffix_len(first, rest);

  let stripped_names: Vec<String> = word_segments
    .iter()
    .map(|segments| extract_middle_segments(segments, common_prefix_len, common_suffix_len, ""))
    .collect();

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

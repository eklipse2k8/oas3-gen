use std::{borrow::Cow, collections::BTreeSet, rc::Rc};

use anyhow::Context;
use indexmap::IndexMap;
use itertools::Itertools;
use oas3::spec::{Discriminator, ObjectSchema, Schema};

use super::{
  ConversionOutput,
  methods::MethodGenerator,
  union_types::{CollisionStrategy, UnionVariantSpec, variants_to_cache_key},
  value_enums::{ValueEnumBuilder, scalar_repr, variant_matches_value},
  variants::VariantBuilder,
};
use crate::{
  generator::{
    ast::{
      Documentation, EnumDefault, EnumDefaultValue, EnumVariantToken, RustPrimitive, RustType, TypeRef, VariantContent,
      VariantDef,
    },
    converter::{ConverterContext, discriminator::DiscriminatorConverter},
    naming::{
      identifiers::ensure_unique,
      inference::{NormalizedVariant, strip_common_affixes},
    },
    schema_registry::SchemaRegistry,
  },
  utils::{SchemaExt, SchemaInspect, SchemaRefName, SchemaResolveExt, schema_ext::SchemaExtIters},
};

#[derive(Clone, Debug)]
pub(crate) struct EnumConverter {
  context: Rc<ConverterContext>,
  value_enum_builder: ValueEnumBuilder,
}

impl EnumConverter {
  /// Creates a new enum converter with the specified converter context.
  ///
  /// The converter inherits case-sensitivity, variant ordering, and collision
  /// settings from the context's [`CodegenConfig`] and uses them to configure
  /// the underlying [`ValueEnumBuilder`].
  pub(crate) fn new(context: Rc<ConverterContext>) -> Self {
    let config = context.config();
    let strategy = if config.preserve_case_variants() {
      CollisionStrategy::Preserve
    } else {
      CollisionStrategy::Deduplicate
    };
    let value_enum_builder =
      ValueEnumBuilder::new(config.case_insensitive_enums(), config.sort_enum_variants(), strategy);
    Self {
      context,
      value_enum_builder,
    }
  }

  /// Converts an OpenAPI string enum schema into a Rust enum type.
  ///
  /// Extracts the `enum` values from the schema and generates a Rust enum
  /// with PascalCase variant names and `#[serde(rename)]` attributes preserving
  /// the original JSON string values.
  pub(crate) fn convert_value_enum(&self, name: &str, schema: &ObjectSchema) -> RustType {
    let variants = schema.extract_enum_entries(self.context.graph().spec());
    self
      .value_enum_builder
      .build_enum_from_variants(name, schema, variants, false)
  }

  /// Converts an `anyOf` of listed string values and a freeform `{ "type": "string" }`
  /// into a `#[non_exhaustive]` value enum that holds unlisted values in a trailing
  /// `Other(String)` variant.
  ///
  /// Returns `None` when the schema isn't that pattern: a `oneOf`, a union without
  /// a freeform string branch, or one whose listed values are absent or not all
  /// strings.
  pub(crate) fn try_convert_non_exhaustive_enum(&self, name: &str, schema: &ObjectSchema) -> Option<RustType> {
    let spec = self.context.graph().spec();
    if !schema.one_of.is_empty() || !schema.has_unconstrained_string_variant(spec) {
      return None;
    }

    let variants = schema.extract_enum_entries(spec);
    (!variants.is_empty() && scalar_repr(schema, &variants).is_none()).then(|| {
      self
        .value_enum_builder
        .build_enum_from_variants(name, schema, variants, true)
    })
  }

  /// Converts a `oneOf` or `anyOf` whose every non-null branch is a `const` into a
  /// closed value enum, the same type an `enum` array of those values produces.
  ///
  /// Returns `None` when any branch carries a payload or no branch has a `const`.
  pub(crate) fn try_convert_const_union(&self, name: &str, schema: &ObjectSchema) -> Option<RustType> {
    let spec = self.context.graph().spec();
    let branches = schema.union_variants().resolve_all(spec).collect::<Vec<_>>();
    let is_const_union = branches.iter().all(|b| b.is_null() || b.const_value.is_some())
      && branches.iter().any(|b| b.const_value.is_some());
    if !is_const_union {
      return None;
    }

    let variants = schema.extract_enum_entries(spec);
    (!variants.is_empty()).then(|| {
      self
        .value_enum_builder
        .build_enum_from_variants(name, schema, variants, false)
    })
  }
}

#[derive(Clone, Debug)]
pub(crate) struct UnionConverter {
  context: Rc<ConverterContext>,
  variant_builder: VariantBuilder,
  enum_converter: EnumConverter,
  method_generator: MethodGenerator,
  discriminator_converter: DiscriminatorConverter,
}

impl UnionConverter {
  /// Creates a new union converter with the specified converter context.
  ///
  /// Initializes internal builders for variant construction, non-exhaustive
  /// enum generation (anyOf with freeform strings), and helper method generation.
  pub(crate) fn new(context: Rc<ConverterContext>) -> Self {
    let variant_builder = VariantBuilder::new(context.clone());
    let enum_converter = EnumConverter::new(context.clone());
    let method_generator = MethodGenerator::new(context.clone());
    let discriminator_converter = DiscriminatorConverter::new(context.clone());

    Self {
      context,
      variant_builder,
      enum_converter,
      method_generator,
      discriminator_converter,
    }
  }

  /// Converts an OpenAPI `oneOf` or `anyOf` schema into a Rust enum type.
  ///
  /// For `anyOf` schemas containing both enumerated values and a freeform string
  /// branch, produces a non-exhaustive value enum ending in `Other(String)`.
  /// Otherwise, generates an untagged enum with one variant per union branch.
  ///
  /// If the schema has a discriminator mapping, upgrades the result to a
  /// discriminated enum with `#[serde(tag)]` instead of `#[serde(untagged)]`.
  ///
  /// Returns the main enum type plus any inline types generated for anonymous
  /// variant schemas.
  pub(crate) fn convert_union(&self, name: &str, schema: &ObjectSchema) -> anyhow::Result<ConversionOutput<RustType>> {
    if let Some(value_enum) = self
      .enum_converter
      .try_convert_non_exhaustive_enum(name, schema)
      .or_else(|| self.enum_converter.try_convert_const_union(name, schema))
    {
      return Ok(ConversionOutput::new(value_enum));
    }

    let output = self.collect_union_variants(name, schema)?;

    if !schema.enum_values.is_empty() {
      let variants = schema.extract_enum_entries(self.context.graph().spec());
      if !variants.is_empty()
        && let RustType::Enum(e) = &output.result
      {
        let cache_key = variants_to_cache_key(&variants);
        self
          .context
          .cache
          .borrow_mut()
          .register_enum(cache_key, e.name.to_string());
      }
    }

    Ok(output)
  }

  /// Builds enum variants from the union branches and assembles the final enum.
  ///
  /// Resolves each `oneOf` or `anyOf` branch to a variant spec, constructs
  /// [`VariantDef`]s, strips common name prefixes/suffixes for conciseness,
  /// and generates optional helper constructors.
  ///
  /// Attempts to upgrade to a discriminated enum if the schema contains a
  /// `discriminator` mapping; otherwise produces an untagged enum.
  fn collect_union_variants(&self, name: &str, schema: &ObjectSchema) -> anyhow::Result<ConversionOutput<RustType>> {
    let variants_src = if schema.one_of.is_empty() {
      &schema.any_of
    } else {
      &schema.one_of
    };

    let (variant_specs, has_null_variant) = self.collect_union_variant_specs(variants_src)?;

    let (mut variants, inline_types) = itertools::process_results(
      variant_specs.into_iter().map(|spec| {
        let output = self.variant_builder.build_variant(name, &spec)?;
        anyhow::Ok((output.result, output.inline_types))
      }),
      |iter| iter.unzip::<_, _, Vec<_>, Vec<_>>(),
    )?;
    let inline_types = inline_types.into_iter().flatten().collect::<Vec<_>>();

    variants = strip_common_affixes(variants);

    if self.context.config().sort_enum_variants() {
      variants = variants
        .into_iter()
        .sorted_by(|a, b| a.name.as_str().cmp(b.name.as_str()))
        .collect();
    }
    variants = plain_strings_last(variants);

    let default_mode = self.resolve_default_mode(schema, &mut variants, &inline_types, has_null_variant);
    variants = arrays_first(variants, &default_mode);

    let methods = if self.context.config().no_helpers() {
      vec![]
    } else {
      self.method_generator.build_constructors(&variants, &inline_types, name)
    };

    let main_enum = self
      .discriminator_converter
      .try_upgrade_to_discriminated(
        name,
        &self.with_inferred_discriminator(schema),
        &variants,
        methods.clone(),
      )
      .unwrap_or_else(|| {
        RustType::untagged_enum()
          .name(name)
          .docs(Documentation::from_optional(schema.description.as_ref()))
          .variants(variants)
          .methods(methods)
          .default_mode(default_mode)
          .call()
      });

    Ok(ConversionOutput::with_inline_types(main_enum, inline_types))
  }

  /// Gives a union that declares no `discriminator` the one its branches imply.
  ///
  /// When every non-null branch is a `$ref` to an object schema that requires the
  /// same property and fixes it to a distinct string `const`, that property
  /// identifies the branch. Left untagged, the first branch whose fields fit would
  /// win instead, because a constant property is never checked on input.
  fn with_inferred_discriminator<'s>(&self, schema: &'s ObjectSchema) -> Cow<'s, ObjectSchema> {
    if schema.discriminator.is_some() {
      return Cow::Borrowed(schema);
    }

    let spec = self.context.graph().spec();
    let mut branches = vec![];
    for branch in schema.union_variants() {
      let Some(ref_path) = branch.ref_path() else {
        if branch
          .as_inline()
          .is_some_and(crate::utils::schema_ext::SchemaExt::is_null)
        {
          continue;
        }
        return Cow::Borrowed(schema);
      };
      let Ok(resolved) = branch.resolve_object(spec) else {
        return Cow::Borrowed(schema);
      };
      if resolved.is_null() {
        continue;
      }
      branches.push((ref_path.to_string(), resolved));
    }
    if branches.len() < 2 {
      return Cow::Borrowed(schema);
    }

    let mut tags_by_property = IndexMap::<&str, Vec<String>>::new();
    for (_, branch) in &branches {
      for property in &branch.required {
        if let Some(tag) = SchemaRegistry::extract_const_discriminator_value(branch, property) {
          tags_by_property.entry(property).or_default().push(tag);
        }
      }
    }

    let Some((property, tags)) = tags_by_property
      .into_iter()
      .find(|(_, tags)| tags.len() == branches.len() && tags.iter().all_unique())
    else {
      return Cow::Borrowed(schema);
    };

    let mapping = tags
      .into_iter()
      .zip(&branches)
      .map(|(tag, (ref_path, _))| (tag, ref_path.clone()))
      .collect();
    Cow::Owned(ObjectSchema {
      discriminator: Some(Discriminator {
        property_name: property.to_string(),
        mapping: Some(mapping),
      }),
      ..schema.clone()
    })
  }

  fn resolve_default_mode(
    &self,
    schema: &ObjectSchema,
    variants: &mut [VariantDef],
    inline_types: &[RustType],
    has_null_variant: bool,
  ) -> EnumDefault {
    let default = schema.default.as_ref().filter(|v| !v.is_null());
    if let Some(value) = default
      && let Ok(normalized) = NormalizedVariant::try_from(value)
      && let Some(unit) = variants
        .iter_mut()
        .find(|v| v.content == VariantContent::Unit && variant_matches_value(v, &normalized.rename_value))
    {
      unit.default = true;
      return EnumDefault::Derive;
    }
    if let Some(value) = default
      && let Some(payload) = self.match_default_payload(value, variants, inline_types)
    {
      return EnumDefault::Value(payload);
    }

    if has_null_variant {
      EnumDefault::None
    } else {
      EnumDefault::Derive
    }
  }

  fn match_default_payload(
    &self,
    value: &serde_json::Value,
    variants: &mut [VariantDef],
    inline_types: &[RustType],
  ) -> Option<EnumDefaultValue> {
    let normalized = NormalizedVariant::try_from(value).ok()?;

    variants.iter_mut().find_map(|variant| {
      let type_ref = variant.single_wrapped_type().filter(|t| !t.is_array)?;
      let enum_variants = self.method_generator.resolve_enum_value_defs(type_ref, inline_types);
      let payload = wrapped_default_payload(type_ref, value, &normalized.rename_value, &enum_variants)?;
      variant.default = true;
      Some(payload)
    })
  }

  /// Extracts variant specifications from raw union branch references.
  ///
  /// For each branch, resolves the schema reference, infers a variant name
  /// (from `$ref` path, schema `title`, or positional fallback), and ensures
  /// uniqueness across all variants. Null schemas are skipped as they represent
  /// nullable wrappers rather than distinct variants; the returned flag records
  /// whether any null branch was seen.
  fn collect_union_variant_specs(&self, variants_src: &[Schema]) -> anyhow::Result<(Vec<UnionVariantSpec>, bool)> {
    let mut specs = vec![];
    let mut seen_names = BTreeSet::new();
    let mut has_null_variant = false;

    for (i, variant_ref) in variants_src.iter().enumerate() {
      let resolved = variant_ref
        .resolve_object(self.context.graph().spec())
        .context(format!("Schema resolution failed for union variant {i}"))?;

      if resolved.is_null() {
        has_null_variant = true;
        continue;
      }

      let ref_name = variant_ref.schema_ref_name().or_else(|| {
        if resolved.all_of.len() == 1 {
          resolved.all_of[0].schema_ref_name()
        } else {
          None
        }
      });

      let base_name = resolved.infer_union_variant_label(ref_name.as_deref(), i);
      let variant_name = ensure_unique(&base_name, &seen_names);
      seen_names.insert(variant_name.clone());

      specs.push(UnionVariantSpec {
        variant_name: EnumVariantToken::new(variant_name),
        resolved_schema: resolved,
        ref_name,
      });
    }

    Ok((specs, has_null_variant))
  }
}

/// Moves variants wrapping a plain `String` after the others, keeping their relative order.
///
/// An untagged enum deserializes into the first variant that accepts the input, and a
/// `String` accepts every JSON string, so trying it first would shadow wrapped value enums
/// such as `Chat(Chat)`.
fn plain_strings_last(variants: Vec<VariantDef>) -> Vec<VariantDef> {
  variants
    .into_iter()
    .sorted_by_key(|variant| {
      variant
        .single_wrapped_type()
        .is_some_and(|t| t.base_type == RustPrimitive::String && !t.is_array)
    })
    .collect()
}

/// Moves variants wrapping an array ahead of the others, keeping their relative order and
/// the variant `Default` derives.
///
/// A struct with `#[serde(default)]` deserializes from an empty JSON array, so trying it
/// first would shadow `Array(Vec<T>)`. An array variant only accepts a JSON array, which no
/// object variant validly matches.
fn arrays_first(mut variants: Vec<VariantDef>, default_mode: &EnumDefault) -> Vec<VariantDef> {
  let wraps_array = |variant: &VariantDef| variant.single_wrapped_type().is_some_and(|t| t.is_array);
  if variants.iter().is_sorted_by_key(|variant| !wraps_array(variant)) {
    return variants;
  }

  if matches!(default_mode, EnumDefault::Derive)
    && !variants.iter().any(|variant| variant.default)
    && let Some(first) = variants.first_mut()
  {
    first.default = true;
  }

  variants
    .into_iter()
    .sorted_by_key(|variant| !wraps_array(variant))
    .collect()
}

/// Builds the payload for a union variant wrapping the non-array `type_ref` from a schema
/// `default`.
///
/// `enum_variants` are the variants of the enum `type_ref` names, or empty when it names
/// anything else; only unit variants are considered. When there are any, the default is
/// accepted only through the one whose wire value is `rename_value`. Otherwise it is
/// accepted when its JSON kind fits the primitive. Returns `None` when the variant cannot
/// hold the value.
pub(crate) fn wrapped_default_payload(
  type_ref: &TypeRef,
  value: &serde_json::Value,
  rename_value: &str,
  enum_variants: &[VariantDef],
) -> Option<EnumDefaultValue> {
  let mut unit_variants = enum_variants
    .iter()
    .filter(|v| matches!(v.content, VariantContent::Unit))
    .peekable();
  if unit_variants.peek().is_some() {
    return unit_variants
      .find(|v| variant_matches_value(v, rename_value))
      .map(|inner| EnumDefaultValue {
        value: value.clone(),
        inner_variant: Some(inner.name.clone()),
      });
  }

  let accepts = match (&type_ref.base_type, value) {
    (RustPrimitive::String, serde_json::Value::String(_)) | (RustPrimitive::Bool, serde_json::Value::Bool(_)) => true,
    (primitive, serde_json::Value::Number(_)) if primitive.is_float() => true,
    (primitive, serde_json::Value::Number(n)) if primitive.is_integer() => n.is_i64() || n.is_u64(),
    _ => false,
  };

  accepts.then(|| EnumDefaultValue {
    value: value.clone(),
    inner_variant: None,
  })
}

use std::{collections::BTreeSet, rc::Rc};

use anyhow::Context;
use itertools::Itertools;
use oas3::spec::{ObjectSchema, Schema, SchemaType};

use super::{
  ConversionOutput,
  methods::MethodGenerator,
  relaxed_enum::RelaxedEnumBuilder,
  union_types::{CollisionStrategy, UnionVariantSpec, variants_to_cache_key},
  value_enums::{ValueEnumBuilder, variant_matches_value},
  variants::VariantBuilder,
};
use crate::{
  generator::{
    ast::{Documentation, EnumDefault, EnumDefaultValue, EnumVariantToken, RustPrimitive, RustType, VariantDef},
    converter::{ConverterContext, discriminator::DiscriminatorConverter},
    naming::{
      identifiers::ensure_unique,
      inference::{NormalizedVariant, strip_common_affixes},
    },
  },
  utils::{SchemaExt, SchemaRefName, SchemaResolveExt},
};

#[derive(Clone, Debug)]
pub(crate) struct EnumConverter {
  context: Rc<ConverterContext>,
  value_enum_builder: ValueEnumBuilder,
}

impl EnumConverter {
  /// Creates a new enum converter with the specified converter context.
  ///
  /// The converter inherits case-sensitivity settings from the context's
  /// [`CodegenConfig`] and uses them to configure the underlying
  /// [`ValueEnumBuilder`].
  pub(crate) fn new(context: Rc<ConverterContext>) -> Self {
    let case_insensitive = context.config().case_insensitive_enums();
    let sort_variants = context.config().sort_enum_variants();
    Self {
      context,
      value_enum_builder: ValueEnumBuilder::new(case_insensitive, sort_variants),
    }
  }

  /// Converts an OpenAPI string enum schema into a Rust enum type.
  ///
  /// Extracts the `enum` values from the schema and generates a Rust enum
  /// with PascalCase variant names and `#[serde(rename)]` attributes preserving
  /// the original JSON string values.
  ///
  /// The collision strategy (from config) determines how variant name collisions
  /// are resolved: either by appending numeric suffixes or by merging duplicates
  /// with serde aliases.
  pub(crate) fn convert_value_enum(&self, name: &str, schema: &ObjectSchema) -> RustType {
    let strategy = if self.context.config().preserve_case_variants() {
      CollisionStrategy::Preserve
    } else {
      CollisionStrategy::Deduplicate
    };

    let variants = schema.extract_enum_entries(self.context.graph().spec());
    let scalar_repr = Self::scalar_repr(schema);

    self.value_enum_builder.build_enum_from_variants(
      name,
      variants,
      strategy,
      Documentation::from_optional(schema.description.as_ref()),
      scalar_repr,
      schema.default.as_ref(),
    )
  }

  /// Determines the backing scalar primitive for a numeric value enum.
  ///
  /// Returns `Some` for `type: integer` / `type: number` schemas (honoring any
  /// numeric `format`), and `None` for string-backed enums, which use the
  /// derived serde path.
  fn scalar_repr(schema: &ObjectSchema) -> Option<RustPrimitive> {
    match schema.single_type() {
      Some(SchemaType::Integer) => Some(RustPrimitive::with_format_override(
        RustPrimitive::I64,
        schema.format.as_deref(),
      )),
      Some(SchemaType::Number) => Some(RustPrimitive::with_format_override(
        RustPrimitive::F64,
        schema.format.as_deref(),
      )),
      _ => None,
    }
  }
}

#[derive(Clone, Debug)]
pub(crate) struct UnionConverter {
  context: Rc<ConverterContext>,
  variant_builder: VariantBuilder,
  relaxed_enum_builder: RelaxedEnumBuilder,
  method_generator: MethodGenerator,
  discriminator_converter: DiscriminatorConverter,
}

impl UnionConverter {
  /// Creates a new union converter with the specified converter context.
  ///
  /// Initializes internal builders for variant construction, relaxed enum
  /// generation (anyOf with freeform strings), and helper method generation.
  pub(crate) fn new(context: Rc<ConverterContext>) -> Self {
    let variant_builder = VariantBuilder::new(context.clone());
    let relaxed_enum_builder = RelaxedEnumBuilder::new(context.clone());
    let method_generator = MethodGenerator::new(context.clone());
    let discriminator_converter = DiscriminatorConverter::new(context.clone());

    Self {
      context,
      variant_builder,
      relaxed_enum_builder,
      method_generator,
      discriminator_converter,
    }
  }

  /// Converts an OpenAPI `oneOf` or `anyOf` schema into a Rust enum type.
  ///
  /// For `anyOf` schemas containing both enumerated values and a freeform string
  /// branch, produces a "relaxed enum" with `Known` and `Other` variants.
  /// Otherwise, generates an untagged enum with one variant per union branch.
  ///
  /// If the schema has a discriminator mapping, upgrades the result to a
  /// discriminated enum with `#[serde(tag)]` instead of `#[serde(untagged)]`.
  ///
  /// Returns the main enum type plus any inline types generated for anonymous
  /// variant schemas.
  pub(crate) fn convert_union(&self, name: &str, schema: &ObjectSchema) -> anyhow::Result<ConversionOutput<RustType>> {
    if !schema.any_of.is_empty()
      && let Some(output) = self.relaxed_enum_builder.try_build_relaxed_enum(name, schema)
    {
      return Ok(output);
    }

    let output = self.collect_union_variants(name, schema)?;

    let should_register_enum = !schema.enum_values.is_empty() || schema.has_relaxed_anyof_enum();
    if should_register_enum {
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

    let default_mode = self.resolve_default_mode(schema, &mut variants, &inline_types, has_null_variant);

    variants = strip_common_affixes(variants);

    if self.context.config().sort_enum_variants() {
      variants = variants
        .into_iter()
        .sorted_by(|a, b| a.name.as_str().cmp(b.name.as_str()))
        .collect();
    }

    let methods = if self.context.config().no_helpers() {
      vec![]
    } else {
      self.method_generator.build_constructors(&variants, &inline_types, name)
    };

    let main_enum = self
      .discriminator_converter
      .try_upgrade_to_discriminated(name, schema, &variants, methods.clone())
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

  fn resolve_default_mode(
    &self,
    schema: &ObjectSchema,
    variants: &mut [VariantDef],
    inline_types: &[RustType],
    has_null_variant: bool,
  ) -> EnumDefault {
    let default = schema.default.as_ref().filter(|v| !v.is_null());
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

    for variant in variants.iter_mut() {
      let Some(type_ref) = variant.single_wrapped_type() else {
        continue;
      };
      if type_ref.is_array {
        continue;
      }

      let enum_variants = self.method_generator.resolve_enum_value_defs(type_ref, inline_types);
      if !enum_variants.is_empty() {
        if let Some(inner) = enum_variants
          .iter()
          .find(|v| variant_matches_value(v, &normalized.rename_value))
        {
          let inner_variant = Some(inner.name.clone());
          variant.default = true;
          return Some(EnumDefaultValue {
            value: value.clone(),
            inner_variant,
          });
        }
        continue;
      }

      let accepts = match (&type_ref.base_type, value) {
        (RustPrimitive::String, serde_json::Value::String(_)) | (RustPrimitive::Bool, serde_json::Value::Bool(_)) => {
          true
        }
        (primitive, serde_json::Value::Number(_)) if primitive.is_float() => true,
        (primitive, serde_json::Value::Number(n)) if primitive.is_integer() => n.is_i64() || n.is_u64(),
        _ => false,
      };
      if accepts {
        variant.default = true;
        return Some(EnumDefaultValue {
          value: value.clone(),
          inner_variant: None,
        });
      }
    }

    None
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

use std::{collections::BTreeSet, rc::Rc};

use anyhow::Context;
use oas3::spec::{ObjectOrReference, ObjectSchema};

use super::{
  ConversionOutput,
  methods::MethodGenerator,
  relaxed_enum::RelaxedEnumBuilder,
  union_types::{CollisionStrategy, UnionKind, UnionVariantSpec, variants_to_cache_key},
  value_enums::ValueEnumBuilder,
  variants::VariantBuilder,
};
use crate::{
  generator::{
    ast::{Documentation, EnumVariantToken, RustType},
    converter::{ConverterContext, discriminator::DiscriminatorConverter},
    naming::{identifiers::ensure_unique, inference::strip_common_affixes},
  },
  utils::{SchemaExt, extract_schema_ref_name},
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
    Self {
      context,
      value_enum_builder: ValueEnumBuilder::new(case_insensitive),
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

    self.value_enum_builder.build_enum_from_variants(
      name,
      variants,
      strategy,
      Documentation::from_optional(schema.description.as_ref()),
    )
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
  pub(crate) fn convert_union(
    &self,
    name: &str,
    schema: &ObjectSchema,
    kind: UnionKind,
  ) -> anyhow::Result<ConversionOutput<RustType>> {
    if kind == UnionKind::AnyOf
      && let Some(output) = self.relaxed_enum_builder.try_build_relaxed_enum(name, schema)
    {
      return Ok(output);
    }

    let output = self.collect_union_variants(name, schema, kind)?;

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
  fn collect_union_variants(
    &self,
    name: &str,
    schema: &ObjectSchema,
    kind: UnionKind,
  ) -> anyhow::Result<ConversionOutput<RustType>> {
    let variants_src = match kind {
      UnionKind::OneOf => &schema.one_of,
      UnionKind::AnyOf => &schema.any_of,
    };

    let variant_specs = self.collect_union_variant_specs(variants_src)?;

    let (mut variants, inline_types) = itertools::process_results(
      variant_specs.into_iter().map(|spec| {
        let output = self.variant_builder.build_variant(name, &spec)?;
        anyhow::Ok((output.result, output.inline_types))
      }),
      |iter| iter.unzip::<_, _, Vec<_>, Vec<_>>(),
    )?;
    let inline_types = inline_types.into_iter().flatten().collect::<Vec<_>>();

    variants = strip_common_affixes(variants);

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
          .schema(schema)
          .variants(variants)
          .methods(methods)
          .call()
      });

    Ok(ConversionOutput::with_inline_types(main_enum, inline_types))
  }

  /// Extracts variant specifications from raw union branch references.
  ///
  /// For each branch, resolves the schema reference, infers a variant name
  /// (from `$ref` path, schema `title`, or positional fallback), and ensures
  /// uniqueness across all variants. Null schemas are skipped as they represent
  /// nullable wrappers rather than distinct variants.
  fn collect_union_variant_specs(
    &self,
    variants_src: &[ObjectOrReference<ObjectSchema>],
  ) -> anyhow::Result<Vec<UnionVariantSpec>> {
    let mut specs = vec![];
    let mut seen_names = BTreeSet::new();

    for (i, variant_ref) in variants_src.iter().enumerate() {
      let resolved = variant_ref
        .resolve(self.context.graph().spec())
        .context(format!("Schema resolution failed for union variant {i}"))?;

      if resolved.is_null() {
        continue;
      }

      let ref_name = extract_schema_ref_name(variant_ref).or_else(|| {
        if resolved.all_of.len() == 1 {
          extract_schema_ref_name(&resolved.all_of[0])
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

    Ok(specs)
  }
}

use std::{collections::BTreeSet, sync::Arc};

use anyhow::Context;
use oas3::spec::{ObjectOrReference, ObjectSchema};

use super::{
  CodegenConfig, ConversionOutput,
  cache::SharedSchemaCache,
  common::SchemaExt,
  methods::MethodGenerator,
  relaxed_enum::RelaxedEnumBuilder,
  union_types::{CollisionStrategy, EnumValueEntry, UnionKind, UnionVariantSpec},
  value_enums::ValueEnumBuilder,
  variants::VariantBuilder,
};
use crate::generator::{
  ast::{Documentation, EnumVariantToken, RustType},
  converter::discriminator::DiscriminatorConverter,
  naming::{
    identifiers::ensure_unique,
    inference::{InferenceExt, strip_common_affixes},
  },
  schema_registry::{RefCollector, SchemaRegistry},
};

#[derive(Clone, Debug)]
pub(crate) struct EnumConverter {
  value_enum_builder: ValueEnumBuilder,
  preserve_case_variants: bool,
}

impl EnumConverter {
  pub(crate) fn new(config: &CodegenConfig) -> Self {
    Self {
      value_enum_builder: ValueEnumBuilder::new(config.case_insensitive_enums()),
      preserve_case_variants: config.preserve_case_variants(),
    }
  }

  pub(crate) fn convert_value_enum(
    &self,
    name: &str,
    schema: &ObjectSchema,
    cache: Option<&mut SharedSchemaCache>,
  ) -> RustType {
    let strategy = if self.preserve_case_variants {
      CollisionStrategy::Preserve
    } else {
      CollisionStrategy::Deduplicate
    };

    let entries: Vec<EnumValueEntry> = schema
      .enum_values
      .iter()
      .cloned()
      .map(|value| EnumValueEntry {
        value,
        docs: Documentation::default(),
        deprecated: false,
      })
      .collect();

    let enum_def = self.value_enum_builder.build_enum_from_values(
      name,
      &entries,
      strategy,
      Documentation::from_optional(schema.description.as_ref()),
    );

    if let (Some(c), RustType::Enum(e)) = (cache, &enum_def) {
      c.mark_name_used(e.name.to_string());
    }

    enum_def
  }
}

#[derive(Clone, Debug)]
pub(crate) struct UnionConverter {
  graph: Arc<SchemaRegistry>,
  variant_builder: VariantBuilder,
  relaxed_enum_builder: RelaxedEnumBuilder,
  method_generator: MethodGenerator,
  no_helpers: bool,
}

impl UnionConverter {
  pub(crate) fn new(graph: &Arc<SchemaRegistry>, config: &CodegenConfig) -> Self {
    let variant_builder = VariantBuilder::new(graph, config);
    let relaxed_enum_builder = RelaxedEnumBuilder::new(graph, config.case_insensitive_enums(), config.no_helpers());
    let method_generator = MethodGenerator::new(graph, config);

    Self {
      graph: graph.clone(),
      variant_builder,
      relaxed_enum_builder,
      method_generator,
      no_helpers: config.no_helpers(),
    }
  }

  pub(crate) fn convert_union(
    &self,
    name: &str,
    schema: &ObjectSchema,
    kind: UnionKind,
    mut cache: Option<&mut SharedSchemaCache>,
  ) -> anyhow::Result<ConversionOutput<RustType>> {
    if kind == UnionKind::AnyOf
      && let Some(output) = self
        .relaxed_enum_builder
        .try_build_relaxed_enum(name, schema, cache.as_deref_mut())
    {
      return Ok(output);
    }

    let output = self.collect_union_variants(name, schema, kind, cache.as_deref_mut())?;

    if let Some(c) = cache
      && let Some(values) = schema.extract_enum_values()
      && let RustType::Enum(e) = &output.result
    {
      c.register_enum(values, e.name.to_string());
    }

    Ok(output)
  }

  fn collect_union_variants(
    &self,
    name: &str,
    schema: &ObjectSchema,
    kind: UnionKind,
    mut cache: Option<&mut SharedSchemaCache>,
  ) -> anyhow::Result<ConversionOutput<RustType>> {
    let variants_src = match kind {
      UnionKind::OneOf => &schema.one_of,
      UnionKind::AnyOf => &schema.any_of,
    };

    let (mut variants, inline_types) = self.collect_union_variant_specs(variants_src)?.into_iter().try_fold(
      (vec![], vec![]),
      |(mut variants, mut inline_types), spec| {
        let output = self.variant_builder.build_variant(name, &spec, cache.as_deref_mut())?;
        variants.push(output.result);
        inline_types.extend(output.inline_types);
        anyhow::Ok((variants, inline_types))
      },
    )?;

    strip_common_affixes(&mut variants);

    let methods = if self.no_helpers {
      vec![]
    } else {
      self
        .method_generator
        .build_constructors(&variants, &inline_types, name, cache)
    };

    let main_enum = if let Some(discriminated) =
      DiscriminatorConverter::try_from_variants(name, schema, &variants, methods.clone())
    {
      discriminated
    } else {
      RustType::untagged_enum()
        .name(name)
        .schema(schema)
        .variants(variants)
        .methods(methods)
        .call()
    };

    Ok(ConversionOutput::with_inline_types(main_enum, inline_types))
  }

  fn collect_union_variant_specs(
    &self,
    variants_src: &[ObjectOrReference<ObjectSchema>],
  ) -> anyhow::Result<Vec<UnionVariantSpec>> {
    let mut specs = vec![];
    let mut seen_names = BTreeSet::new();

    for (i, variant_ref) in variants_src.iter().enumerate() {
      let resolved = variant_ref
        .resolve(self.graph.spec())
        .context(format!("Schema resolution failed for union variant {i}"))?;

      if resolved.is_null() {
        continue;
      }

      let ref_name = RefCollector::parse_schema_ref(variant_ref).or_else(|| {
        if resolved.all_of.len() == 1 {
          RefCollector::parse_schema_ref(&resolved.all_of[0])
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

use std::{collections::BTreeSet, rc::Rc};

use anyhow::Context;
use oas3::spec::{ObjectOrReference, ObjectSchema};

use super::{
  ConversionOutput,
  methods::MethodGenerator,
  relaxed_enum::RelaxedEnumBuilder,
  union_types::{CollisionStrategy, EnumValueEntry, UnionKind, UnionVariantSpec},
  value_enums::ValueEnumBuilder,
  variants::VariantBuilder,
};
use crate::{
  generator::{
    ast::{Documentation, EnumVariantToken, RustType},
    converter::{ConverterContext, discriminator::DiscriminatorConverter},
    naming::{identifiers::ensure_unique, inference::strip_common_affixes},
    schema_registry::RefCollector,
  },
  utils::SchemaExt,
};

#[derive(Clone, Debug)]
pub(crate) struct EnumConverter {
  context: Rc<ConverterContext>,
  value_enum_builder: ValueEnumBuilder,
}

impl EnumConverter {
  pub(crate) fn new(context: Rc<ConverterContext>) -> Self {
    let case_insensitive = context.config().case_insensitive_enums();
    Self {
      context,
      value_enum_builder: ValueEnumBuilder::new(case_insensitive),
    }
  }

  pub(crate) fn convert_value_enum(&self, name: &str, schema: &ObjectSchema) -> RustType {
    let strategy = if self.context.config().preserve_case_variants() {
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

    self.value_enum_builder.build_enum_from_values(
      name,
      &entries,
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
}

impl UnionConverter {
  pub(crate) fn new(context: Rc<ConverterContext>) -> Self {
    let variant_builder = VariantBuilder::new(context.clone());
    let relaxed_enum_builder = RelaxedEnumBuilder::new(context.clone());
    let method_generator = MethodGenerator::new(context.clone());

    Self {
      context,
      variant_builder,
      relaxed_enum_builder,
      method_generator,
    }
  }

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

    if let Some(values) = schema.extract_enum_values()
      && let RustType::Enum(e) = &output.result
    {
      self
        .context
        .cache
        .borrow_mut()
        .register_enum(values, e.name.to_string());
    }

    Ok(output)
  }

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

    let (mut variants, inline_types): (Vec<_>, Vec<_>) = itertools::process_results(
      variant_specs.into_iter().map(|spec| {
        let output = self.variant_builder.build_variant(name, &spec)?;
        anyhow::Ok((output.result, output.inline_types))
      }),
      |iter| iter.unzip(),
    )?;
    let inline_types: Vec<_> = inline_types.into_iter().flatten().collect();

    variants = strip_common_affixes(variants);

    let methods = if self.context.config().no_helpers() {
      vec![]
    } else {
      self.method_generator.build_constructors(&variants, &inline_types, name)
    };

    let main_enum = if let Some(discriminated) =
      DiscriminatorConverter::try_upgrade_to_discriminated(name, schema, &variants, methods.clone())
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
        .resolve(self.context.graph().spec())
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

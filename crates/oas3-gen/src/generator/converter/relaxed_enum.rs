use std::{
  collections::{BTreeSet, HashSet},
  rc::Rc,
  string::ToString,
};

use oas3::spec::ObjectSchema;
use serde_json::Value;

use super::{
  ConversionOutput,
  union_types::{CollisionStrategy, EnumValueEntry},
  value_enums::ValueEnumBuilder,
};
use crate::{
  generator::{
    ast::{
      Documentation, EnumDef, EnumMethod, EnumMethodKind, EnumToken, EnumVariantToken, RustPrimitive, RustType,
      SerdeAttribute, TypeRef, VariantContent, VariantDef,
    },
    converter::ConverterContext,
    naming::{
      constants::{KNOWN_ENUM_VARIANT, OTHER_ENUM_VARIANT},
      identifiers::{ensure_unique, to_rust_type_name},
      inference::{NormalizedVariant, derive_method_names},
    },
  },
  utils::SchemaExt,
};

#[derive(Clone, Debug)]
pub(crate) struct RelaxedEnumBuilder {
  context: Rc<ConverterContext>,
  value_enum_builder: ValueEnumBuilder,
}

impl RelaxedEnumBuilder {
  pub(crate) fn new(context: Rc<ConverterContext>) -> Self {
    let case_insensitive = context.config().case_insensitive_enums();
    Self {
      context,
      value_enum_builder: ValueEnumBuilder::new(case_insensitive),
    }
  }

  pub(crate) fn try_build_relaxed_enum(&self, name: &str, schema: &ObjectSchema) -> Option<ConversionOutput<RustType>> {
    let known_values = self.collect_known_values(schema);
    if known_values.is_empty() {
      return None;
    }

    Some(self.build_relaxed_enum_types(name, schema, &known_values))
  }

  fn collect_known_values(&self, schema: &ObjectSchema) -> Vec<EnumValueEntry> {
    let mut seen_values = HashSet::new();
    let mut known_values = vec![];
    let mut has_freeform = false;

    for variant in &schema.any_of {
      let Ok(resolved) = variant.resolve(self.context.graph().spec()) else {
        continue;
      };

      if resolved.is_freeform_string() {
        has_freeform = true;
      }

      let docs = Documentation::from_optional(resolved.description.as_ref());
      let deprecated = resolved.deprecated.unwrap_or(false);

      if let Some(const_entry) = Self::extract_const_value(&resolved, &docs, deprecated, &mut seen_values) {
        known_values.push(const_entry);
        continue;
      }

      if resolved.is_string() {
        Self::extract_enum_values(&resolved, &docs, deprecated, &mut seen_values, &mut known_values);
      }
    }

    if has_freeform { known_values } else { vec![] }
  }

  fn extract_const_value(
    schema: &ObjectSchema,
    docs: &Documentation,
    deprecated: bool,
    seen_values: &mut HashSet<String>,
  ) -> Option<EnumValueEntry> {
    let const_val = schema.const_value.as_ref()?.as_str()?;

    if seen_values.insert(const_val.to_string()) {
      Some(EnumValueEntry {
        value: Value::String(const_val.to_string()),
        docs: docs.clone(),
        deprecated,
      })
    } else {
      None
    }
  }

  fn extract_enum_values(
    schema: &ObjectSchema,
    docs: &Documentation,
    deprecated: bool,
    seen_values: &mut HashSet<String>,
    known_values: &mut Vec<EnumValueEntry>,
  ) {
    for enum_value in &schema.enum_values {
      if let Some(str_val) = enum_value.as_str()
        && seen_values.insert(str_val.to_string())
      {
        known_values.push(EnumValueEntry {
          value: Value::String(str_val.to_string()),
          docs: docs.clone(),
          deprecated,
        });
      }
    }
  }

  fn build_relaxed_enum_types(
    &self,
    name: &str,
    schema: &ObjectSchema,
    known_values: &[EnumValueEntry],
  ) -> ConversionOutput<RustType> {
    let base_name = to_rust_type_name(name);

    let mut cache_key_values: Vec<String> = known_values
      .iter()
      .filter_map(|e| e.value.as_str().map(String::from))
      .collect();
    cache_key_values.sort();

    let (known_enum_name, inner_enum_type) = self.resolve_cached_known_enum(&base_name, known_values, cache_key_values);

    let methods = if self.context.config().no_helpers() {
      vec![]
    } else {
      Self::build_known_value_constructors(&base_name, &known_enum_name, known_values)
    };

    let outer_enum = Self::build_wrapper_enum(&base_name, &known_enum_name, schema, methods);
    let inline_types = inner_enum_type.into_iter().collect();

    ConversionOutput::with_inline_types(outer_enum, inline_types)
  }

  fn resolve_cached_known_enum(
    &self,
    base_name: &str,
    known_values: &[EnumValueEntry],
    cache_key: Vec<String>,
  ) -> (String, Option<RustType>) {
    let cached_state = {
      let cache = self.context.cache.borrow();
      cache
        .get_enum_name(&cache_key)
        .map(|name| (name.clone(), cache.is_enum_generated(&cache_key)))
    };

    if let Some((name, true)) = cached_state {
      return (name, None);
    }

    let name = cached_state.map_or_else(|| format!("{base_name}Known"), |(name, _)| name);

    let def = self.value_enum_builder.build_enum_from_values(
      name.as_str(),
      known_values,
      CollisionStrategy::Preserve,
      Documentation::from_lines(["Known values for the string enum."]),
    );

    {
      let mut cache = self.context.cache.borrow_mut();
      cache.register_enum(cache_key, name.clone());
      cache.mark_name_used(name.clone());
    }

    (name, Some(def))
  }

  fn build_known_value_constructors(
    wrapper_enum_name: &str,
    known_type_name: &str,
    entries: &[EnumValueEntry],
  ) -> Vec<EnumMethod> {
    let known_type = EnumToken::new(known_type_name);

    let variant_names: Vec<EnumVariantToken> = entries
      .iter()
      .filter_map(|entry| {
        NormalizedVariant::try_from(&entry.value)
          .ok()
          .map(|n| EnumVariantToken::new(n.name))
      })
      .collect();

    let variant_name_strings: Vec<String> = variant_names.iter().map(ToString::to_string).collect();
    let method_names = derive_method_names(wrapper_enum_name, &variant_name_strings);

    let mut seen = BTreeSet::new();
    variant_names
      .into_iter()
      .zip(method_names)
      .zip(entries.iter())
      .map(|((variant, base_name), entry)| {
        let method_name = ensure_unique(&base_name, &seen);
        seen.insert(method_name.clone());
        EnumMethod::new(
          method_name,
          EnumMethodKind::KnownValueConstructor {
            known_type: known_type.clone(),
            known_variant: variant,
          },
          entry.docs.clone(),
        )
      })
      .collect()
  }

  fn build_wrapper_enum(
    name: &str,
    known_type_name: &str,
    schema: &ObjectSchema,
    methods: Vec<EnumMethod>,
  ) -> RustType {
    let variants = vec![
      VariantDef::builder()
        .name(EnumVariantToken::new(KNOWN_ENUM_VARIANT))
        .content(VariantContent::Tuple(vec![TypeRef::new(known_type_name)]))
        .build(),
      VariantDef::builder()
        .name(EnumVariantToken::new(OTHER_ENUM_VARIANT))
        .content(VariantContent::Tuple(vec![TypeRef::new(RustPrimitive::String)]))
        .build(),
    ];

    RustType::Enum(
      EnumDef::builder()
        .name(EnumToken::new(name))
        .docs(Documentation::from_optional(schema.description.as_ref()))
        .variants(variants)
        .serde_attrs(vec![SerdeAttribute::Untagged])
        .methods(methods)
        .generate_display(true)
        .build(),
    )
  }
}

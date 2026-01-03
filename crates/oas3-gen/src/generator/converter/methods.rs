use std::{
  collections::{BTreeMap, BTreeSet},
  rc::Rc,
};

use super::{SchemaExt, struct_summaries::StructSummary, structs::StructConverter};
use crate::generator::{
  ast::{EnumMethod, EnumMethodKind, EnumVariantToken, MethodNameToken, RustType, TypeRef, VariantDef},
  converter::ConverterContext,
  naming::{
    identifiers::{ensure_unique, to_rust_type_name},
    inference::derive_method_names,
  },
};

#[derive(Clone, Debug)]
pub(crate) struct MethodGenerator {
  context: Rc<ConverterContext>,
  struct_converter: StructConverter,
}

impl MethodGenerator {
  pub(crate) fn new(context: Rc<ConverterContext>) -> Self {
    let struct_converter = StructConverter::new(context.clone());
    Self {
      context,
      struct_converter,
    }
  }

  pub(crate) fn build_constructors(
    &self,
    variants: &[VariantDef],
    inline_types: &[RustType],
    enum_name: &str,
  ) -> Vec<EnumMethod> {
    let enum_name = to_rust_type_name(enum_name);

    let mut summary_cache: BTreeMap<String, StructSummary> = inline_types
      .iter()
      .filter_map(|t| match t {
        RustType::Struct(s) => Some((s.name.to_string(), StructSummary::from(s))),
        _ => None,
      })
      .collect();

    let eligible = self.collect_eligible_variants(variants, &mut summary_cache);

    if eligible.is_empty() {
      return vec![];
    }

    Self::build_methods_from_eligible(&enum_name, &eligible, variants)
  }

  fn collect_eligible_variants(
    &self,
    variants: &[VariantDef],
    summary_cache: &mut BTreeMap<String, StructSummary>,
  ) -> Vec<(EnumVariantToken, EnumMethodKind)> {
    let mut eligible = vec![];

    for variant in variants {
      let Some(type_ref) = variant.single_wrapped_type() else {
        continue;
      };

      let Some(summary) = self.resolve_struct_summary(type_ref, summary_cache) else {
        continue;
      };

      if let Some(method_kind) = Self::constructor_kind_for(type_ref, &variant.name, &summary) {
        eligible.push((variant.name.clone(), method_kind));
      }
    }

    eligible
  }

  fn build_methods_from_eligible(
    enum_name: &str,
    eligible: &[(EnumVariantToken, EnumMethodKind)],
    variants: &[VariantDef],
  ) -> Vec<EnumMethod> {
    let variant_names: Vec<String> = eligible.iter().map(|(name, _)| name.to_string()).collect();
    let method_names = derive_method_names(enum_name, &variant_names);

    let mut seen = BTreeSet::new();
    eligible
      .iter()
      .zip(method_names)
      .map(|((variant_name, kind), base_name)| {
        let method_name = ensure_unique(&base_name, &seen);
        seen.insert(method_name.clone());
        let docs = variants
          .iter()
          .find(|v| v.name == *variant_name)
          .map(|v| v.docs.clone())
          .unwrap_or_default();

        EnumMethod::new(MethodNameToken::from_raw(&method_name), kind.clone(), docs)
      })
      .collect()
  }

  fn constructor_kind_for(
    type_ref: &TypeRef,
    variant_name: &EnumVariantToken,
    summary: &StructSummary,
  ) -> Option<EnumMethodKind> {
    if !summary.has_default || type_ref.is_array {
      return None;
    }

    match summary.required_fields.len() {
      0 => {
        if summary.user_fields.len() == 1 {
          let (ref name, ref rust_type) = summary.user_fields[0];
          Some(EnumMethodKind::ParameterizedConstructor {
            variant_name: variant_name.clone(),
            wrapped_type: type_ref.clone(),
            param_name: name.to_string(),
            param_type: rust_type.clone(),
          })
        } else {
          Some(EnumMethodKind::SimpleConstructor {
            variant_name: variant_name.clone(),
            wrapped_type: type_ref.clone(),
          })
        }
      }
      1 => {
        let (ref name, ref rust_type) = summary.required_fields[0];
        Some(EnumMethodKind::ParameterizedConstructor {
          variant_name: variant_name.clone(),
          wrapped_type: type_ref.clone(),
          param_name: name.to_string(),
          param_type: rust_type.clone(),
        })
      }
      _ => None,
    }
  }

  fn resolve_struct_summary(
    &self,
    type_ref: &TypeRef,
    summary_cache: &mut BTreeMap<String, StructSummary>,
  ) -> Option<StructSummary> {
    let base_name = type_ref.unboxed_base_type_name();

    if let Some(summary) = summary_cache.get(&base_name) {
      return Some(summary.clone());
    }

    {
      let cache = self.context.cache.borrow();
      if let Some(summary) = cache.get_struct_summary(&base_name) {
        summary_cache.insert(base_name.clone(), summary.clone());
        return Some(summary.clone());
      }
    }

    let schema = self.context.graph().get(&base_name)?;
    if !schema.is_object() && schema.properties.is_empty() {
      return None;
    }

    let struct_result = self.struct_converter.convert_struct(&base_name, schema, None).ok()?;

    if let RustType::Struct(s) = struct_result.result {
      let summary = StructSummary::from(&s);
      summary_cache.insert(base_name, summary.clone());
      Some(summary)
    } else {
      None
    }
  }
}

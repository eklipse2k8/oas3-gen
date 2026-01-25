use std::{collections::BTreeSet, rc::Rc};

use itertools::{Itertools, izip};
use oas3::spec::ObjectSchema;

use super::{
  ConversionOutput, ConverterContext,
  union_types::{CollisionStrategy, EnumValueEntry, entries_to_cache_key},
  value_enums::ValueEnumBuilder,
};
use crate::{
  generator::{
    ast::{
      Documentation, EnumDef, EnumMethod, EnumMethodKind, EnumToken, EnumVariantToken, RustPrimitive, RustType,
      SerdeAttribute, TypeRef, VariantContent, VariantDef,
    },
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
  /// Creates a new `RelaxedEnumBuilder` with the given converter context.
  ///
  /// Initializes the internal `ValueEnumBuilder` with case-sensitivity settings
  /// from the codegen configuration.
  pub(crate) fn new(context: Rc<ConverterContext>) -> Self {
    let case_insensitive = context.config().case_insensitive_enums();
    Self {
      context,
      value_enum_builder: ValueEnumBuilder::new(case_insensitive),
    }
  }

  /// Attempts to convert an OpenAPI `anyOf` schema into a relaxed enum type pair.
  ///
  /// Recognizes schemas containing both constrained string variants (with enum values)
  /// and a freeform string variant. Returns `None` if the schema does not match
  /// the relaxed enum pattern.
  ///
  /// On success, returns a wrapper enum with `Known(T)` and `Other(String)` variants,
  /// plus an inner enum `T` containing the known values.
  pub(crate) fn try_build_relaxed_enum(&self, name: &str, schema: &ObjectSchema) -> Option<ConversionOutput<RustType>> {
    let known_values = self.collect_known_values(schema);
    if known_values.is_empty() {
      return None;
    }

    Some(self.build_relaxed_enum_types(name, schema, &known_values))
  }

  /// Extracts known enum values from an `anyOf` schema if it contains a freeform string variant.
  ///
  /// Returns an empty vector if the schema lacks a freeform string variant,
  /// indicating it does not follow the relaxed enum pattern. Otherwise, returns
  /// all constrained enum values from the non-freeform variants.
  fn collect_known_values(&self, schema: &ObjectSchema) -> Vec<EnumValueEntry> {
    let spec = self.context.graph().spec();

    let has_freeform = schema
      .any_of
      .iter()
      .filter_map(|v| v.resolve(spec).ok())
      .any(|s| s.is_freeform_string());

    if !has_freeform {
      return vec![];
    }

    schema.extract_enum_entries(spec)
  }

  /// Constructs the relaxed enum type pair: an inner enum of known values and
  /// an outer wrapper enum.
  ///
  /// The inner enum (e.g., `FooKnown`) contains one variant per known string value.
  /// The outer enum (e.g., `Foo`) has two variants: `Known(FooKnown)` and `Other(String)`.
  /// Constructor methods are generated for each known value unless helpers are disabled.
  fn build_relaxed_enum_types(
    &self,
    name: &str,
    schema: &ObjectSchema,
    known_values: &[EnumValueEntry],
  ) -> ConversionOutput<RustType> {
    let base_name = to_rust_type_name(name);
    let cache_key = entries_to_cache_key(known_values);

    let (known_enum_name, inner_enum_type) = self.resolve_or_create_known_enum(&base_name, known_values, &cache_key);

    let methods = if self.context.config().no_helpers() {
      vec![]
    } else {
      Self::build_known_value_constructors(&base_name, &known_enum_name, known_values)
    };

    let outer_enum = Self::build_wrapper_enum(&base_name, &known_enum_name, schema, methods);
    ConversionOutput::with_inline_types(outer_enum, inner_enum_type.into_iter().collect())
  }

  /// Looks up or creates the inner known-values enum, deduplicating across schemas.
  ///
  /// Returns the enum type name and optionally the `RustType` definition. Returns
  /// `None` for the type if an identical enum was already generated, avoiding
  /// duplicate definitions in the output.
  fn resolve_or_create_known_enum(
    &self,
    base_name: &str,
    known_values: &[EnumValueEntry],
    cache_key: &[String],
  ) -> (String, Option<RustType>) {
    let cached_result = {
      let cache = self.context.cache.borrow();
      cache
        .get_enum_name(cache_key)
        .map(|name| (name.clone(), cache.is_enum_generated(cache_key)))
    };

    match cached_result {
      Some((name, true)) => (name, None),
      Some((name, false)) => self.create_known_enum(name, known_values, cache_key),
      None => self.create_known_enum(format!("{base_name}Known"), known_values, cache_key),
    }
  }

  /// Generates the known-values enum definition and registers it in the cache.
  ///
  /// Creates an enum with one variant per known string value, using the
  /// `ValueEnumBuilder` for variant construction. Registers both the enum-to-name
  /// mapping and marks the name as used in the shared cache.
  fn create_known_enum(
    &self,
    name: String,
    known_values: &[EnumValueEntry],
    cache_key: &[String],
  ) -> (String, Option<RustType>) {
    let def = self.value_enum_builder.build_enum_from_values(
      &name,
      known_values,
      CollisionStrategy::Preserve,
      Documentation::from_lines(["Known values for the string enum."]),
    );

    let mut cache = self.context.cache.borrow_mut();
    cache.register_enum(cache_key.to_vec(), name.clone());
    cache.mark_name_used(name.clone());

    (name, Some(def))
  }

  /// Generates constructor methods for each known value on the wrapper enum.
  ///
  /// For an enum `Status` with known values `["active", "pending"]`, produces
  /// methods like `fn active() -> Self` and `fn pending() -> Self` that wrap
  /// the inner known-values enum variant. Method names are derived by removing
  /// words shared with the enum name and ensuring uniqueness.
  fn build_known_value_constructors(
    wrapper_enum_name: &str,
    known_type_name: &str,
    entries: &[EnumValueEntry],
  ) -> Vec<EnumMethod> {
    let known_type = EnumToken::new(known_type_name);

    let variants = entries
      .iter()
      .filter_map(|e| {
        NormalizedVariant::try_from(&e.value)
          .ok()
          .map(|n| EnumVariantToken::new(n.name))
      })
      .collect_vec();

    let variant_strings = variants.iter().map(ToString::to_string).collect_vec();
    let method_names = derive_method_names(wrapper_enum_name, &variant_strings);

    let mut seen = BTreeSet::new();
    izip!(&variants, &method_names, entries)
      .map(|(variant, base_name, entry)| {
        let method_name = ensure_unique(base_name, &seen);
        seen.insert(method_name.clone());
        EnumMethod::new(
          method_name,
          EnumMethodKind::KnownValueConstructor {
            known_type: known_type.clone(),
            known_variant: variant.clone(),
          },
          entry.docs.clone(),
        )
      })
      .collect()
  }

  /// Constructs the wrapper enum with `Known` and `Other` variants.
  ///
  /// Creates an untagged serde enum where `Known` wraps the inner known-values
  /// enum and `Other` accepts arbitrary strings. Includes documentation from
  /// the schema and attaches any provided constructor methods.
  fn build_wrapper_enum(
    name: &str,
    known_type_name: &str,
    schema: &ObjectSchema,
    methods: Vec<EnumMethod>,
  ) -> RustType {
    RustType::Enum(
      EnumDef::builder()
        .name(EnumToken::new(name))
        .docs(Documentation::from_optional(schema.description.as_ref()))
        .variants(vec![
          VariantDef::builder()
            .name(EnumVariantToken::new(KNOWN_ENUM_VARIANT))
            .content(VariantContent::Tuple(vec![TypeRef::new(known_type_name)]))
            .build(),
          VariantDef::builder()
            .name(EnumVariantToken::new(OTHER_ENUM_VARIANT))
            .content(VariantContent::Tuple(vec![TypeRef::new(RustPrimitive::String)]))
            .build(),
        ])
        .serde_attrs(vec![SerdeAttribute::Untagged])
        .methods(methods)
        .generate_display(true)
        .build(),
    )
  }
}

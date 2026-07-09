use std::{
  collections::{BTreeMap, BTreeSet},
  rc::Rc,
};

use itertools::Itertools;

use super::structs::StructConverter;
use crate::{
  generator::{
    ast::{
      BuilderField, BuilderNestedStruct, Documentation, EnumMethod, EnumMethodKind, EnumToken, EnumVariantToken,
      FieldDef, FieldNameToken, MethodKind, MethodNameToken, RustType, StructDef, StructMethod, TypeRef,
      VariantContent, VariantDef,
    },
    converter::ConverterContext,
    naming::{
      identifiers::{ensure_unique, to_rust_type_name},
      inference::derive_method_names,
    },
  },
  utils::SchemaExt,
};

/// Generates helper constructor methods for enum variants and builder methods for request structs.
///
/// For enum types, analyzes variant wrapped types to determine which variants can
/// have convenient constructors (e.g., `fn user(u: User) -> Self`). For request
/// structs, builds a `new()` method that accepts required fields and nested
/// parameter structs.
#[derive(Clone, Debug)]
pub(crate) struct MethodGenerator {
  context: Rc<ConverterContext>,
  struct_converter: StructConverter,
}

impl MethodGenerator {
  /// Creates a new method generator with access to the converter context.
  pub(crate) fn new(context: Rc<ConverterContext>) -> Self {
    let struct_converter = StructConverter::new(context.clone());
    Self {
      context,
      struct_converter,
    }
  }

  /// Generates constructor methods for enum variants wrapping struct types.
  ///
  /// For each variant containing a single wrapped type, inspects the struct
  /// definition to determine constructor eligibility. Structs with all-optional
  /// fields get zero-argument constructors; structs with exactly one required
  /// field get single-argument constructors. Method names are derived by
  /// stripping common prefixes from variant names.
  pub(crate) fn build_constructors(
    &self,
    variants: &[VariantDef],
    inline_types: &[RustType],
    enum_name: &str,
  ) -> Vec<EnumMethod> {
    let enum_name = to_rust_type_name(enum_name);

    let mut struct_cache = inline_types
      .iter()
      .filter_map(|t| match t {
        RustType::Struct(s) => Some((s.name.to_string(), s.clone())),
        _ => None,
      })
      .collect::<BTreeMap<String, StructDef>>();

    let eligible = self.collect_eligible_variants(variants, &mut struct_cache);
    let enum_value_variants = self.collect_enum_value_variants(variants, inline_types);

    if eligible.is_empty() && enum_value_variants.is_empty() {
      return vec![];
    }

    let mut seen = BTreeSet::new();
    let mut methods = Self::build_methods_from_eligible(&enum_name, &eligible, variants, &mut seen);

    for group in &enum_value_variants {
      methods.extend(Self::build_wrapped_enum_value_constructors(
        &enum_name, group, &mut seen,
      ));
    }

    methods
  }

  /// Filters variants to those eligible for constructor generation.
  ///
  /// A variant is eligible if it wraps a single struct type that has a
  /// default implementation (all optional fields or all fields with defaults).
  /// Array-typed variants are excluded.
  fn collect_eligible_variants(
    &self,
    variants: &[VariantDef],
    struct_cache: &mut BTreeMap<String, StructDef>,
  ) -> Vec<(EnumVariantToken, EnumMethodKind)> {
    let mut eligible = vec![];

    for variant in variants {
      let Some(type_ref) = variant.single_wrapped_type() else {
        continue;
      };

      let Some(struct_def) = self.resolve_struct_def(type_ref, struct_cache) else {
        continue;
      };

      if let Some(method_kind) = Self::constructor_kind_for(type_ref, &variant.name, &struct_def) {
        eligible.push((variant.name.clone(), method_kind));
      }
    }

    eligible
  }

  /// Collects union variants that wrap a value enum, along with that enum's values.
  ///
  /// A variant wrapping a value enum (e.g. `Preset(ImageSizePreset)`) can expose one
  /// convenience constructor per enum value, mirroring the `Known(T)` variant of a
  /// relaxed enum. Array-typed variants and variants whose wrapped type is not a
  /// value enum are skipped.
  fn collect_enum_value_variants(
    &self,
    variants: &[VariantDef],
    inline_types: &[RustType],
  ) -> Vec<WrappedEnumConstructors> {
    variants
      .iter()
      .filter_map(|variant| {
        let type_ref = variant.single_wrapped_type()?;
        if type_ref.is_array {
          return None;
        }

        let values = self.resolve_enum_value_defs(type_ref, inline_types);
        if values.is_empty() {
          return None;
        }

        Some(WrappedEnumConstructors {
          wrapper_variant: variant.name.clone(),
          enum_type: EnumToken::new(type_ref.unboxed_base_type_name()),
          values,
        })
      })
      .collect()
  }

  /// Resolves the unit variant definitions of a wrapped value enum.
  ///
  /// Prefers an inline enum definition (exact generated variants), falling back to
  /// the named schema in the graph. Only unit variants are returned, so nested
  /// union enums with tuple variants do not produce spurious constructors.
  fn resolve_enum_value_defs(&self, type_ref: &TypeRef, inline_types: &[RustType]) -> Vec<VariantDef> {
    let base_name = type_ref.unboxed_base_type_name();

    let variants = inline_types
      .iter()
      .find_map(|t| match t {
        RustType::Enum(e) if e.name.as_str() == base_name => Some(e.variants.clone()),
        _ => None,
      })
      .or_else(|| {
        let spec = self.context.graph().spec();
        self
          .context
          .graph()
          .get(&base_name)
          .filter(|schema| schema.has_enum_values())
          .map(|schema| schema.extract_enum_entries(spec))
      })
      .unwrap_or_default();

    variants
      .into_iter()
      .filter(|v| matches!(v.content, VariantContent::Unit))
      .collect()
  }

  /// Builds one constructor per value of a wrapped value enum.
  ///
  /// For a variant `Preset(ImageSizePreset)`, produces methods like
  /// `fn auto_2k() -> Self { Self::Preset(ImageSizePreset::Auto2k) }`. Method names
  /// are derived by stripping words shared with the enum name and deduplicated
  /// against the shared `seen` set so they never collide with struct constructors.
  fn build_wrapped_enum_value_constructors(
    enum_name: &str,
    group: &WrappedEnumConstructors,
    seen: &mut BTreeSet<String>,
  ) -> Vec<EnumMethod> {
    let value_names = group.values.iter().map(|v| v.name.to_string()).collect::<Vec<String>>();
    let method_names = derive_method_names(enum_name, &value_names);

    group
      .values
      .iter()
      .zip_eq(method_names)
      .map(|(value, base_name)| {
        let method_name = ensure_unique(&base_name, seen);
        seen.insert(method_name.clone());
        EnumMethod::new(
          MethodNameToken::from_raw(&method_name),
          EnumMethodKind::KnownValueConstructor {
            wrapper_variant: group.wrapper_variant.clone(),
            known_type: group.enum_type.clone(),
            known_variant: value.name.clone(),
          },
          value.docs.clone(),
        )
      })
      .collect()
  }

  /// Constructs method definitions from eligible variant specifications.
  ///
  /// Derives method names by stripping common prefixes shared with `enum_name`,
  /// then ensures uniqueness by appending numeric suffixes if needed. Attaches
  /// documentation from the original variant definitions.
  fn build_methods_from_eligible(
    enum_name: &str,
    eligible: &[(EnumVariantToken, EnumMethodKind)],
    variants: &[VariantDef],
    seen: &mut BTreeSet<String>,
  ) -> Vec<EnumMethod> {
    let variant_names = eligible
      .iter()
      .map(|(name, _)| name.to_string())
      .collect::<Vec<String>>();
    let method_names = derive_method_names(enum_name, &variant_names);

    eligible
      .iter()
      .zip_eq(method_names)
      .map(|((variant_name, kind), base_name)| {
        let method_name = ensure_unique(&base_name, seen);
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

  /// Determines the appropriate constructor kind for a variant's wrapped type.
  ///
  /// Returns `None` for types without defaults or array types. For struct types:
  /// - Zero required fields + one optional field → `ParameterizedConstructor`
  /// - Zero required fields + multiple optional fields → `SimpleConstructor`
  /// - One required field → `ParameterizedConstructor`
  /// - Multiple required fields → `None`
  fn constructor_kind_for(
    type_ref: &TypeRef,
    variant_name: &EnumVariantToken,
    struct_def: &StructDef,
  ) -> Option<EnumMethodKind> {
    if !struct_def.has_default() || type_ref.is_array {
      return None;
    }

    let required_fields = struct_def.required_fields().collect::<Vec<_>>();
    let user_fields = struct_def.user_fields().collect::<Vec<_>>();

    match required_fields.len() {
      0 => {
        if user_fields.len() == 1 {
          let field = &user_fields[0];
          Some(EnumMethodKind::ParameterizedConstructor {
            variant_name: variant_name.clone(),
            wrapped_type: type_ref.clone(),
            param_name: field.name.to_string(),
            param_type: field.rust_type.clone(),
          })
        } else {
          Some(EnumMethodKind::SimpleConstructor {
            variant_name: variant_name.clone(),
            wrapped_type: type_ref.clone(),
          })
        }
      }
      1 => {
        let field = &required_fields[0];
        Some(EnumMethodKind::ParameterizedConstructor {
          variant_name: variant_name.clone(),
          wrapped_type: type_ref.clone(),
          param_name: field.name.to_string(),
          param_type: field.rust_type.clone(),
        })
      }
      _ => None,
    }
  }

  /// Retrieves the struct definition for a type reference.
  ///
  /// Checks the local cache first, then the shared schema cache, and finally
  /// attempts on-demand conversion from the schema registry. Returns `None`
  /// if the type is not a struct or cannot be found.
  fn resolve_struct_def(
    &self,
    type_ref: &TypeRef,
    struct_cache: &mut BTreeMap<String, StructDef>,
  ) -> Option<StructDef> {
    let base_name = type_ref.unboxed_base_type_name();

    if let Some(struct_def) = struct_cache.get(&base_name) {
      return Some(struct_def.clone());
    }

    if let Some(struct_def) = self.context.cache().get_struct_def(&base_name) {
      struct_cache.insert(base_name.clone(), struct_def.clone());
      return Some(struct_def.clone());
    }

    let schema = self.context.graph().get(&base_name)?;
    if !schema.is_object() && schema.properties.is_empty() {
      return None;
    }

    let struct_result = self.struct_converter.convert_struct(&base_name, schema, None).ok()?;

    if let RustType::Struct(s) = struct_result.result {
      struct_cache.insert(base_name, s.clone());
      Some(s)
    } else {
      None
    }
  }

  /// Generates a `new()` builder method for request structs.
  ///
  /// Flattens nested parameter structs (path, query, header) into the method
  /// signature, allowing callers to pass individual parameters rather than
  /// constructing nested types manually. Returns `None` if there are no
  /// fields to include in the builder.
  pub(crate) fn build_builder_method(nested_structs: &[StructDef], main_fields: &[FieldDef]) -> Option<StructMethod> {
    let (fields, nested): BuilderFieldTuple = main_fields
      .iter()
      .map(|field| Self::resolve_field_components(field, nested_structs))
      .unzip();

    let fields = fields.into_iter().flatten().collect::<Vec<_>>();
    let nested = nested.into_iter().flatten().collect::<Vec<_>>();

    if fields.is_empty() {
      return None;
    }

    Some(
      StructMethod::builder()
        .name(MethodNameToken::from_raw("new"))
        .docs(Documentation::from_lines([
          "Create a new request with the given parameters.",
        ]))
        .kind(MethodKind::Builder {
          fields,
          nested_structs: nested,
        })
        .build(),
    )
  }

  /// Expands a field into builder components, flattening nested structs.
  ///
  /// If the field's type matches a nested struct, extracts that struct's
  /// fields for inclusion in the builder method signature. Returns both
  /// the flattened fields and metadata about the nesting relationship
  /// for code generation.
  fn resolve_field_components(
    field: &FieldDef,
    nested_structs: &[StructDef],
  ) -> (Vec<BuilderField>, Option<BuilderNestedStruct>) {
    let type_name = field.rust_type.to_rust_type();

    let Some(nested) = nested_structs.iter().find(|s| s.name.to_string() == type_name) else {
      return (vec![BuilderField::from(field)], None);
    };

    let nested_info = BuilderNestedStruct::builder()
      .field_name(field.name.clone())
      .struct_name(nested.name.clone())
      .field_names(nested.fields.iter().map(|f| f.name.clone()).collect::<Vec<_>>())
      .build();

    let flattened_fields = nested
      .fields
      .iter()
      .map(|nested_field| BuilderField::from_nested(nested_field, &field.name))
      .collect::<Vec<_>>();

    (flattened_fields, Some(nested_info))
  }
}

type BuilderFieldTuple = (Vec<Vec<BuilderField>>, Vec<Option<BuilderNestedStruct>>);

/// A union variant that wraps a value enum, paired with that enum's unit values.
///
/// Used to generate one convenience constructor per enum value on the union type.
struct WrappedEnumConstructors {
  wrapper_variant: EnumVariantToken,
  enum_type: EnumToken,
  values: Vec<VariantDef>,
}

impl BuilderField {
  /// Creates a builder field from a nested struct's field with owner tracking.
  ///
  /// The `owner` parameter records which nested struct contains this field,
  /// enabling correct field assignment during code generation.
  pub(crate) fn from_nested(field: &FieldDef, owner: &FieldNameToken) -> Self {
    let mut builder_field = Self::from(field);
    builder_field.owner_field = Some(owner.clone());
    builder_field
  }
}

impl From<&FieldDef> for BuilderField {
  fn from(field: &FieldDef) -> Self {
    let type_ref = &field.rust_type;

    BuilderField::builder()
      .name(field.name.clone())
      .rust_type(if field.is_required() {
        type_ref.clone().unwrap_option()
      } else {
        type_ref.clone()
      })
      .build()
  }
}

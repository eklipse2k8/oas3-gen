use std::collections::HashMap;

use crate::generator::{
  ast::{
    DefaultAtom, DefaultVariant, EnumDef, EnumDefault, FieldDef, RustPrimitive, RustType, TypeRef, VariantContent,
  },
  converter::{unions::wrapped_default_payload, value_enums::variant_matches_value},
  naming::inference::NormalizedVariant,
};

/// Makes every generated struct's derived `Default` hold.
///
/// Inline enums and unions are shared across every schema with the same value set, so a
/// shared definition's `Default`, or its lack of one, comes from whichever schema was
/// converted first. This pass resolves each field's schema `default` to an explicit variant,
/// then derives `Default` on every enum a derived `Default` still falls back to.
pub(crate) struct FieldDefaultProcessor;

impl FieldDefaultProcessor {
  pub(crate) fn process(types: &mut [RustType]) {
    DefaultVariantResolver::resolve_fields(types);
    Self::derive_required_enum_defaults(types);
  }

  /// Switches enums without a `Default` to a derived one when a struct field without an
  /// explicit default, or the default variant of an enum deriving `Default`, holds them.
  fn derive_required_enum_defaults(types: &mut [RustType]) {
    let mut enum_indices = HashMap::new();
    let mut pending = vec![];
    for (index, rust_type) in types.iter().enumerate() {
      match rust_type {
        RustType::Struct(def) => pending.extend(
          def
            .fields
            .iter()
            .filter(|f| f.default_variant.is_none())
            .filter_map(|f| defaulted_type_name(&f.rust_type)),
        ),
        RustType::Enum(def) => {
          enum_indices.insert(def.name.to_atom(), index);
          if matches!(def.default_mode, EnumDefault::Derive) {
            pending.extend(derived_default_payload(def));
          }
        }
        _ => {}
      }
    }

    while let Some(name) = pending.pop() {
      let Some(&index) = enum_indices.get(&name) else {
        continue;
      };
      let RustType::Enum(def) = &mut types[index] else {
        continue;
      };
      if matches!(def.default_mode, EnumDefault::None) {
        def.default_mode = EnumDefault::Derive;
        pending.extend(derived_default_payload(def));
      }
    }
  }
}

/// Name of the generated type whose `Default` a derived `Default` calls for a value of
/// `type_ref`, or `None` when `Option` or a collection supplies the default instead.
fn defaulted_type_name(type_ref: &TypeRef) -> Option<DefaultAtom> {
  if type_ref.nullable || type_ref.is_array {
    return None;
  }
  match &type_ref.base_type {
    RustPrimitive::Custom(name) => Some(name.clone()),
    _ => None,
  }
}

/// Generated type wrapped by the variant the enum's derived `Default` constructs.
fn derived_default_payload(def: &EnumDef) -> Option<DefaultAtom> {
  let variant = def.variants.get(def.derived_default_index())?;
  defaulted_type_name(variant.single_wrapped_type()?)
}

/// Resolves struct field defaults against the final enum definitions their types name.
struct DefaultVariantResolver<'a> {
  enums: HashMap<DefaultAtom, &'a EnumDef>,
}

impl<'a> DefaultVariantResolver<'a> {
  fn resolve_fields(types: &'a mut [RustType]) {
    let mut fields = vec![];
    let mut enums = HashMap::new();
    for rust_type in types {
      match rust_type {
        RustType::Struct(def) => fields.extend(def.fields.iter_mut().filter(|f| f.default_value.is_some())),
        RustType::Enum(def) => {
          enums.insert(def.name.to_atom(), &*def);
        }
        _ => {}
      }
    }

    let resolver = Self { enums };
    for field in fields {
      field.default_variant = resolver.resolve(field);
    }
  }

  fn resolve(&self, field: &FieldDef) -> Option<DefaultVariant> {
    let value = field.default_value.as_ref()?;
    if field.rust_type.is_array {
      return None;
    }
    let enum_def = self.enum_named_by(&field.rust_type)?;
    let normalized = NormalizedVariant::try_from(value).ok()?;

    enum_def.variants.iter().find_map(|variant| match &variant.content {
      VariantContent::Unit => variant_matches_value(variant, &normalized.rename_value).then(|| DefaultVariant {
        variant: variant.name.clone(),
        wrapped_type: None,
        inner_variant: None,
      }),
      VariantContent::Tuple(_) => {
        let type_ref = variant.single_wrapped_type().filter(|t| !t.is_array)?;
        let enum_variants = self.enum_named_by(type_ref).map_or(&[][..], |def| &def.variants);
        let payload = wrapped_default_payload(type_ref, value, &normalized.rename_value, enum_variants)?;
        Some(DefaultVariant {
          variant: variant.name.clone(),
          wrapped_type: Some(type_ref.clone()),
          inner_variant: payload.inner_variant,
        })
      }
    })
  }

  fn enum_named_by(&self, type_ref: &TypeRef) -> Option<&EnumDef> {
    let RustPrimitive::Custom(name) = &type_ref.base_type else {
      return None;
    };
    self.enums.get(name).copied()
  }
}

use std::collections::BTreeSet;

use crate::generator::ast::{DefaultAtom, RustPrimitive, RustType, TypeRef, ValidationAttribute};

pub(crate) struct NestedValidationProcessor {
  validated_structs: BTreeSet<DefaultAtom>,
}

impl NestedValidationProcessor {
  pub(crate) fn new(types: &[RustType]) -> Self {
    let validated_structs = types
      .iter()
      .filter_map(|rt| match rt {
        RustType::Struct(def) if def.has_validation_attrs() => Some(def.name.to_atom()),
        _ => None,
      })
      .collect::<BTreeSet<_>>();

    Self { validated_structs }
  }

  pub(crate) fn process(&mut self, types: &mut [RustType]) {
    let mut changed = true;
    while changed {
      changed = false;

      for rust_type in &mut *types {
        let RustType::Struct(def) = rust_type else {
          continue;
        };

        let mut updated_struct = false;
        for field in &mut def.fields {
          let Some(referenced) = Self::referenced_custom_atom(&field.rust_type) else {
            continue;
          };

          if !self.validated_structs.contains(&referenced) {
            continue;
          }

          if field.validation_attrs.contains(&ValidationAttribute::Nested) {
            continue;
          }

          field.validation_attrs.push(ValidationAttribute::Nested);
          updated_struct = true;
        }

        if updated_struct && self.validated_structs.insert(def.name.to_atom()) {
          changed = true;
        }
      }
    }
  }

  fn referenced_custom_atom(type_ref: &TypeRef) -> Option<DefaultAtom> {
    match &type_ref.base_type {
      RustPrimitive::Custom(atom) => Some(atom.clone()),
      _ => None,
    }
  }
}

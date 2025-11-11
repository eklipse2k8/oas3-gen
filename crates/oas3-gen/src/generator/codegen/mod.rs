use std::collections::{BTreeMap, HashSet};

use proc_macro2::TokenStream;
use quote::quote;

use super::ast::RustType;

pub mod attributes;
pub mod coercion;
pub mod constants;
pub mod enums;
pub mod error_impls;
pub mod structs;
pub mod type_aliases;

#[cfg(test)]
mod tests;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Visibility {
  #[default]
  Public,
  Crate,
  File,
}

impl Visibility {
  pub fn parse(s: &str) -> Option<Self> {
    match s {
      "public" => Some(Visibility::Public),
      "crate" => Some(Visibility::Crate),
      "file" => Some(Visibility::File),
      _ => None,
    }
  }

  pub(crate) fn to_tokens(self) -> TokenStream {
    match self {
      Visibility::Public => quote! { pub },
      Visibility::Crate => quote! { pub(crate) },
      Visibility::File => quote! {},
    }
  }
}

pub(crate) fn generate(
  types: &[RustType],
  headers: &[&String],
  error_schemas: &HashSet<String>,
  visibility: Visibility,
) -> TokenStream {
  let ordered = deduplicate_and_order_types(types);
  let (regex_consts, regex_lookup) = constants::generate_regex_constants(&ordered);
  let header_consts = constants::generate_header_constants(headers);
  let serde_use = compute_serde_use(&ordered);
  let type_tokens: Vec<TokenStream> = ordered
    .iter()
    .map(|ty| generate_type(ty, &regex_lookup, error_schemas, visibility))
    .collect();

  quote! {
    #serde_use

    #regex_consts

    #header_consts

    #(#type_tokens)*
  }
}

fn deduplicate_and_order_types<'a>(types: &'a [RustType]) -> Vec<&'a RustType> {
  let mut map: BTreeMap<String, &'a RustType> = BTreeMap::new();
  for ty in types {
    let name = ty.type_name().to_string();

    if let Some(existing) = map.get(&name) {
      let existing_priority = type_priority(existing);
      let new_priority = type_priority(ty);

      if new_priority < existing_priority {
        map.insert(name, ty);
      }
    } else {
      map.insert(name, ty);
    }
  }
  map.into_values().collect()
}

fn type_priority(rust_type: &RustType) -> u8 {
  match rust_type {
    RustType::Struct(_) => 0,
    RustType::ResponseEnum(_) => 1,
    RustType::DiscriminatedEnum(_) => 2,
    RustType::Enum(_) => 3,
    RustType::TypeAlias(_) => 4,
  }
}

fn generate_type(
  rust_type: &RustType,
  regex_lookup: &BTreeMap<constants::RegexKey, String>,
  error_schemas: &HashSet<String>,
  visibility: Visibility,
) -> TokenStream {
  let type_tokens = match rust_type {
    RustType::Struct(def) => structs::generate_struct(def, regex_lookup, visibility),
    RustType::Enum(def) => enums::generate_enum(def, visibility),
    RustType::TypeAlias(def) => type_aliases::generate_type_alias(def, visibility),
    RustType::DiscriminatedEnum(def) => enums::generate_discriminated_enum(def, visibility),
    RustType::ResponseEnum(def) => enums::generate_response_enum(def, visibility),
  };

  if let Some(error_impl) = try_generate_error_impl(rust_type, error_schemas) {
    quote! {
      #type_tokens
      #error_impl
    }
  } else {
    type_tokens
  }
}

fn try_generate_error_impl(rust_type: &RustType, error_schemas: &HashSet<String>) -> Option<TokenStream> {
  match rust_type {
    RustType::Struct(def) if error_schemas.contains(&def.name) => error_impls::generate_error_impl(rust_type),
    RustType::Enum(def) if error_schemas.contains(&def.name) => error_impls::generate_error_impl(rust_type),
    _ => None,
  }
}

fn compute_serde_use(types: &[&RustType]) -> TokenStream {
  let mut needs_serialize = false;
  let mut needs_deserialize = false;

  for ty in types {
    match ty {
      RustType::Struct(def) => {
        needs_serialize |= derives_include(&def.derives, "Serialize");
        needs_deserialize |= derives_include(&def.derives, "Deserialize");
      }
      RustType::Enum(def) => {
        needs_serialize |= derives_include(&def.derives, "Serialize");
        needs_deserialize |= derives_include(&def.derives, "Deserialize");
      }
      RustType::ResponseEnum(_) | RustType::DiscriminatedEnum(_) | RustType::TypeAlias(_) => {}
    }
  }

  match (needs_serialize, needs_deserialize) {
    (true, true) => quote! { use serde::{Deserialize, Serialize}; },
    (true, false) => quote! { use serde::Serialize; },
    (false, true) => quote! { use serde::Deserialize; },
    (false, false) => quote! {},
  }
}

fn derives_include(derives: &[String], target: &str) -> bool {
  derives.iter().any(|derive| derive == target)
}

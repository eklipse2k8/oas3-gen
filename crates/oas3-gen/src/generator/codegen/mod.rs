use std::collections::{BTreeMap, HashSet};

use proc_macro2::TokenStream;
use quote::quote;

use super::ast::{OperationInfo, RustType};

pub mod attributes;
pub mod coercion;
pub mod constants;
pub mod derives;
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TypeUsage {
  RequestOnly,
  ResponseOnly,
  Bidirectional,
}

pub(crate) fn build_type_usage_map(operations: &[OperationInfo]) -> BTreeMap<String, TypeUsage> {
  let mut usage_map: BTreeMap<String, (bool, bool)> = BTreeMap::new();

  for op in operations {
    if let Some(ref req_type) = op.request_type {
      let entry = usage_map.entry(req_type.clone()).or_insert((false, false));
      entry.0 = true;
    }

    for body_type in &op.request_body_types {
      let entry = usage_map.entry(body_type.clone()).or_insert((false, false));
      entry.0 = true;
    }

    if let Some(ref resp_type) = op.response_type {
      let entry = usage_map.entry(resp_type.clone()).or_insert((false, false));
      entry.1 = true;
    }
  }

  usage_map
    .into_iter()
    .map(|(type_name, (in_request, in_response))| {
      let usage = match (in_request, in_response) {
        (true, false) => TypeUsage::RequestOnly,
        (false, true) => TypeUsage::ResponseOnly,
        (true, true) | (false, false) => TypeUsage::Bidirectional,
      };
      (type_name, usage)
    })
    .collect()
}

pub(crate) fn generate(
  types: &[RustType],
  type_usage: &BTreeMap<String, TypeUsage>,
  headers: &[&String],
  error_schemas: &HashSet<String>,
  visibility: Visibility,
) -> TokenStream {
  let ordered = deduplicate_and_order_types(types);
  let (regex_consts, regex_lookup) = constants::generate_regex_constants(&ordered);
  let header_consts = constants::generate_header_constants(headers);
  let type_tokens: Vec<TokenStream> = ordered
    .iter()
    .map(|ty| generate_type(ty, &regex_lookup, type_usage, error_schemas, visibility))
    .collect();

  quote! {
    use serde::{Deserialize, Serialize};

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
    RustType::DiscriminatedEnum(_) => 1,
    RustType::Enum(_) => 2,
    RustType::TypeAlias(_) => 3,
  }
}

fn generate_type(
  rust_type: &RustType,
  regex_lookup: &BTreeMap<constants::RegexKey, String>,
  type_usage: &BTreeMap<String, TypeUsage>,
  error_schemas: &HashSet<String>,
  visibility: Visibility,
) -> TokenStream {
  let type_tokens = match rust_type {
    RustType::Struct(def) => structs::generate_struct(def, regex_lookup, type_usage, visibility),
    RustType::Enum(def) => enums::generate_enum(def, visibility),
    RustType::TypeAlias(def) => type_aliases::generate_type_alias(def, visibility),
    RustType::DiscriminatedEnum(def) => enums::generate_discriminated_enum(def, visibility),
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

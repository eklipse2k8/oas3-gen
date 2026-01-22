use std::{collections::BTreeMap, rc::Rc};

use proc_macro2::TokenStream;
use quote::{ToTokens, quote};

use crate::generator::{
  ast::{MethodKind, RegexKey, RustType, SerdeImpl, ValidationAttribute, constants::HttpHeaderRef, tokens::ConstToken},
  codegen::{
    Visibility,
    constants::{HeaderConstantsFragment, RegexConstantsResult},
    enums::{DiscriminatedEnumFragment, EnumFragment, ResponseEnumFragment},
    server::AxumResponseEnumFragment,
    structs::StructFragment,
    type_aliases::TypeAliasFragment,
  },
  converter::GenerationTarget,
};

#[derive(Clone, Debug)]
pub(crate) struct TypeFragment {
  rust_type: RustType,
  regex_lookup: BTreeMap<RegexKey, ConstToken>,
  visibility: Visibility,
  target: GenerationTarget,
}

impl TypeFragment {
  pub(crate) fn new(
    rust_type: RustType,
    regex_lookup: BTreeMap<RegexKey, ConstToken>,
    visibility: Visibility,
    target: GenerationTarget,
  ) -> Self {
    Self {
      rust_type,
      regex_lookup,
      visibility,
      target,
    }
  }
}

impl ToTokens for TypeFragment {
  fn to_tokens(&self, tokens: &mut TokenStream) {
    let ts = match &self.rust_type {
      RustType::Struct(def) => {
        StructFragment::new(def.clone(), self.regex_lookup.clone(), self.visibility).into_token_stream()
      }
      RustType::Enum(def) => EnumFragment::new(def.clone(), self.visibility).into_token_stream(),
      RustType::TypeAlias(def) => TypeAliasFragment::new(def.clone(), self.visibility).into_token_stream(),
      RustType::DiscriminatedEnum(def) => {
        DiscriminatedEnumFragment::new(def.clone(), self.visibility).into_token_stream()
      }
      RustType::ResponseEnum(def) => match self.target {
        GenerationTarget::Server => AxumResponseEnumFragment::new(self.visibility, def.clone()).into_token_stream(),
        GenerationTarget::Client => ResponseEnumFragment::new(self.visibility, def.clone()).into_token_stream(),
      },
    };
    tokens.extend(ts);
  }
}

#[derive(Clone, Debug)]
pub(crate) struct TypesFragment {
  rust_types: Rc<Vec<RustType>>,
  header_refs: Rc<Vec<HttpHeaderRef>>,
  visibility: Visibility,
  target: GenerationTarget,
}

impl TypesFragment {
  pub(crate) fn new(
    rust_types: Rc<Vec<RustType>>,
    header_refs: Rc<Vec<HttpHeaderRef>>,
    visibility: Visibility,
    target: GenerationTarget,
  ) -> Self {
    Self {
      rust_types,
      header_refs,
      visibility,
      target,
    }
  }
}

impl ToTokens for TypesFragment {
  fn to_tokens(&self, tokens: &mut TokenStream) {
    let regex_result = RegexConstantsResult::from_types(&self.rust_types);
    let header_consts = HeaderConstantsFragment::new((*self.header_refs).clone());

    let mut needs_serialize = false;
    let mut needs_deserialize = false;
    let mut needs_validate = false;
    let mut type_tokens = vec![];

    for ty in self.rust_types.iter() {
      needs_serialize |= ty.is_serializable() == SerdeImpl::Derive;
      needs_deserialize |= ty.is_deserializable() == SerdeImpl::Derive;
      needs_validate |= matches!(ty, RustType::Struct(def) if def.fields.iter().any(|f| f.validation_attrs.contains(&ValidationAttribute::Nested)));
      needs_validate |=
        matches!(ty, RustType::Struct(def) if def.methods.iter().any(|m| matches!(m.kind, MethodKind::Builder { .. })));
      let type_fragment = TypeFragment::new(ty.clone(), regex_result.lookup.clone(), self.visibility, self.target);
      type_tokens.push(type_fragment.into_token_stream());
    }

    let serde_use = match (needs_serialize, needs_deserialize) {
      (true, true) => quote! { use serde::{Deserialize, Serialize}; },
      (true, false) => quote! { use serde::Serialize; },
      (false, true) => quote! { use serde::Deserialize; },
      (false, false) => quote! {},
    };

    let validator_use = if needs_validate {
      quote! { use validator::Validate; }
    } else {
      quote! {}
    };

    let axum_use = if self.target == GenerationTarget::Server {
      quote! { use axum::response::IntoResponse; }
    } else {
      quote! {}
    };

    let ts = quote! {
      #serde_use
      #validator_use
      #axum_use

      #regex_result
      #header_consts

      #(#type_tokens)*
    };

    tokens.extend(ts);
  }
}

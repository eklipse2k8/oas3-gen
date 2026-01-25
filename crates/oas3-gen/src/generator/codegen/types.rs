use std::{
  collections::{BTreeMap, BTreeSet},
  rc::Rc,
};

use proc_macro2::TokenStream;
use quote::{ToTokens, quote};
use syn::Path;

use crate::generator::{
  ast::{RegexKey, RustType, constants::HttpHeaderRef, tokens::ConstToken},
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
        StructFragment::new(def.clone(), self.regex_lookup.clone(), self.visibility, self.target).into_token_stream()
      }
      RustType::Enum(def) => EnumFragment::new(def.clone(), self.visibility, self.target).into_token_stream(),
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
  uses: BTreeSet<String>,
  visibility: Visibility,
  target: GenerationTarget,
}

impl TypesFragment {
  pub(crate) fn new(
    rust_types: Rc<Vec<RustType>>,
    header_refs: Rc<Vec<HttpHeaderRef>>,
    uses: BTreeSet<String>,
    visibility: Visibility,
    target: GenerationTarget,
  ) -> Self {
    Self {
      rust_types,
      header_refs,
      uses,
      visibility,
      target,
    }
  }
}

impl ToTokens for TypesFragment {
  fn to_tokens(&self, tokens: &mut TokenStream) {
    let use_statements = ModuleUsesFragment::new(self.uses.clone());
    let regex_result = RegexConstantsResult::from_types(&self.rust_types);
    let header_consts = HeaderConstantsFragment::new((*self.header_refs).clone());

    let type_tokens = self
      .rust_types
      .iter()
      .map(|ty| TypeFragment::new(ty.clone(), regex_result.lookup.clone(), self.visibility, self.target))
      .collect::<Vec<_>>();

    let ts = quote! {
      #use_statements

      #regex_result
      #header_consts

      #(#type_tokens)*
    };

    tokens.extend(ts);
  }
}

pub(crate) struct UseFragment {
  pub(crate) module: String,
  pub(crate) items: Vec<String>,
}

impl UseFragment {
  pub(crate) fn new(module: String, items: Vec<String>) -> Self {
    Self { module, items }
  }
}

impl ToTokens for UseFragment {
  fn to_tokens(&self, tokens: &mut TokenStream) {
    let Some(path) = syn::parse_str::<Path>(&self.module).ok() else {
      return;
    };

    let items = self
      .items
      .iter()
      .filter_map(|item| syn::parse_str::<Path>(item).ok())
      .collect::<Vec<_>>();

    if let [single] = items.as_slice() {
      tokens.extend(quote! { use #path::#single; });
    } else {
      tokens.extend(quote! { use #path::{#(#items),*}; });
    }
  }
}

pub(crate) struct ModuleUsesFragment(BTreeSet<String>);

impl ModuleUsesFragment {
  pub(crate) fn new(uses: BTreeSet<String>) -> Self {
    Self(uses)
  }
}

impl ToTokens for ModuleUsesFragment {
  fn to_tokens(&self, tokens: &mut TokenStream) {
    let mut current_module: Option<&str> = None;
    let mut current_items: Vec<String> = vec![];

    for path in &self.0 {
      let Some((module, item)) = path.rsplit_once("::") else {
        continue;
      };

      if let Some(current_module) = current_module
        && current_module != module
      {
        tokens.extend(UseFragment::new(current_module.to_string(), current_items.clone()).into_token_stream());
        current_items.clear();
      }

      current_module = Some(module);
      current_items.push(item.to_string());
    }

    if let Some(current_module) = current_module
      && !current_items.is_empty()
    {
      tokens.extend(UseFragment::new(current_module.to_string(), current_items).into_token_stream());
    }
  }
}

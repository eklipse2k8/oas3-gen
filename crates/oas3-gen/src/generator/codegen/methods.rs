use proc_macro2::TokenStream;
use quote::{ToTokens, quote};

use crate::generator::{
  ast::{FieldDef, MethodNode, TypeRef},
  codegen::Visibility,
};

pub(crate) trait HelperMethodParts {
  type Kind;
  fn method(&self) -> MethodNode<Self::Kind>;
  fn parameters(&self) -> impl ToTokens;
  fn implementation(&self) -> TokenStream;
}

#[derive(Clone, Debug)]
pub(crate) struct HelperMethodFragment<Parts>(Visibility, Parts)
where
  Parts: HelperMethodParts;

impl<Parts> HelperMethodFragment<Parts>
where
  Parts: HelperMethodParts,
{
  pub(crate) fn new(vis: Visibility, node: Parts) -> Self {
    Self(vis, node)
  }
}

impl<Parts> ToTokens for HelperMethodFragment<Parts>
where
  Parts: HelperMethodParts,
{
  fn to_tokens(&self, tokens: &mut TokenStream) {
    let method_name = self.1.method().name.clone();
    let docs = self.1.method().docs.clone();
    let parameter = self.1.parameters();
    let implementation = self.1.implementation();
    let vis = &self.0;

    let ts = quote! {
      #docs
      #vis fn #method_name(#parameter) -> Self {
        #implementation
      }
    };

    tokens.extend(ts);
  }
}

#[derive(Clone, Debug)]
pub(crate) struct FieldFunctionParameterFragment(FieldDef);

impl FieldFunctionParameterFragment {
  pub(crate) fn new(field: FieldDef) -> Self {
    Self(field)
  }
}

impl ToTokens for FieldFunctionParameterFragment {
  fn to_tokens(&self, tokens: &mut TokenStream) {
    let name = &self.0.name;
    let rust_type = &self.0.rust_type;

    let ts = quote! { #name: #rust_type };

    tokens.extend(ts);
  }
}

#[derive(Clone, Debug)]
pub(crate) struct StructConstructorFragment(TypeRef, Vec<FieldDef>);

impl StructConstructorFragment {
  pub(crate) fn new(type_token: TypeRef, fields: Vec<FieldDef>) -> Self {
    Self(type_token, fields)
  }
}

impl ToTokens for StructConstructorFragment {
  fn to_tokens(&self, tokens: &mut TokenStream) {
    let base_type = &self.0.base_type;
    let field_names = self.1.iter().map(|f| f.name.clone()).collect::<Vec<_>>();

    let parameters = quote! { #(#field_names),* };

    let constructor = quote! {
      #base_type {
        #parameters,
        ..Default::default()
      }
    };

    let ts = if self.0.boxed {
      quote! { Box::new(#constructor) }
    } else {
      constructor
    };

    tokens.extend(ts);
  }
}

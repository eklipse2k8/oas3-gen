use proc_macro2::TokenStream;
use quote::{format_ident, quote};

use super::{
  Visibility,
  attributes::{
    generate_deprecated_attr, generate_derives_from_slice, generate_docs, generate_outer_attrs, generate_serde_attrs,
  },
  coercion,
};
use crate::generator::ast::{DiscriminatedEnumDef, EnumDef, ResponseEnumDef, VariantContent, VariantDef};

pub(crate) fn generate_enum(def: &EnumDef, visibility: Visibility) -> TokenStream {
  let name = format_ident!("{}", def.name);
  let docs = generate_docs(&def.docs);
  let vis = visibility.to_tokens();
  let derives = generate_derives_from_slice(&def.derives);
  let outer_attrs = generate_outer_attrs(&def.outer_attrs);
  let serde_attrs = generate_enum_serde_attrs(def);
  let variants = generate_variants(&def.variants);

  quote! {
    #docs
    #outer_attrs
    #derives
    #serde_attrs
    #vis enum #name {
      #(#variants),*
    }
  }
}

pub(crate) fn generate_discriminated_enum(def: &DiscriminatedEnumDef, visibility: Visibility) -> TokenStream {
  let name = format_ident!("{}", def.name);
  let disc_field = &def.discriminator_field;
  let docs = generate_docs(&def.docs);
  let vis = visibility.to_tokens();

  let variants: Vec<TokenStream> = def
    .variants
    .iter()
    .map(|v| {
      let disc_value = &v.discriminator_value;
      let variant_name = format_ident!("{}", v.variant_name);
      let type_name = coercion::parse_type_string(&v.type_name);
      quote! { (#disc_value, #variant_name(#type_name)) }
    })
    .collect();

  if let Some(ref fallback) = def.fallback {
    let fallback_variant = format_ident!("{}", fallback.variant_name);
    let fallback_type = coercion::parse_type_string(&fallback.type_name);

    quote! {
      #docs
      oas3_gen_support::discriminated_enum! {
        #vis enum #name {
          discriminator: #disc_field,
          variants: [
            #(#variants),*
          ],
          fallback: #fallback_variant(#fallback_type),
        }
      }
    }
  } else {
    quote! {
      #docs
      oas3_gen_support::discriminated_enum! {
        #vis enum #name {
          discriminator: #disc_field,
          variants: [
            #(#variants),*
          ],
        }
      }
    }
  }
}

fn generate_variants(variants: &[VariantDef]) -> Vec<TokenStream> {
  variants
    .iter()
    .enumerate()
    .map(|(idx, variant)| {
      let name = format_ident!("{}", variant.name);
      let docs = generate_docs(&variant.docs);
      let serde_attrs = generate_serde_attrs(&variant.serde_attrs);
      let deprecated_attr = generate_deprecated_attr(variant.deprecated);
      let default_attr = if idx == 0 {
        quote! { #[default] }
      } else {
        quote! {}
      };

      let content = match &variant.content {
        VariantContent::Unit => quote! {},
        VariantContent::Tuple(types) => {
          let type_tokens: Vec<_> = types
            .iter()
            .map(|t| coercion::parse_type_string(&t.to_rust_type()))
            .collect();
          quote! { ( #(#type_tokens),* ) }
        }
      };

      quote! {
        #docs
        #deprecated_attr
        #serde_attrs
        #default_attr
        #name #content
      }
    })
    .collect()
}

fn generate_enum_serde_attrs(def: &EnumDef) -> TokenStream {
  let mut attrs = Vec::new();

  if let Some(ref discriminator) = def.discriminator {
    attrs.push(quote! { tag = #discriminator });
  }

  for attr in &def.serde_attrs {
    if let Ok(tokens) = attr.parse::<TokenStream>() {
      attrs.push(tokens);
    }
  }

  if attrs.is_empty() {
    return quote! {};
  }

  quote! {
    #[serde(#(#attrs),*)]
  }
}

pub(crate) fn generate_response_enum(def: &ResponseEnumDef, visibility: Visibility) -> TokenStream {
  let name = format_ident!("{}", def.name);
  let docs = generate_docs(&def.docs);
  let vis = visibility.to_tokens();

  let variants: Vec<TokenStream> = def
    .variants
    .iter()
    .map(|v| {
      let variant_name = format_ident!("{}", v.variant_name);
      let variant_docs = if let Some(ref desc) = v.description {
        let doc_line = format!("{}: {}", v.status_code, desc);
        quote! { #[doc = #doc_line] }
      } else {
        let doc_line = v.status_code.clone();
        quote! { #[doc = #doc_line] }
      };

      if let Some(ref schema) = v.schema_type {
        let type_token = coercion::parse_type_string(&schema.to_rust_type());
        quote! {
          #variant_docs
          #variant_name(#type_token)
        }
      } else {
        quote! {
          #variant_docs
          #variant_name
        }
      }
    })
    .collect();

  quote! {
    #docs
    #[derive(Clone, Debug)]
    #vis enum #name {
      #(#variants),*
    }
  }
}

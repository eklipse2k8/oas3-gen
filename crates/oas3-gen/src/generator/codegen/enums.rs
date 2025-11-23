use proc_macro2::TokenStream;
use quote::{format_ident, quote};

use super::{
  Visibility,
  attributes::{
    generate_deprecated_attr, generate_derives_from_slice, generate_docs, generate_outer_attrs, generate_serde_attrs,
  },
  coercion,
};
use crate::generator::ast::{
  DeriveTrait, DiscriminatedEnumDef, EnumDef, EnumMethodKind, ResponseEnumDef, SerdeAttribute, VariantContent,
  VariantDef,
};

pub(crate) fn generate_enum(def: &EnumDef, visibility: Visibility) -> TokenStream {
  let name = format_ident!("{}", def.name);
  let docs = generate_docs(&def.docs);
  let vis = visibility.to_tokens();

  let mut derives_list = def.derives.clone();
  if def.case_insensitive {
    derives_list.remove(&DeriveTrait::Deserialize);
  }
  let derives = generate_derives_from_slice(&derives_list);

  let outer_attrs = generate_outer_attrs(&def.outer_attrs);
  let serde_attrs = generate_enum_serde_attrs(def);
  let variants = generate_variants(&def.variants);
  let methods = generate_enum_methods(def, visibility);

  let enum_def = quote! {
    #docs
    #outer_attrs
    #derives
    #serde_attrs
    #vis enum #name {
      #(#variants),*
    }
    #methods
  };

  if def.case_insensitive {
    let deserialize_impl = generate_case_insensitive_deserialize(def);
    quote! {
      #enum_def
      #deserialize_impl
    }
  } else {
    enum_def
  }
}

fn generate_enum_methods(def: &EnumDef, visibility: Visibility) -> TokenStream {
  if def.methods.is_empty() {
    return quote! {};
  }

  let name = format_ident!("{}", def.name);
  let vis = visibility.to_tokens();

  let methods = def.methods.iter().map(|m| {
    let method_name = format_ident!("{}", m.name);
    let docs = generate_docs(&m.docs);

    match &m.kind {
      EnumMethodKind::SimpleConstructor {
        variant_name,
        wrapped_type,
      } => {
        let variant = format_ident!("{}", variant_name);
        let type_name = coercion::parse_type_string(wrapped_type);
        quote! {
          #docs
          #vis fn #method_name() -> Self {
            Self::#variant(#type_name::default())
          }
        }
      }
      EnumMethodKind::ParameterizedConstructor {
        variant_name,
        wrapped_type,
        param_name,
        param_type,
      } => {
        let variant = format_ident!("{}", variant_name);
        let type_name = coercion::parse_type_string(wrapped_type);
        let param_ident = format_ident!("{}", param_name);
        let param_ty = coercion::parse_type_string(param_type);
        quote! {
          #docs
          #vis fn #method_name(#param_ident: #param_ty) -> Self {
            Self::#variant(#type_name {
              #param_ident,
              ..Default::default()
            })
          }
        }
      }
    }
  });

  quote! {
    impl #name {
      #(#methods)*
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
    if let Ok(tokens) = attr.to_string().parse::<TokenStream>() {
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

fn generate_case_insensitive_deserialize(def: &EnumDef) -> TokenStream {
  let name = format_ident!("{}", def.name);

  let match_arms: Vec<TokenStream> = def
    .variants
    .iter()
    .map(|v| {
      let variant_name = format_ident!("{}", v.name);
      let mut rename = v.name.clone();
      for attr in &v.serde_attrs {
        if let SerdeAttribute::Rename(val) = attr {
          rename.clone_from(val);
        }
      }
      let lower_val = rename.to_ascii_lowercase();
      quote! {
        #lower_val => Ok(#name::#variant_name),
      }
    })
    .collect();

  let error_variants_list: Vec<String> = def
    .variants
    .iter()
    .map(|v| {
      let mut rename = v.name.clone();
      for attr in &v.serde_attrs {
        if let SerdeAttribute::Rename(val) = attr {
          rename.clone_from(val);
        }
      }
      rename
    })
    .collect();

  let error_variants_list_tokens = quote! { &[ #(#error_variants_list),* ] };

  // Check for fallback variant (Unknown/Other)
  let fallback_arm =
    if let Some(unknown_variant) = def.variants.iter().find(|v| v.name == "Unknown" || v.name == "Other") {
      let variant_name = format_ident!("{}", unknown_variant.name);
      quote! { _ => Ok(#name::#variant_name), }
    } else {
      quote! { _ => Err(serde::de::Error::unknown_variant(&s, #error_variants_list_tokens)), }
    };

  quote! {
    impl<'de> serde::Deserialize<'de> for #name {
      fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
      where
        D: serde::Deserializer<'de>,
      {
        let s = String::deserialize(deserializer)?;
        match s.to_ascii_lowercase().as_str() {
          #(#match_arms)*
          #fallback_arm
        }
      }
    }
  }
}

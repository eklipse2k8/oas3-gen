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
  DeriveTrait, DiscriminatedEnumDef, EnumDef, EnumMethodKind, EnumVariantToken, ResponseEnumDef, SerdeAttribute,
  SerdeMode, VariantContent, VariantDef,
};

pub(crate) fn generate_enum(def: &EnumDef, visibility: Visibility) -> TokenStream {
  let name = &def.name;
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

  let name = &def.name;
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
  let name = &def.name;
  let disc_field = &def.discriminator_field;
  let docs = generate_docs(&def.docs);
  let vis = visibility.to_tokens();

  let enum_variants: Vec<TokenStream> = def
    .variants
    .iter()
    .map(|v| {
      let variant_name = format_ident!("{}", v.variant_name);
      let type_name = coercion::parse_type_string(&v.type_name);
      quote! { #variant_name(#type_name) }
    })
    .collect();

  let fallback_variant_def = def.fallback.as_ref().map(|fb| {
    let fallback_variant = format_ident!("{}", fb.variant_name);
    let fallback_type = coercion::parse_type_string(&fb.type_name);
    quote! { #fallback_variant(#fallback_type) }
  });

  let all_variants = if let Some(fb) = fallback_variant_def {
    quote! { #(#enum_variants),*, #fb }
  } else {
    quote! { #(#enum_variants),* }
  };

  let enum_def = quote! {
    #docs
    #[derive(Debug, Clone, PartialEq)]
    #vis enum #name {
      #all_variants
    }
  };

  let discriminator_const = quote! {
    impl #name {
      #vis const DISCRIMINATOR_FIELD: &'static str = #disc_field;
    }
  };

  let default_impl = generate_discriminated_default_impl(def);
  let serialize_impl = generate_discriminated_serialize_impl(def);
  let deserialize_impl = generate_discriminated_deserialize_impl(def);

  quote! {
    #enum_def
    #discriminator_const
    #default_impl
    #serialize_impl
    #deserialize_impl
  }
}

fn generate_discriminated_default_impl(def: &DiscriminatedEnumDef) -> TokenStream {
  let name = &def.name;

  if let Some(ref fb) = def.fallback {
    let fallback_variant = format_ident!("{}", fb.variant_name);
    let fallback_type = coercion::parse_type_string(&fb.type_name);
    quote! {
      impl Default for #name {
        fn default() -> Self {
          Self::#fallback_variant(<#fallback_type>::default())
        }
      }
    }
  } else if let Some(first) = def.variants.first() {
    let first_variant = format_ident!("{}", first.variant_name);
    let first_type = coercion::parse_type_string(&first.type_name);
    quote! {
      impl Default for #name {
        fn default() -> Self {
          Self::#first_variant(<#first_type>::default())
        }
      }
    }
  } else {
    quote! {}
  }
}

fn generate_discriminated_serialize_impl(def: &DiscriminatedEnumDef) -> TokenStream {
  if def.serde_mode == SerdeMode::DeserializeOnly {
    return quote! {};
  }

  let name = &def.name;

  let variant_arms: Vec<TokenStream> = def
    .variants
    .iter()
    .map(|v| {
      let variant_name = format_ident!("{}", v.variant_name);
      quote! { Self::#variant_name(v) => v.serialize(serializer) }
    })
    .collect();

  let fallback_arm = def.fallback.as_ref().map(|fb| {
    let fallback_variant = format_ident!("{}", fb.variant_name);
    quote! { Self::#fallback_variant(v) => v.serialize(serializer) }
  });

  let all_arms = if let Some(fb) = fallback_arm {
    quote! { #(#variant_arms,)* #fb }
  } else {
    quote! { #(#variant_arms),* }
  };

  quote! {
    impl serde::Serialize for #name {
      fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
      where
        S: serde::Serializer,
      {
        match self {
          #all_arms
        }
      }
    }
  }
}

fn generate_discriminated_deserialize_impl(def: &DiscriminatedEnumDef) -> TokenStream {
  if def.serde_mode == SerdeMode::SerializeOnly {
    return quote! {};
  }

  let name = &def.name;
  let disc_field = &def.discriminator_field;

  let variant_arms: Vec<TokenStream> = def
    .variants
    .iter()
    .map(|v| {
      let disc_value = &v.discriminator_value;
      let variant_name = format_ident!("{}", v.variant_name);
      quote! {
        Some(#disc_value) => serde_json::from_value(value)
          .map(Self::#variant_name)
          .map_err(serde::de::Error::custom)
      }
    })
    .collect();

  let none_handling = if let Some(ref fb) = def.fallback {
    let fallback_variant = format_ident!("{}", fb.variant_name);
    quote! {
      None => serde_json::from_value(value)
        .map(Self::#fallback_variant)
        .map_err(serde::de::Error::custom)
    }
  } else {
    quote! {
      None => Err(serde::de::Error::missing_field(Self::DISCRIMINATOR_FIELD))
    }
  };

  quote! {
    impl<'de> serde::Deserialize<'de> for #name {
      fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
      where
        D: serde::Deserializer<'de>,
      {
        let value = serde_json::Value::deserialize(deserializer)?;
        match value.get(Self::DISCRIMINATOR_FIELD).and_then(|v| v.as_str()) {
          #(#variant_arms,)*
          #none_handling,
          Some(other) => Err(serde::de::Error::custom(format!(
            "Unknown discriminator value '{}' for field '{}'",
            other, #disc_field
          ))),
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
  let mut attrs = vec![];

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
  let name = &def.name;
  let docs = generate_docs(&def.docs);
  let vis = visibility.to_tokens();

  let variants: Vec<TokenStream> = def
    .variants
    .iter()
    .map(|v| {
      let variant_name = &v.variant_name;
      let variant_docs = if let Some(ref desc) = v.description {
        let doc_line = format!("{}: {desc}", v.status_code);
        quote! { #[doc = #doc_line] }
      } else {
        let doc_line = v.status_code.to_string();
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
  let name = &def.name;

  let (match_arms, error_variants_list): (Vec<TokenStream>, Vec<String>) = def
    .variants
    .iter()
    .map(|v| {
      let variant_name = format_ident!("{}", v.name);
      let serde_name = v
        .serde_attrs
        .iter()
        .find_map(|attr| match attr {
          SerdeAttribute::Rename(val) => Some(val.clone()),
          _ => None,
        })
        .unwrap_or_else(|| v.name.to_string());

      let lower_val = serde_name.to_ascii_lowercase();
      let match_arm = quote! {
        #lower_val => Ok(#name::#variant_name),
      };
      (match_arm, serde_name)
    })
    .unzip();

  let error_variants_list_tokens = quote! { &[ #(#error_variants_list),* ] };

  // Check for fallback variant (Unknown/Other)
  let fallback_arm = {
    let fallback_candidates = [EnumVariantToken::new("Unknown"), EnumVariantToken::new("Other")];
    if let Some(unknown_variant) = def.variants.iter().find(|v| fallback_candidates.contains(&v.name)) {
      let variant_name = format_ident!("{}", unknown_variant.name);
      quote! { _ => Ok(#name::#variant_name), }
    } else {
      quote! { _ => Err(serde::de::Error::unknown_variant(&s, #error_variants_list_tokens)), }
    }
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

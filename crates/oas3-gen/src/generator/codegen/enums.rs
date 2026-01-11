use std::rc::Rc;

use proc_macro2::TokenStream;
use quote::{ToTokens, TokenStreamExt as _, format_ident, quote};

use super::{
  CodeGenerationContext, Visibility,
  attributes::{generate_deprecated_attr, generate_outer_attrs, generate_serde_attrs},
};
use crate::generator::{
  ast::{
    DeriveTrait, DerivesProvider, DiscriminatedEnumDef, DiscriminatedVariant, EnumDef, EnumMethod, EnumMethodKind,
    ResponseEnumDef, ResponseVariant, SerdeAttribute, SerdeMode, VariantContent, VariantDef,
  },
  codegen::{attributes::DeriveAttribute, server::AxumIntoResponse},
  converter::GenerationTarget,
};

fn box_if_needed(boxed: bool, inner: TokenStream) -> TokenStream {
  if boxed {
    quote! { Box::new(#inner) }
  } else {
    inner
  }
}

fn emit_enum_methods(name: impl ToTokens, vis: Visibility, methods: &[EnumMethod]) -> TokenStream {
  if methods.is_empty() {
    return quote! {};
  }

  let method_tokens = methods.iter().map(|m| emit_enum_method(vis, m));

  quote! {
    impl #name {
      #(#method_tokens)*
    }
  }
}

fn emit_enum_method(vis: Visibility, method: &EnumMethod) -> TokenStream {
  let method_name = &method.name;
  let docs = &method.docs;

  match &method.kind {
    EnumMethodKind::SimpleConstructor {
      variant_name,
      wrapped_type,
    } => {
      let inner_type = &wrapped_type.base_type;
      let constructor = box_if_needed(wrapped_type.boxed, quote! { #inner_type::default() });

      quote! {
        #docs
        #vis fn #method_name() -> Self {
          Self::#variant_name(#constructor)
        }
      }
    }
    EnumMethodKind::ParameterizedConstructor {
      variant_name,
      wrapped_type,
      param_name,
      param_type,
    } => {
      let inner_type = &wrapped_type.base_type;
      let param_ident = format_ident!("{param_name}");

      let constructor = box_if_needed(
        wrapped_type.boxed,
        quote! {
          #inner_type {
            #param_ident,
            ..Default::default()
          }
        },
      );

      quote! {
        #docs
        #vis fn #method_name(#param_ident: #param_type) -> Self {
          Self::#variant_name(#constructor)
        }
      }
    }
    EnumMethodKind::KnownValueConstructor {
      known_type,
      known_variant,
    } => {
      quote! {
        #docs
        #vis fn #method_name() -> Self {
          Self::Known(#known_type::#known_variant)
        }
      }
    }
  }
}

#[derive(Clone, Debug)]
pub(crate) struct EnumGenerator {
  def: EnumDef,
  vis: Visibility,
}

impl EnumGenerator {
  pub fn new(_context: &Rc<CodeGenerationContext>, def: &EnumDef, visibility: Visibility) -> Self {
    Self {
      def: def.clone(),
      vis: visibility,
    }
  }

  pub fn generate(&self) -> TokenStream {
    let name = &self.def.name;
    let docs = &self.def.docs;

    let derives = DeriveAttribute::new(self.def.derives());

    let outer_attrs = generate_outer_attrs(&self.def.outer_attrs);
    let serde_attrs = self.emit_serde_attrs();
    let methods = emit_enum_methods(name, self.vis, &self.def.methods);
    let variants = self.emit_variants();

    let vis = &self.vis;
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

    let display_impl = if self.def.generate_display {
      self.emit_display_impl()
    } else {
      quote! {}
    };

    if self.def.case_insensitive {
      let deserialize_impl = self.emit_case_insensitive_deser();
      quote! {
        #enum_def
        #display_impl
        #deserialize_impl
      }
    } else {
      quote! {
        #enum_def
        #display_impl
      }
    }
  }

  fn emit_variants(&self) -> Vec<TokenStream> {
    let has_serde_derive = self
      .def
      .derives()
      .iter()
      .any(|d| matches!(d, DeriveTrait::Serialize | DeriveTrait::Deserialize));

    self
      .def
      .variants
      .iter()
      .enumerate()
      .map(|(idx, v)| {
        let variant_name = &v.name;
        let variant_docs = &v.docs;
        let variant_serde_attrs = if has_serde_derive {
          generate_serde_attrs(&v.serde_attrs)
        } else {
          quote! {}
        };
        let deprecated_attr = generate_deprecated_attr(v.deprecated);
        let default_attr = (idx == 0).then(|| quote! { #[default] });
        let content = v.content.tuple_types().map(|types| {
          let type_tokens: Vec<_> = types.iter().map(|t| quote! { #t }).collect();
          quote! { ( #(#type_tokens),* ) }
        });

        quote! {
          #variant_docs
          #deprecated_attr
          #variant_serde_attrs
          #default_attr
          #variant_name #content
        }
      })
      .collect()
  }

  fn emit_serde_attrs(&self) -> TokenStream {
    let mut all_attrs: Vec<SerdeAttribute> = Vec::with_capacity(self.def.serde_attrs.len() + 1);

    if let Some(ref discriminator) = self.def.discriminator {
      all_attrs.push(SerdeAttribute::Tag(discriminator.clone()));
    }

    all_attrs.extend(self.def.serde_attrs.iter().cloned());

    generate_serde_attrs(&all_attrs)
  }

  fn emit_display_impl(&self) -> TokenStream {
    let name = &self.def.name;
    let match_arms: Vec<TokenStream> = self.def.variants.iter().map(Self::emit_variant_display_arm).collect();

    quote! {
      impl core::fmt::Display for #name {
        fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
          match self {
            #(#match_arms)*
          }
        }
      }
    }
  }

  fn emit_variant_display_arm(variant: &VariantDef) -> TokenStream {
    let variant_name = &variant.name;
    match &variant.content {
      VariantContent::Unit => {
        let serde_name = variant.serde_name();
        quote! { Self::#variant_name => write!(f, #serde_name), }
      }
      VariantContent::Tuple(_) => {
        quote! { Self::#variant_name(v) => write!(f, "{v}"), }
      }
    }
  }

  fn emit_case_insensitive_deser(&self) -> TokenStream {
    let name = &self.def.name;

    let (match_arms, serde_names): (Vec<TokenStream>, Vec<String>) = self
      .def
      .variants
      .iter()
      .map(|v| {
        let variant_name = &v.name;
        let serde_name = v.serde_name();
        let lower_val = serde_name.to_ascii_lowercase();
        let match_arm = quote! {
          #lower_val => Ok(#name::#variant_name),
        };
        (match_arm, serde_name)
      })
      .unzip();

    let fallback_arm = if let Some(fb) = self.def.fallback_variant() {
      let variant_name = &fb.name;
      quote! { _ => Ok(#name::#variant_name), }
    } else {
      quote! { _ => Err(serde::de::Error::unknown_variant(&s, &[ #(#serde_names),* ])), }
    };

    quote! {
      impl<'de> serde::Deserialize<'de> for #name {
        fn deserialize<D>(deserializer: D) -> core::result::Result<Self, D::Error>
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
}

#[derive(Clone, Debug)]
pub(crate) struct DiscriminatedEnumGenerator {
  def: DiscriminatedEnumDef,
  vis: Visibility,
}

impl DiscriminatedEnumGenerator {
  pub fn new(_context: &Rc<CodeGenerationContext>, def: &DiscriminatedEnumDef, visibility: Visibility) -> Self {
    Self {
      def: def.clone(),
      vis: visibility,
    }
  }

  pub fn generate(&self) -> TokenStream {
    let name = &self.def.name;
    let disc_field = &self.def.discriminator_field;
    let docs = &self.def.docs;

    let variants = self
      .def
      .all_variants()
      .map(|v| EnumDiscriminantVariant::new(v.clone()))
      .collect::<Vec<_>>();

    let variants = EnumVariants::new(variants);
    let derives = DeriveAttribute::new(self.def.derives());

    let vis = &self.vis;
    let enum_def = quote! {
      #docs
      #derives
      #vis enum #name {
        #variants
      }
    };

    let discriminator_const = quote! {
      impl #name {
        #vis const DISCRIMINATOR_FIELD: &'static str = #disc_field;
      }
    };

    let default_impl = self.emit_default_impl();
    let serialize_impl = self.emit_serialize_impl();
    let deserialize_impl = self.emit_deserialize_impl();
    let methods_impl = emit_enum_methods(name, self.vis, &self.def.methods);

    quote! {
      #enum_def
      #discriminator_const
      #default_impl
      #serialize_impl
      #deserialize_impl
      #methods_impl
    }
  }

  fn emit_default_impl(&self) -> TokenStream {
    let Some(default_variant) = self.def.default_variant() else {
      return quote! {};
    };

    let name = &self.def.name;
    let variant_ident = &default_variant.variant_name;
    let type_tokens = &default_variant.type_name;

    quote! {
      impl Default for #name {
        fn default() -> Self {
          Self::#variant_ident(<#type_tokens>::default())
        }
      }
    }
  }

  fn emit_serialize_impl(&self) -> TokenStream {
    if self.def.serde_mode == SerdeMode::DeserializeOnly {
      return quote! {};
    }

    let name = &self.def.name;

    let arms: Vec<TokenStream> = self
      .def
      .all_variants()
      .map(|v| {
        let variant_name = &v.variant_name;
        quote! { Self::#variant_name(v) => v.serialize(serializer) }
      })
      .collect();

    quote! {
      impl serde::Serialize for #name {
        fn serialize<S>(&self, serializer: S) -> core::result::Result<S::Ok, S::Error>
        where
          S: serde::Serializer,
        {
          match self {
            #(#arms),*
          }
        }
      }
    }
  }

  fn emit_deserialize_impl(&self) -> TokenStream {
    if self.def.serde_mode == SerdeMode::SerializeOnly {
      return quote! {};
    }

    let name = &self.def.name;
    let disc_field = &self.def.discriminator_field;

    let variant_arms: Vec<TokenStream> = self
      .def
      .variants
      .iter()
      .flat_map(|v| {
        let variant_name = &v.variant_name;
        v.discriminator_values.iter().map(move |disc_value| {
          quote! {
            Some(#disc_value) => serde_json::from_value(value)
              .map(Self::#variant_name)
              .map_err(serde::de::Error::custom)
          }
        })
      })
      .collect();

    let none_handling = if let Some(ref fb) = self.def.fallback {
      let fallback_variant = &fb.variant_name;
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
        fn deserialize<D>(deserializer: D) -> core::result::Result<Self, D::Error>
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
}

#[derive(Clone, Debug)]
pub(crate) struct ResponseEnumGenerator {
  context: Rc<CodeGenerationContext>,
  def: ResponseEnumDef,
  vis: Visibility,
}

impl ResponseEnumGenerator {
  pub fn new(context: &Rc<CodeGenerationContext>, def: &ResponseEnumDef, visibility: Visibility) -> Self {
    Self {
      context: context.clone(),
      def: def.clone(),
      vis: visibility,
    }
  }

  pub fn generate(&self) -> TokenStream {
    let response = ResponseEnum::new(self.vis, self.def.clone());
    let into_response_impl = if self.context.config.target == GenerationTarget::Server {
      AxumIntoResponse::new(self.def.clone()).to_token_stream()
    } else {
      quote! {}
    };

    quote! {
      #response
      #into_response_impl
    }
  }
}

#[derive(Clone, Debug)]
pub struct ResponseEnum(Visibility, ResponseEnumDef);

impl ResponseEnum {
  pub(crate) fn new(vis: Visibility, def: ResponseEnumDef) -> Self {
    Self(vis, def)
  }

  fn variants(&self) -> Vec<EnumResponseVariant> {
    self
      .1
      .variants
      .iter()
      .map(|v| EnumResponseVariant::new(v.clone()))
      .collect()
  }
}

impl ToTokens for ResponseEnum {
  fn to_tokens(&self, tokens: &mut TokenStream) {
    let name = &self.1.name;
    let docs = &self.1.docs;
    let variants = EnumVariants::new(self.variants());
    let derives = DeriveAttribute::new(self.1.derives());
    let vis = &self.0;

    let ts = quote! {
      #docs
      #derives
      #vis enum #name {
        #variants
      }
    };

    tokens.extend(ts);
  }
}

#[derive(Clone, Debug)]
pub struct EnumVariants<T>(Vec<T>);

impl<T: ToTokens> EnumVariants<T> {
  pub fn new(variants: Vec<T>) -> Self {
    Self(variants)
  }
}

impl<T: ToTokens> ToTokens for EnumVariants<T> {
  fn to_tokens(&self, tokens: &mut TokenStream) {
    if self.0.is_empty() {
      return;
    }

    let variants = &self.0;
    tokens.append_all(quote! { #(#variants),* });
  }
}

/// Wrapper to convert `DiscriminatedVariant` to Enum variant
#[derive(Clone, Debug)]
pub(crate) struct EnumDiscriminantVariant(DiscriminatedVariant);

impl EnumDiscriminantVariant {
  pub(crate) fn new(variant: DiscriminatedVariant) -> Self {
    Self(variant)
  }
}

impl ToTokens for EnumDiscriminantVariant {
  fn to_tokens(&self, tokens: &mut TokenStream) {
    let variant_name = &self.0.variant_name;
    let type_name = &self.0.type_name;
    let ts = quote! { #variant_name(#type_name) };

    tokens.extend(ts);
  }
}

/// Wrapper to convert `ResponseVariant` to Enum variant
#[derive(Clone, Debug)]
pub(crate) struct EnumResponseVariant(ResponseVariant);

impl EnumResponseVariant {
  pub(crate) fn new(variant: ResponseVariant) -> Self {
    Self(variant)
  }
}

impl ToTokens for EnumResponseVariant {
  fn to_tokens(&self, tokens: &mut TokenStream) {
    let variant_name = &self.0.variant_name;
    let doc_line = self.0.doc_line();
    let content = self.0.schema_type.as_ref().map(|schema| {
      quote! { (#schema) }
    });

    let ts = quote! {
      #[doc = #doc_line]
      #variant_name #content
    };

    tokens.extend(ts);
  }
}

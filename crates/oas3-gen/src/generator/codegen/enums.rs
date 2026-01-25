use proc_macro2::TokenStream;
use quote::{ToTokens, TokenStreamExt as _, quote};

use super::{
  Visibility,
  attributes::{generate_deprecated_attr, generate_outer_attrs, generate_serde_attrs},
};
use crate::generator::{
  ast::{
    DeriveTrait, DerivesProvider, DiscriminatedEnumDef, DiscriminatedVariant, EnumDef, EnumMethod, EnumMethodKind,
    EnumToken, EnumVariantToken, FieldDef, ResponseEnumDef, ResponseVariant, SerdeMode, TypeRef, VariantContent,
    VariantDef,
  },
  codegen::{
    attributes::DeriveAttribute,
    methods::{FieldFunctionParameterFragment, HelperMethodFragment, HelperMethodParts, StructConstructorFragment},
  },
  converter::GenerationTarget,
};

#[derive(Clone, Debug)]
pub(crate) struct DefaultConstructorFragment(TypeRef);

impl DefaultConstructorFragment {
  pub(crate) fn new(type_token: TypeRef) -> Self {
    Self(type_token)
  }
}

impl ToTokens for DefaultConstructorFragment {
  fn to_tokens(&self, tokens: &mut TokenStream) {
    let base_type = &self.0.base_type;
    let constructor = quote! { #base_type::default() };

    let ts = if self.0.boxed {
      quote! { Box::new(#constructor) }
    } else {
      constructor
    };

    tokens.extend(ts);
  }
}

#[derive(Clone, Debug)]
pub(crate) struct EnumMethodFragment {
  vis: Visibility,
  method: EnumMethod,
}

impl EnumMethodFragment {
  pub(crate) fn new(vis: Visibility, method: EnumMethod) -> Self {
    Self { vis, method }
  }
}

impl HelperMethodParts for EnumMethodFragment {
  type Kind = EnumMethodKind;

  fn method(&self) -> EnumMethod {
    self.method.clone()
  }

  fn parameters(&self) -> impl ToTokens {
    match &self.method.kind {
      EnumMethodKind::ParameterizedConstructor {
        param_name, param_type, ..
      } => {
        let field = FieldDef::builder()
          .name(param_name.into())
          .rust_type(param_type.clone())
          .build();
        let parameter = FieldFunctionParameterFragment::new(field);
        quote! { #parameter }
      }
      _ => quote! {},
    }
  }

  fn implementation(&self) -> TokenStream {
    match &self.method.kind {
      EnumMethodKind::SimpleConstructor {
        variant_name,
        wrapped_type,
      } => {
        let constructor = DefaultConstructorFragment::new(wrapped_type.clone());
        quote! { Self::#variant_name(#constructor) }
      }
      EnumMethodKind::ParameterizedConstructor {
        variant_name,
        wrapped_type,
        param_name,
        param_type,
      } => {
        // TODO: pass in list of fields to detect need for Default
        let field = FieldDef::builder()
          .name(param_name.into())
          .rust_type(param_type.clone())
          .build();

        let constructor = StructConstructorFragment::new(wrapped_type.clone(), vec![field]);
        quote! { Self::#variant_name(#constructor) }
      }
      EnumMethodKind::KnownValueConstructor {
        known_type,
        known_variant,
      } => {
        quote! { Self::Known(#known_type::#known_variant) }
      }
    }
  }
}

impl ToTokens for EnumMethodFragment {
  fn to_tokens(&self, tokens: &mut TokenStream) {
    let helper_fragment = HelperMethodFragment::new(self.vis, self.clone());
    helper_fragment.to_tokens(tokens);
  }
}

#[derive(Clone, Debug)]
pub(crate) struct EnumMethodsImplFragment {
  name: EnumToken,
  methods: Vec<EnumMethodFragment>,
}

impl EnumMethodsImplFragment {
  pub(crate) fn new(name: EnumToken, vis: Visibility, methods: Vec<EnumMethod>) -> Self {
    let fragments = methods
      .into_iter()
      .map(|m| EnumMethodFragment::new(vis, m))
      .collect::<Vec<_>>();

    Self {
      name,
      methods: fragments,
    }
  }
}

impl ToTokens for EnumMethodsImplFragment {
  fn to_tokens(&self, tokens: &mut TokenStream) {
    if self.methods.is_empty() {
      return;
    }

    let name = &self.name;
    let methods = &self.methods;

    let ts = quote! {
      impl #name {
        #(#methods)*
      }
    };

    tokens.extend(ts);
  }
}

#[derive(Clone, Debug)]
pub(crate) struct EnumValueVariantFragment {
  name: EnumVariantToken,
  docs: TokenStream,
  serde_attrs: TokenStream,
  deprecated: TokenStream,
  default_attr: Option<TokenStream>,
  content: Option<TokenStream>,
}

impl EnumValueVariantFragment {
  pub(crate) fn new(variant: VariantDef, idx: usize, has_serde_derive: bool) -> Self {
    let docs = variant.docs.to_token_stream();
    let serde_attrs = if has_serde_derive {
      generate_serde_attrs(&variant.serde_attrs)
    } else {
      quote! {}
    };
    let deprecated = generate_deprecated_attr(variant.deprecated);
    let default_attr = (idx == 0).then(|| quote! { #[default] });
    let content = variant.content.tuple_types().map(|types| {
      let type_tokens = types.iter().map(|t| quote! { #t }).collect::<Vec<_>>();
      quote! { ( #(#type_tokens),* ) }
    });

    Self {
      name: variant.name,
      docs,
      serde_attrs,
      deprecated,
      default_attr,
      content,
    }
  }
}

impl ToTokens for EnumValueVariantFragment {
  fn to_tokens(&self, tokens: &mut TokenStream) {
    let name = &self.name;
    let docs = &self.docs;
    let serde_attrs = &self.serde_attrs;
    let deprecated = &self.deprecated;
    let default_attr = &self.default_attr;
    let content = &self.content;

    let ts = quote! {
      #docs
      #deprecated
      #serde_attrs
      #default_attr
      #name #content
    };

    tokens.extend(ts);
  }
}

#[derive(Clone, Debug)]
pub(crate) struct DisplayImplArmFragment {
  variant_name: EnumVariantToken,
  content: VariantContent,
  serde_name: String,
}

impl DisplayImplArmFragment {
  pub(crate) fn new(variant: VariantDef) -> Self {
    let serde_name = variant.serde_name();
    Self {
      variant_name: variant.name,
      content: variant.content,
      serde_name,
    }
  }
}

impl ToTokens for DisplayImplArmFragment {
  fn to_tokens(&self, tokens: &mut TokenStream) {
    let variant_name = &self.variant_name;

    let ts = match &self.content {
      VariantContent::Unit => {
        let serde_name = &self.serde_name;
        quote! { Self::#variant_name => write!(f, #serde_name), }
      }
      VariantContent::Tuple(_) => {
        quote! { Self::#variant_name(v) => write!(f, "{v}"), }
      }
    };

    tokens.extend(ts);
  }
}

#[derive(Clone, Debug)]
pub(crate) struct DisplayImplFragment {
  name: EnumToken,
  arms: Vec<DisplayImplArmFragment>,
}

impl DisplayImplFragment {
  pub(crate) fn new(name: EnumToken, variants: Vec<VariantDef>) -> Self {
    let arms = variants.into_iter().map(DisplayImplArmFragment::new).collect();
    Self { name, arms }
  }
}

impl ToTokens for DisplayImplFragment {
  fn to_tokens(&self, tokens: &mut TokenStream) {
    let name = &self.name;
    let arms = &self.arms;

    let ts = quote! {
      impl core::fmt::Display for #name {
        fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
          match self {
            #(#arms)*
          }
        }
      }
    };

    tokens.extend(ts);
  }
}

#[derive(Clone, Debug)]
pub(crate) struct FromStrImplArmFragment {
  variant_name: EnumVariantToken,
  serde_name: String,
}

impl FromStrImplArmFragment {
  pub(crate) fn new(variant: VariantDef) -> Self {
    let serde_name = variant.serde_name();
    Self {
      variant_name: variant.name,
      serde_name,
    }
  }
}

impl ToTokens for FromStrImplArmFragment {
  fn to_tokens(&self, tokens: &mut TokenStream) {
    let variant_name = &self.variant_name;
    let serde_name = &self.serde_name;

    let ts = quote! { #serde_name => Ok(Self::#variant_name), };
    tokens.extend(ts);
  }
}

#[derive(Clone, Debug)]
pub(crate) struct FromStrImplFragment {
  name: EnumToken,
  arms: Vec<FromStrImplArmFragment>,
  serde_names: Vec<String>,
}

impl FromStrImplFragment {
  pub(crate) fn new(name: EnumToken, variants: Vec<VariantDef>) -> Self {
    let (arms, serde_names): (Vec<_>, Vec<_>) = variants
      .into_iter()
      .filter(|v| matches!(v.content, VariantContent::Unit))
      .map(|v| {
        let serde_name = v.serde_name();
        (FromStrImplArmFragment::new(v), serde_name)
      })
      .unzip();

    Self {
      name,
      arms,
      serde_names,
    }
  }
}

impl ToTokens for FromStrImplFragment {
  fn to_tokens(&self, tokens: &mut TokenStream) {
    let name = &self.name;
    let arms = &self.arms;
    let serde_names = &self.serde_names;
    let expected = serde_names.join(", ");

    let ts = quote! {
      impl core::str::FromStr for #name {
        type Err = String;

        fn from_str(s: &str) -> core::result::Result<Self, Self::Err> {
          match s {
            #(#arms)*
            _ => Err(format!("unknown variant '{}', expected one of: {}", s, #expected)),
          }
        }
      }
    };

    tokens.extend(ts);
  }
}

#[derive(Clone, Debug)]
pub(crate) struct CaseInsensitiveDeserializeArmFragment {
  variant_name: EnumVariantToken,
  lower_val: String,
}

impl CaseInsensitiveDeserializeArmFragment {
  pub(crate) fn new(variant_name: EnumVariantToken, serde_name: &str) -> Self {
    Self {
      variant_name,
      lower_val: serde_name.to_ascii_lowercase(),
    }
  }
}

#[derive(Clone, Debug)]
pub(crate) struct CaseInsensitiveDeserializeImplFragment {
  name: EnumToken,
  arms: Vec<CaseInsensitiveDeserializeArmFragment>,
  serde_names: Vec<String>,
  fallback_variant: Option<EnumVariantToken>,
}

impl CaseInsensitiveDeserializeImplFragment {
  pub(crate) fn new(name: EnumToken, variants: Vec<VariantDef>, fallback_variant: Option<VariantDef>) -> Self {
    let (arms, serde_names): (Vec<_>, Vec<_>) = variants
      .into_iter()
      .map(|v| {
        let serde_name = v.serde_name();
        let arm = CaseInsensitiveDeserializeArmFragment::new(v.name, &serde_name);
        (arm, serde_name)
      })
      .unzip();

    Self {
      name,
      arms,
      serde_names,
      fallback_variant: fallback_variant.map(|v| v.name),
    }
  }
}

impl ToTokens for CaseInsensitiveDeserializeImplFragment {
  fn to_tokens(&self, tokens: &mut TokenStream) {
    let name = &self.name;

    let match_arms = self
      .arms
      .iter()
      .map(|arm| {
        let variant_name = &arm.variant_name;
        let lower_val = &arm.lower_val;
        quote! {
          #lower_val => Ok(#name::#variant_name),
        }
      })
      .collect::<Vec<TokenStream>>();

    let serde_names = &self.serde_names;
    let fallback_arm = if let Some(ref fb) = self.fallback_variant {
      quote! { _ => Ok(#name::#fb), }
    } else {
      quote! { _ => Err(serde::de::Error::unknown_variant(&s, &[ #(#serde_names),* ])), }
    };

    let ts = quote! {
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
    };

    tokens.extend(ts);
  }
}

#[derive(Clone, Debug)]
pub(crate) struct EnumFragment {
  def: EnumDef,
  vis: Visibility,
  target: GenerationTarget,
}

impl EnumFragment {
  pub(crate) fn new(def: EnumDef, visibility: Visibility, target: GenerationTarget) -> Self {
    Self {
      def,
      vis: visibility,
      target,
    }
  }
}

impl ToTokens for EnumFragment {
  fn to_tokens(&self, tokens: &mut TokenStream) {
    let name = &self.def.name;
    let docs = &self.def.docs;

    let derives = DeriveAttribute::new(self.def.derives());
    let outer_attrs = generate_outer_attrs(&self.def.outer_attrs);
    let serde_attrs = generate_serde_attrs(&self.def.serde_attrs);

    let has_serde_derive = self
      .def
      .derives()
      .iter()
      .any(|d| matches!(d, DeriveTrait::Serialize | DeriveTrait::Deserialize));

    let variants: Vec<EnumValueVariantFragment> = self
      .def
      .variants
      .iter()
      .enumerate()
      .map(|(idx, v)| EnumValueVariantFragment::new(v.clone(), idx, has_serde_derive))
      .collect();
    let variants = EnumVariants::new(variants);

    let methods = EnumMethodsImplFragment::new(name.clone(), self.vis, self.def.methods.clone());

    let vis = &self.vis;
    let enum_def = quote! {
      #docs
      #outer_attrs
      #derives
      #serde_attrs
      #vis enum #name {
        #variants
      }
      #methods
    };

    let display_impl = if self.def.generate_display {
      DisplayImplFragment::new(name.clone(), self.def.variants.clone()).to_token_stream()
    } else {
      quote! {}
    };

    let from_str_impl = if self.def.generate_display && self.def.is_simple() && self.target == GenerationTarget::Server
    {
      FromStrImplFragment::new(name.clone(), self.def.variants.clone()).to_token_stream()
    } else {
      quote! {}
    };

    let ts = if self.def.case_insensitive {
      let deserialize_impl = CaseInsensitiveDeserializeImplFragment::new(
        name.clone(),
        self.def.variants.clone(),
        self.def.fallback_variant().cloned(),
      );
      quote! {
        #enum_def
        #display_impl
        #from_str_impl
        #deserialize_impl
      }
    } else {
      quote! {
        #enum_def
        #display_impl
        #from_str_impl
      }
    };

    tokens.extend(ts);
  }
}

#[derive(Clone, Debug)]
pub(crate) struct DiscriminatedVariantFragment {
  variant_name: EnumVariantToken,
  type_name: TypeRef,
}

impl DiscriminatedVariantFragment {
  pub(crate) fn new(variant: DiscriminatedVariant) -> Self {
    Self {
      variant_name: variant.variant_name,
      type_name: variant.type_name,
    }
  }
}

impl ToTokens for DiscriminatedVariantFragment {
  fn to_tokens(&self, tokens: &mut TokenStream) {
    let variant_name = &self.variant_name;
    let type_name = &self.type_name;

    let ts = quote! { #variant_name(#type_name) };
    tokens.extend(ts);
  }
}

#[derive(Clone, Debug)]
pub(crate) struct DiscriminatedDefaultImplFragment {
  name: EnumToken,
  variant_ident: EnumVariantToken,
  type_tokens: TypeRef,
}

impl DiscriminatedDefaultImplFragment {
  pub(crate) fn new(name: EnumToken, default_variant: DiscriminatedVariant) -> Self {
    Self {
      name,
      variant_ident: default_variant.variant_name,
      type_tokens: default_variant.type_name,
    }
  }
}

impl ToTokens for DiscriminatedDefaultImplFragment {
  fn to_tokens(&self, tokens: &mut TokenStream) {
    let name = &self.name;
    let variant_ident = &self.variant_ident;
    let type_tokens = &self.type_tokens;

    let ts = quote! {
      impl Default for #name {
        fn default() -> Self {
          Self::#variant_ident(<#type_tokens>::default())
        }
      }
    };

    tokens.extend(ts);
  }
}

#[derive(Clone, Debug)]
pub(crate) struct DiscriminatedSerializeImplFragment {
  name: EnumToken,
  variant_names: Vec<EnumVariantToken>,
}

impl DiscriminatedSerializeImplFragment {
  pub(crate) fn new(name: EnumToken, variants: Vec<DiscriminatedVariant>) -> Self {
    let variant_names = variants.into_iter().map(|v| v.variant_name).collect::<Vec<_>>();
    Self { name, variant_names }
  }
}

impl ToTokens for DiscriminatedSerializeImplFragment {
  fn to_tokens(&self, tokens: &mut TokenStream) {
    let name = &self.name;

    let arms = self
      .variant_names
      .iter()
      .map(|variant_name| {
        quote! { Self::#variant_name(v) => v.serialize(serializer) }
      })
      .collect::<Vec<TokenStream>>();

    let ts = quote! {
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
    };

    tokens.extend(ts);
  }
}

#[derive(Clone, Debug)]
pub(crate) struct DiscriminatedDeserializeArmFragment {
  variant_name: EnumVariantToken,
  discriminator_values: Vec<String>,
}

impl DiscriminatedDeserializeArmFragment {
  pub(crate) fn new(variant: DiscriminatedVariant) -> Self {
    Self {
      variant_name: variant.variant_name,
      discriminator_values: variant.discriminator_values,
    }
  }
}

#[derive(Clone, Debug)]
pub(crate) struct DiscriminatedDeserializeImplFragment {
  name: EnumToken,
  discriminator_field: String,
  arms: Vec<DiscriminatedDeserializeArmFragment>,
  fallback_variant: Option<EnumVariantToken>,
}

impl DiscriminatedDeserializeImplFragment {
  pub(crate) fn new(
    name: EnumToken,
    discriminator_field: String,
    variants: Vec<DiscriminatedVariant>,
    fallback: Option<DiscriminatedVariant>,
  ) -> Self {
    let arms = variants
      .into_iter()
      .map(DiscriminatedDeserializeArmFragment::new)
      .collect();
    Self {
      name,
      discriminator_field,
      arms,
      fallback_variant: fallback.map(|f| f.variant_name),
    }
  }
}

impl ToTokens for DiscriminatedDeserializeImplFragment {
  fn to_tokens(&self, tokens: &mut TokenStream) {
    let name = &self.name;
    let disc_field = &self.discriminator_field;

    let variant_arms: Vec<TokenStream> = self
      .arms
      .iter()
      .flat_map(|arm| {
        let variant_name = &arm.variant_name;
        arm.discriminator_values.iter().map(move |disc_value| {
          quote! {
            Some(#disc_value) => serde_json::from_value(value)
              .map(Self::#variant_name)
              .map_err(serde::de::Error::custom)
          }
        })
      })
      .collect::<Vec<TokenStream>>();

    let none_handling = if let Some(ref fb) = self.fallback_variant {
      quote! {
        None => serde_json::from_value(value)
          .map(Self::#fb)
          .map_err(serde::de::Error::custom)
      }
    } else {
      quote! {
        None => Err(serde::de::Error::missing_field(Self::DISCRIMINATOR_FIELD))
      }
    };

    let ts = quote! {
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
    };

    tokens.extend(ts);
  }
}

#[derive(Clone, Debug)]
pub(crate) struct DiscriminatorConstImplFragment {
  name: EnumToken,
  vis: Visibility,
  discriminator_field: String,
}

impl DiscriminatorConstImplFragment {
  pub(crate) fn new(name: EnumToken, vis: Visibility, discriminator_field: String) -> Self {
    Self {
      name,
      vis,
      discriminator_field,
    }
  }
}

impl ToTokens for DiscriminatorConstImplFragment {
  fn to_tokens(&self, tokens: &mut TokenStream) {
    let name = &self.name;
    let vis = &self.vis;
    let disc_field = &self.discriminator_field;

    let ts = quote! {
      impl #name {
        #vis const DISCRIMINATOR_FIELD: &'static str = #disc_field;
      }
    };

    tokens.extend(ts);
  }
}

#[derive(Clone, Debug)]
pub(crate) struct DiscriminatedEnumFragment {
  def: DiscriminatedEnumDef,
  vis: Visibility,
}

impl DiscriminatedEnumFragment {
  pub(crate) fn new(def: DiscriminatedEnumDef, visibility: Visibility) -> Self {
    Self { def, vis: visibility }
  }
}

impl ToTokens for DiscriminatedEnumFragment {
  fn to_tokens(&self, tokens: &mut TokenStream) {
    let name = &self.def.name;
    let docs = &self.def.docs;

    let variants = self
      .def
      .all_variants()
      .map(|v| DiscriminatedVariantFragment::new(v.clone()))
      .collect::<Vec<DiscriminatedVariantFragment>>();
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

    let discriminator_const =
      DiscriminatorConstImplFragment::new(name.clone(), self.vis, self.def.discriminator_field.clone());

    let default_impl = self
      .def
      .default_variant()
      .map(|v| DiscriminatedDefaultImplFragment::new(name.clone(), v.clone()));

    let serialize_impl = matches!(self.def.serde_mode, SerdeMode::SerializeOnly | SerdeMode::Both).then(|| {
      DiscriminatedSerializeImplFragment::new(name.clone(), self.def.all_variants().cloned().collect::<Vec<_>>())
    });

    let deserialize_impl = matches!(self.def.serde_mode, SerdeMode::DeserializeOnly | SerdeMode::Both).then(|| {
      DiscriminatedDeserializeImplFragment::new(
        name.clone(),
        self.def.discriminator_field.clone(),
        self.def.variants.clone(),
        self.def.fallback.clone(),
      )
    });

    let methods_impl = EnumMethodsImplFragment::new(name.clone(), self.vis, self.def.methods.clone());

    let ts = quote! {
      #enum_def
      #discriminator_const
      #default_impl
      #serialize_impl
      #deserialize_impl
      #methods_impl
    };

    tokens.extend(ts);
  }
}

#[derive(Clone, Debug)]
pub struct ResponseEnumFragment {
  vis: Visibility,
  def: ResponseEnumDef,
}

impl ResponseEnumFragment {
  pub(crate) fn new(vis: Visibility, def: ResponseEnumDef) -> Self {
    Self { vis, def }
  }

  fn variants(&self) -> Vec<ResponseVariantFragment> {
    self
      .def
      .variants
      .iter()
      .cloned()
      .map(ResponseVariantFragment::new)
      .collect::<Vec<ResponseVariantFragment>>()
  }
}

impl ToTokens for ResponseEnumFragment {
  fn to_tokens(&self, tokens: &mut TokenStream) {
    let name = &self.def.name;
    let docs = &self.def.docs;
    let variants = EnumVariants::new(self.variants());
    let derives = DeriveAttribute::new(self.def.derives());
    let vis = &self.vis;

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

#[derive(Clone, Debug)]
pub(crate) struct ResponseVariantFragment {
  variant: ResponseVariant,
}

impl ResponseVariantFragment {
  pub(crate) fn new(variant: ResponseVariant) -> Self {
    Self { variant }
  }
}

impl ToTokens for ResponseVariantFragment {
  fn to_tokens(&self, tokens: &mut TokenStream) {
    let variant_name = &self.variant.variant_name;
    let doc_line = self.variant.doc_line();
    let content = self.variant.schema_type.as_ref().map(|schema| {
      quote! { (#schema) }
    });

    let ts = quote! {
      #[doc = #doc_line]
      #variant_name #content
    };

    tokens.extend(ts);
  }
}

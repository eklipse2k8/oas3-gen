use proc_macro2::{Ident, Literal, Span, TokenStream};
use quote::{ToTokens, TokenStreamExt as _, quote};

use super::{
  Visibility,
  attributes::{generate_deprecated_attr, generate_outer_attrs, generate_serde_attrs},
  coercion,
};
use crate::generator::{
  ast::{
    DeriveTrait, DerivesProvider, DiscriminatedEnumDef, DiscriminatedVariant, EnumDef, EnumDefault, EnumDefaultValue,
    EnumMethod, EnumMethodKind, EnumToken, EnumVariantToken, FieldDef, ResponseEnumDef, ResponseEnumVariant,
    ResponsePayload, ResponseUnionDef, RustPrimitive, SerdeAttribute, SerdeMode, TypeRef, VariantContent, VariantDef,
    types::generic_args,
  },
  codegen::{
    attributes::DeriveAttribute,
    methods::{FieldFunctionParameterFragment, HelperMethodFragment, HelperMethodParts, StructConstructorFragment},
  },
  converter::GenerationTarget,
};

#[derive(Clone, Copy, Debug)]
pub(crate) struct DefaultConstructorFragment<'a>(&'a TypeRef);

impl<'a> DefaultConstructorFragment<'a> {
  pub(crate) fn new(type_token: &'a TypeRef) -> Self {
    Self(type_token)
  }
}

impl ToTokens for DefaultConstructorFragment<'_> {
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

#[derive(Clone, Copy, Debug)]
pub(crate) struct EnumMethodFragment<'a> {
  vis: Visibility,
  method: &'a EnumMethod,
}

impl<'a> EnumMethodFragment<'a> {
  pub(crate) fn new(vis: Visibility, method: &'a EnumMethod) -> Self {
    Self { vis, method }
  }
}

impl HelperMethodParts for EnumMethodFragment<'_> {
  type Kind = EnumMethodKind;

  fn method(&self) -> &EnumMethod {
    self.method
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
        let parameter = FieldFunctionParameterFragment::new(&field);
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
        let constructor = DefaultConstructorFragment::new(wrapped_type);
        quote! { Self::#variant_name(#constructor) }
      }
      EnumMethodKind::ParameterizedConstructor {
        variant_name,
        wrapped_type,
        param_name,
        param_type,
      } => {
        // TODO: pass in list of fields to detect need for Default
        let fields = [FieldDef::builder()
          .name(param_name.into())
          .rust_type(param_type.clone())
          .build()];

        let constructor = StructConstructorFragment::new(wrapped_type, &fields);
        quote! { Self::#variant_name(#constructor) }
      }
      EnumMethodKind::KnownValueConstructor {
        wrapper_variant,
        known_type,
        known_variant,
      } => {
        quote! { Self::#wrapper_variant(#known_type::#known_variant) }
      }
    }
  }
}

impl ToTokens for EnumMethodFragment<'_> {
  fn to_tokens(&self, tokens: &mut TokenStream) {
    let helper_fragment = HelperMethodFragment::new(self.vis, self);
    helper_fragment.to_tokens(tokens);
  }
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct EnumDefaultImplFragment<'a> {
  name: &'a EnumToken,
  variant: &'a VariantDef,
  default: &'a EnumDefaultValue,
}

impl<'a> EnumDefaultImplFragment<'a> {
  pub(crate) fn new(name: &'a EnumToken, variant: &'a VariantDef, default: &'a EnumDefaultValue) -> Self {
    Self { name, variant, default }
  }
}

impl ToTokens for EnumDefaultImplFragment<'_> {
  fn to_tokens(&self, tokens: &mut TokenStream) {
    let name = self.name;
    let variant_name = &self.variant.name;

    let construction = if let Some(type_ref) = self.variant.content.single_type() {
      let payload = if let Some(inner) = &self.default.inner_variant {
        let enum_type = Ident::new(&type_ref.unboxed_base_type_name(), Span::call_site());
        quote! { #enum_type::#inner }
      } else {
        coercion::json_to_rust_literal(&self.default.value, type_ref)
      };

      if type_ref.boxed {
        quote! { Self::#variant_name(Box::new(#payload)) }
      } else {
        quote! { Self::#variant_name(#payload) }
      }
    } else {
      quote! { Self::#variant_name }
    };

    let ts = quote! {
      impl Default for #name {
        fn default() -> Self {
          #construction
        }
      }
    };

    tokens.extend(ts);
  }
}

#[derive(Clone, Debug)]
pub(crate) struct EnumMethodsImplFragment<'a> {
  name: &'a EnumToken,
  methods: Vec<EnumMethodFragment<'a>>,
}

impl<'a> EnumMethodsImplFragment<'a> {
  pub(crate) fn new(name: &'a EnumToken, vis: Visibility, methods: &'a [EnumMethod]) -> Self {
    let fragments = methods
      .iter()
      .map(|m| EnumMethodFragment::new(vis, m))
      .collect::<Vec<_>>();

    Self {
      name,
      methods: fragments,
    }
  }
}

impl ToTokens for EnumMethodsImplFragment<'_> {
  fn to_tokens(&self, tokens: &mut TokenStream) {
    if self.methods.is_empty() {
      return;
    }

    let name = self.name;
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
pub(crate) struct EnumValueVariantFragment<'a> {
  name: &'a EnumVariantToken,
  docs: TokenStream,
  serde_attrs: TokenStream,
  deprecated: TokenStream,
  default_attr: Option<TokenStream>,
  content: Option<TokenStream>,
}

impl<'a> EnumValueVariantFragment<'a> {
  pub(crate) fn new(variant: &'a VariantDef, is_default: bool, has_serde_derive: bool) -> Self {
    let docs = variant.docs.to_token_stream();
    let serde_attrs = if has_serde_derive {
      generate_serde_attrs(&variant.serde_attrs)
    } else {
      quote! {}
    };
    let deprecated = generate_deprecated_attr(variant.deprecated);
    let default_attr = is_default.then(|| quote! { #[default] });
    let content = variant.content.tuple_types().map(|types| {
      let type_tokens = types.iter().map(|t| quote! { #t }).collect::<Vec<_>>();
      quote! { ( #(#type_tokens),* ) }
    });

    Self {
      name: &variant.name,
      docs,
      serde_attrs,
      deprecated,
      default_attr,
      content,
    }
  }
}

impl ToTokens for EnumValueVariantFragment<'_> {
  fn to_tokens(&self, tokens: &mut TokenStream) {
    let name = self.name;
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

#[derive(Clone, Copy, Debug)]
pub(crate) struct DisplayImplArmFragment<'a> {
  variant_name: &'a EnumVariantToken,
  content: &'a VariantContent,
  serde_name: &'a str,
}

impl<'a> DisplayImplArmFragment<'a> {
  pub(crate) fn new(variant: &'a VariantDef) -> Self {
    Self {
      variant_name: &variant.name,
      content: &variant.content,
      serde_name: variant.serde_name(),
    }
  }
}

impl ToTokens for DisplayImplArmFragment<'_> {
  fn to_tokens(&self, tokens: &mut TokenStream) {
    let variant_name = self.variant_name;

    let ts = match self.content {
      VariantContent::Unit => {
        let serde_name = self.serde_name;
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
pub(crate) struct DisplayImplFragment<'a> {
  name: &'a EnumToken,
  arms: Vec<DisplayImplArmFragment<'a>>,
}

impl<'a> DisplayImplFragment<'a> {
  pub(crate) fn new(name: &'a EnumToken, variants: &'a [VariantDef]) -> Self {
    let arms = variants.iter().map(DisplayImplArmFragment::new).collect();
    Self { name, arms }
  }
}

impl ToTokens for DisplayImplFragment<'_> {
  fn to_tokens(&self, tokens: &mut TokenStream) {
    let name = self.name;
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

#[derive(Clone, Copy, Debug)]
pub(crate) struct FromStrImplArmFragment<'a> {
  variant_name: &'a EnumVariantToken,
  serde_name: &'a str,
}

impl<'a> FromStrImplArmFragment<'a> {
  pub(crate) fn new(variant: &'a VariantDef) -> Self {
    Self {
      variant_name: &variant.name,
      serde_name: variant.serde_name(),
    }
  }
}

impl ToTokens for FromStrImplArmFragment<'_> {
  fn to_tokens(&self, tokens: &mut TokenStream) {
    let variant_name = self.variant_name;
    let serde_name = self.serde_name;

    let ts = quote! { #serde_name => Ok(Self::#variant_name), };
    tokens.extend(ts);
  }
}

#[derive(Clone, Debug)]
pub(crate) struct FromStrImplFragment<'a> {
  name: &'a EnumToken,
  arms: Vec<FromStrImplArmFragment<'a>>,
  serde_names: Vec<&'a str>,
}

impl<'a> FromStrImplFragment<'a> {
  pub(crate) fn new(name: &'a EnumToken, variants: &'a [VariantDef]) -> Self {
    let (arms, serde_names): (Vec<_>, Vec<_>) = variants
      .iter()
      .filter(|v| matches!(v.content, VariantContent::Unit))
      .map(|v| (FromStrImplArmFragment::new(v), v.serde_name()))
      .unzip();

    Self {
      name,
      arms,
      serde_names,
    }
  }
}

impl ToTokens for FromStrImplFragment<'_> {
  fn to_tokens(&self, tokens: &mut TokenStream) {
    let name = self.name;
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
pub(crate) struct CaseInsensitiveDeserializeArmFragment<'a> {
  variant_name: &'a EnumVariantToken,
  lower_val: String,
}

impl<'a> CaseInsensitiveDeserializeArmFragment<'a> {
  pub(crate) fn new(variant_name: &'a EnumVariantToken, serde_name: &str) -> Self {
    Self {
      variant_name,
      lower_val: serde_name.to_ascii_lowercase(),
    }
  }
}

#[derive(Clone, Debug)]
pub(crate) struct CaseInsensitiveDeserializeImplFragment<'a> {
  name: &'a EnumToken,
  arms: Vec<CaseInsensitiveDeserializeArmFragment<'a>>,
  serde_names: Vec<&'a str>,
  fallback_variant: Option<&'a EnumVariantToken>,
}

impl<'a> CaseInsensitiveDeserializeImplFragment<'a> {
  pub(crate) fn new(name: &'a EnumToken, variants: &'a [VariantDef], fallback_variant: Option<&'a VariantDef>) -> Self {
    let (arms, serde_names): (Vec<_>, Vec<_>) = variants
      .iter()
      .map(|v| {
        let serde_name = v.serde_name();
        let arm = CaseInsensitiveDeserializeArmFragment::new(&v.name, serde_name);
        (arm, serde_name)
      })
      .unzip();

    Self {
      name,
      arms,
      serde_names,
      fallback_variant: fallback_variant.map(|v| &v.name),
    }
  }
}

impl ToTokens for CaseInsensitiveDeserializeImplFragment<'_> {
  fn to_tokens(&self, tokens: &mut TokenStream) {
    let name = self.name;

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
    let fallback_arm = if let Some(fb) = self.fallback_variant {
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
pub(crate) struct NumericEnumSerdeImplFragment<'a> {
  name: &'a EnumToken,
  is_float: bool,
  wire_type: Ident,
  serialize_method: Ident,
  arms: Vec<(&'a EnumVariantToken, Literal)>,
  expected: String,
}

impl<'a> NumericEnumSerdeImplFragment<'a> {
  pub(crate) fn new(name: &'a EnumToken, primitive: &RustPrimitive, variants: &'a [VariantDef]) -> Self {
    let is_float = primitive.is_float();
    let is_unsigned = primitive.is_unsigned_integer();

    let (wire_name, serialize_name) = if is_float {
      ("f64", "serialize_f64")
    } else if is_unsigned {
      ("u64", "serialize_u64")
    } else {
      ("i64", "serialize_i64")
    };

    let mut arms = vec![];
    let mut expected_values = vec![];
    for variant in variants {
      let raw = variant.serde_name();
      let literal = if is_float {
        raw.parse::<f64>().ok().map(Literal::f64_suffixed)
      } else if is_unsigned {
        raw.parse::<u64>().ok().map(Literal::u64_suffixed)
      } else {
        raw.parse::<i64>().ok().map(Literal::i64_suffixed)
      };
      if let Some(literal) = literal {
        expected_values.push(raw);
        arms.push((&variant.name, literal));
      }
    }

    Self {
      name,
      is_float,
      wire_type: Ident::new(wire_name, Span::call_site()),
      serialize_method: Ident::new(serialize_name, Span::call_site()),
      arms,
      expected: expected_values.join(", "),
    }
  }
}

impl ToTokens for NumericEnumSerdeImplFragment<'_> {
  fn to_tokens(&self, tokens: &mut TokenStream) {
    let name = self.name;
    let wire_type = &self.wire_type;
    let serialize_method = &self.serialize_method;
    let expected = &self.expected;

    let serialize_arms = self
      .arms
      .iter()
      .map(|(variant_name, literal)| quote! { Self::#variant_name => #literal, })
      .collect::<Vec<TokenStream>>();

    let deserialize_body = if self.is_float {
      let arms = self
        .arms
        .iter()
        .map(|(variant_name, literal)| quote! { bits if bits == (#literal).to_bits() => Ok(Self::#variant_name), })
        .collect::<Vec<TokenStream>>();
      quote! {
        let value = #wire_type::deserialize(deserializer)?;
        match value.to_bits() {
          #(#arms)*
          _ => Err(serde::de::Error::custom(format!("unknown variant {}, expected one of: {}", value, #expected))),
        }
      }
    } else {
      let arms = self
        .arms
        .iter()
        .map(|(variant_name, literal)| quote! { #literal => Ok(Self::#variant_name), })
        .collect::<Vec<TokenStream>>();
      quote! {
        let value = #wire_type::deserialize(deserializer)?;
        match value {
          #(#arms)*
          _ => Err(serde::de::Error::custom(format!("unknown variant {}, expected one of: {}", value, #expected))),
        }
      }
    };

    let ts = quote! {
      impl serde::Serialize for #name {
        fn serialize<S>(&self, serializer: S) -> core::result::Result<S::Ok, S::Error>
        where
          S: serde::Serializer,
        {
          let value: #wire_type = match self {
            #(#serialize_arms)*
          };
          serializer.#serialize_method(value)
        }
      }

      impl<'de> serde::Deserialize<'de> for #name {
        fn deserialize<D>(deserializer: D) -> core::result::Result<Self, D::Error>
        where
          D: serde::Deserializer<'de>,
        {
          #deserialize_body
        }
      }
    };

    tokens.extend(ts);
  }
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct EnumFragment<'a> {
  def: &'a EnumDef,
  vis: Visibility,
  target: GenerationTarget,
}

impl<'a> EnumFragment<'a> {
  pub(crate) fn new(def: &'a EnumDef, visibility: Visibility, target: GenerationTarget) -> Self {
    Self {
      def,
      vis: visibility,
      target,
    }
  }
}

impl ToTokens for EnumFragment<'_> {
  fn to_tokens(&self, tokens: &mut TokenStream) {
    let name = &self.def.name;
    let docs = &self.def.docs;

    let derive_traits = self.def.derives();
    let has_serde_derive = derive_traits
      .iter()
      .any(|d| matches!(d, DeriveTrait::Serialize | DeriveTrait::Deserialize));

    let derives = DeriveAttribute::new(derive_traits);
    let outer_attrs = generate_outer_attrs(&self.def.outer_attrs);
    let serde_attrs = generate_serde_attrs(&self.def.serde_attrs);

    let default_idx = match &self.def.default_mode {
      EnumDefault::Derive => Some(self.def.variants.iter().position(|v| v.default).unwrap_or(0)),
      EnumDefault::Value(_) | EnumDefault::None => None,
    };
    let variants: Vec<EnumValueVariantFragment<'_>> = self
      .def
      .variants
      .iter()
      .enumerate()
      .map(|(idx, v)| EnumValueVariantFragment::new(v, Some(idx) == default_idx, has_serde_derive))
      .collect();
    let variants = EnumVariants::new(variants);

    let methods = EnumMethodsImplFragment::new(name, self.vis, &self.def.methods);

    let default_impl = match &self.def.default_mode {
      EnumDefault::Value(default) => self
        .def
        .variants
        .iter()
        .find(|v| v.default)
        .map(|v| EnumDefaultImplFragment::new(name, v, default).to_token_stream()),
      EnumDefault::Derive | EnumDefault::None => None,
    };

    let vis = &self.vis;
    let enum_def = quote! {
      #docs
      #outer_attrs
      #derives
      #serde_attrs
      #vis enum #name {
        #variants
      }
      #default_impl
      #methods
    };

    let display_impl = if self.def.generate_display {
      DisplayImplFragment::new(name, &self.def.variants).to_token_stream()
    } else {
      quote! {}
    };

    let from_str_impl = if self.def.generate_display && self.def.is_simple() && self.target == GenerationTarget::Server
    {
      FromStrImplFragment::new(name, &self.def.variants).to_token_stream()
    } else {
      quote! {}
    };

    let ts = if let Some(primitive) = &self.def.scalar_repr {
      let serde_impl = NumericEnumSerdeImplFragment::new(name, primitive, &self.def.variants);
      quote! {
        #enum_def
        #display_impl
        #from_str_impl
        #serde_impl
      }
    } else if self.def.case_insensitive {
      let deserialize_impl =
        CaseInsensitiveDeserializeImplFragment::new(name, &self.def.variants, self.def.fallback_variant());
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

#[derive(Clone, Copy, Debug)]
pub(crate) struct DiscriminatedVariantFragment<'a> {
  variant_name: &'a EnumVariantToken,
  type_name: &'a TypeRef,
}

impl<'a> DiscriminatedVariantFragment<'a> {
  pub(crate) fn new(variant: &'a DiscriminatedVariant) -> Self {
    Self {
      variant_name: &variant.variant_name,
      type_name: &variant.type_name,
    }
  }
}

impl ToTokens for DiscriminatedVariantFragment<'_> {
  fn to_tokens(&self, tokens: &mut TokenStream) {
    let variant_name = self.variant_name;
    let type_name = self.type_name;

    let ts = quote! { #variant_name(#type_name) };
    tokens.extend(ts);
  }
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct DiscriminatedDefaultImplFragment<'a> {
  name: &'a EnumToken,
  variant_ident: &'a EnumVariantToken,
  type_tokens: &'a TypeRef,
}

impl<'a> DiscriminatedDefaultImplFragment<'a> {
  pub(crate) fn new(name: &'a EnumToken, default_variant: &'a DiscriminatedVariant) -> Self {
    Self {
      name,
      variant_ident: &default_variant.variant_name,
      type_tokens: &default_variant.type_name,
    }
  }
}

impl ToTokens for DiscriminatedDefaultImplFragment<'_> {
  fn to_tokens(&self, tokens: &mut TokenStream) {
    let name = self.name;
    let variant_ident = self.variant_ident;
    let type_tokens = self.type_tokens;

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
pub(crate) struct DiscriminatedSerializeImplFragment<'a> {
  name: &'a EnumToken,
  variant_names: Vec<&'a EnumVariantToken>,
}

impl<'a> DiscriminatedSerializeImplFragment<'a> {
  pub(crate) fn new(name: &'a EnumToken, variants: impl Iterator<Item = &'a DiscriminatedVariant>) -> Self {
    let variant_names = variants.map(|v| &v.variant_name).collect::<Vec<_>>();
    Self { name, variant_names }
  }
}

impl ToTokens for DiscriminatedSerializeImplFragment<'_> {
  fn to_tokens(&self, tokens: &mut TokenStream) {
    let name = self.name;

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

#[derive(Clone, Copy, Debug)]
pub(crate) struct DiscriminatedDeserializeArmFragment<'a> {
  variant_name: &'a EnumVariantToken,
  discriminator_values: &'a [String],
}

impl<'a> DiscriminatedDeserializeArmFragment<'a> {
  pub(crate) fn new(variant: &'a DiscriminatedVariant) -> Self {
    Self {
      variant_name: &variant.variant_name,
      discriminator_values: &variant.discriminator_values,
    }
  }
}

#[derive(Clone, Debug)]
pub(crate) struct DiscriminatedDeserializeImplFragment<'a> {
  name: &'a EnumToken,
  discriminator_field: &'a str,
  arms: Vec<DiscriminatedDeserializeArmFragment<'a>>,
  fallback_variant: Option<&'a EnumVariantToken>,
}

impl<'a> DiscriminatedDeserializeImplFragment<'a> {
  pub(crate) fn new(
    name: &'a EnumToken,
    discriminator_field: &'a str,
    variants: &'a [DiscriminatedVariant],
    fallback: Option<&'a DiscriminatedVariant>,
  ) -> Self {
    let arms = variants.iter().map(DiscriminatedDeserializeArmFragment::new).collect();
    Self {
      name,
      discriminator_field,
      arms,
      fallback_variant: fallback.map(|f| &f.variant_name),
    }
  }
}

impl ToTokens for DiscriminatedDeserializeImplFragment<'_> {
  fn to_tokens(&self, tokens: &mut TokenStream) {
    let name = self.name;
    let disc_field = self.discriminator_field;

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

    let none_handling = if let Some(fb) = self.fallback_variant {
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

#[derive(Clone, Copy, Debug)]
pub(crate) struct DiscriminatorConstImplFragment<'a> {
  name: &'a EnumToken,
  vis: Visibility,
  discriminator_field: &'a str,
}

impl<'a> DiscriminatorConstImplFragment<'a> {
  pub(crate) fn new(name: &'a EnumToken, vis: Visibility, discriminator_field: &'a str) -> Self {
    Self {
      name,
      vis,
      discriminator_field,
    }
  }
}

impl ToTokens for DiscriminatorConstImplFragment<'_> {
  fn to_tokens(&self, tokens: &mut TokenStream) {
    let name = self.name;
    let vis = &self.vis;
    let disc_field = self.discriminator_field;

    let ts = quote! {
      impl #name {
        #vis const DISCRIMINATOR_FIELD: &'static str = #disc_field;
      }
    };

    tokens.extend(ts);
  }
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct DiscriminatedEnumFragment<'a> {
  def: &'a DiscriminatedEnumDef,
  vis: Visibility,
}

impl<'a> DiscriminatedEnumFragment<'a> {
  pub(crate) fn new(def: &'a DiscriminatedEnumDef, visibility: Visibility) -> Self {
    Self { def, vis: visibility }
  }
}

impl ToTokens for DiscriminatedEnumFragment<'_> {
  fn to_tokens(&self, tokens: &mut TokenStream) {
    let name = &self.def.name;
    let docs = &self.def.docs;

    let variants = self
      .def
      .all_variants()
      .map(DiscriminatedVariantFragment::new)
      .collect::<Vec<DiscriminatedVariantFragment<'_>>>();
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

    let discriminator_const = DiscriminatorConstImplFragment::new(name, self.vis, &self.def.discriminator_field);

    let default_impl = self
      .def
      .default_variant()
      .map(|v| DiscriminatedDefaultImplFragment::new(name, v));

    let serialize_impl = matches!(self.def.serde_mode, SerdeMode::SerializeOnly | SerdeMode::Both)
      .then(|| DiscriminatedSerializeImplFragment::new(name, self.def.all_variants()));

    let deserialize_impl = matches!(self.def.serde_mode, SerdeMode::DeserializeOnly | SerdeMode::Both).then(|| {
      DiscriminatedDeserializeImplFragment::new(
        name,
        &self.def.discriminator_field,
        &self.def.variants,
        self.def.fallback.as_ref(),
      )
    });

    let methods_impl = EnumMethodsImplFragment::new(name, self.vis, &self.def.methods);

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

/// Emits the shared response enum, e.g. `pub enum ApiResponse<Value, Failure> { .. }`.
#[derive(Clone, Copy, Debug)]
pub struct ResponseEnumFragment<'a> {
  vis: Visibility,
  def: &'a ResponseEnumDef,
}

impl<'a> ResponseEnumFragment<'a> {
  pub(crate) fn new(vis: Visibility, def: &'a ResponseEnumDef) -> Self {
    Self { vis, def }
  }

  fn generics(&self) -> TokenStream {
    generic_args(
      [ResponsePayload::Value, ResponsePayload::Failure]
        .into_iter()
        .filter(|payload| self.def.has_param(*payload))
        .map(|payload| payload.to_token_stream()),
    )
  }
}

impl ToTokens for ResponseEnumFragment<'_> {
  fn to_tokens(&self, tokens: &mut TokenStream) {
    let name = &self.def.name;
    let docs = &self.def.docs;
    let generics = self.generics();
    let variants = EnumVariants::new(
      self
        .def
        .variants
        .iter()
        .map(ResponseEnumVariantFragment::new)
        .collect::<Vec<_>>(),
    );
    let derives = DeriveAttribute::new(self.def.derives());
    let vis = &self.vis;

    let ts = quote! {
      #docs
      #derives
      #vis enum #name #generics {
        #variants
      }
    };

    tokens.extend(ts);
  }
}

/// Emits an untagged enum over the body types an operation shares in one status class.
#[derive(Clone, Copy, Debug)]
pub struct ResponseUnionFragment<'a> {
  vis: Visibility,
  def: &'a ResponseUnionDef,
}

impl<'a> ResponseUnionFragment<'a> {
  pub(crate) fn new(vis: Visibility, def: &'a ResponseUnionDef) -> Self {
    Self { vis, def }
  }
}

impl ToTokens for ResponseUnionFragment<'_> {
  fn to_tokens(&self, tokens: &mut TokenStream) {
    let name = &self.def.name;
    let docs = &self.def.docs;
    let derives = DeriveAttribute::new(self.def.derives());
    let vis = &self.vis;

    let derive_traits = self.def.derives();
    let has_serde_derive = derive_traits
      .iter()
      .any(|d| matches!(d, DeriveTrait::Serialize | DeriveTrait::Deserialize));
    let untagged = has_serde_derive.then(|| generate_serde_attrs(&[SerdeAttribute::Untagged]));

    let variants = self.def.variants.iter().map(|variant| {
      let variant_name = &variant.name;
      let rust_type = &variant.rust_type;
      quote! { #variant_name(#rust_type) }
    });

    let ts = quote! {
      #docs
      #derives
      #untagged
      #vis enum #name {
        #(#variants),*
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

#[derive(Clone, Copy, Debug)]
pub(crate) struct ResponseEnumVariantFragment<'a> {
  variant: &'a ResponseEnumVariant,
}

impl<'a> ResponseEnumVariantFragment<'a> {
  pub(crate) fn new(variant: &'a ResponseEnumVariant) -> Self {
    Self { variant }
  }
}

impl ToTokens for ResponseEnumVariantFragment<'_> {
  fn to_tokens(&self, tokens: &mut TokenStream) {
    let variant_name = &self.variant.name;
    let doc_line = self.variant.doc_line();

    let status = self
      .variant
      .status_code
      .carries_status()
      .then(|| quote! { http::StatusCode });
    let body = match self.variant.payload {
      ResponsePayload::None => None,
      ResponsePayload::Value | ResponsePayload::Failure => Some(self.variant.payload.to_token_stream()),
      ResponsePayload::Raw => Some(quote! { Vec<u8> }),
    };
    let fields = status.into_iter().chain(body).collect::<Vec<_>>();
    let content = (!fields.is_empty()).then(|| quote! { (#(#fields),*) });

    let ts = quote! {
      #[doc = #doc_line]
      #variant_name #content
    };

    tokens.extend(ts);
  }
}

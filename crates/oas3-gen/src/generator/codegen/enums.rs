use std::borrow::Cow;

use itertools::Itertools;
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
    ResponsePayload, ResponseUnionDef, RustPrimitive, SerdeAttribute, SerdeMode, StructToken, TypeRef, ValueKind,
    VariantContent, VariantDef, types::generic_args,
  },
  codegen::{
    attributes::DeriveAttribute,
    methods::{FieldFunctionParameterFragment, HelperMethodFragment, HelperMethodParts, StructConstructorFragment},
  },
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
    let construction = EnumValueFragment::new(
      quote! { Self },
      &self.variant.name,
      self.variant.content.single_type(),
      &self.default.value,
      self.default.inner_variant.as_ref(),
    );

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

/// Expression constructing one enum value, such as `Owner::Variant(Inner::Value)`.
///
/// A unit variant ignores `value`. A tuple variant wraps `inner_variant` of the wrapped value
/// enum when one is given, otherwise `value` coerced to the wrapped type.
#[derive(Clone, Debug)]
pub(crate) struct EnumValueFragment<'a> {
  owner: TokenStream,
  variant: &'a EnumVariantToken,
  wrapped_type: Option<&'a TypeRef>,
  value: &'a serde_json::Value,
  inner_variant: Option<&'a EnumVariantToken>,
}

impl<'a> EnumValueFragment<'a> {
  pub(crate) fn new(
    owner: TokenStream,
    variant: &'a EnumVariantToken,
    wrapped_type: Option<&'a TypeRef>,
    value: &'a serde_json::Value,
    inner_variant: Option<&'a EnumVariantToken>,
  ) -> Self {
    Self {
      owner,
      variant,
      wrapped_type,
      value,
      inner_variant,
    }
  }
}

impl ToTokens for EnumValueFragment<'_> {
  fn to_tokens(&self, tokens: &mut TokenStream) {
    let owner = &self.owner;
    let variant = self.variant;

    let Some(type_ref) = self.wrapped_type else {
      tokens.extend(quote! { #owner::#variant });
      return;
    };

    let payload = if let Some(inner) = self.inner_variant {
      let enum_type = &type_ref.base_type;
      quote! { #enum_type::#inner }
    } else {
      coercion::json_to_rust_literal(self.value, type_ref)
    };

    let ts = if type_ref.boxed {
      quote! { #owner::#variant(Box::new(#payload)) }
    } else {
      quote! { #owner::#variant(#payload) }
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

/// Match arm selecting a unit variant by any of its wire values, e.g.
/// `"active" | "on" => Ok(Self::Active),`.
///
/// A non-exhaustive enum's parse can't fail, so its arms evaluate to the variant
/// itself: `"active" | "on" => Self::Active,`.
#[derive(Clone, Debug)]
pub(crate) struct WireValueArmFragment<'a> {
  variant_name: &'a EnumVariantToken,
  wire_values: Vec<Cow<'a, str>>,
  infallible: bool,
}

impl<'a> WireValueArmFragment<'a> {
  /// Builds one arm per unit variant of `def`, matching its serde name and aliases.
  ///
  /// For a case-insensitive enum, the values are ASCII-lowercased. A value an earlier
  /// arm already matches is dropped, so no pattern is unreachable.
  pub(crate) fn for_enum(def: &'a EnumDef) -> Vec<Self> {
    def
      .variants
      .iter()
      .filter(|v| matches!(v.content, VariantContent::Unit))
      .flat_map(|variant| {
        variant.wire_values().map(move |value| {
          let value = if def.case_insensitive && value.bytes().any(|b| b.is_ascii_uppercase()) {
            Cow::Owned(value.to_ascii_lowercase())
          } else {
            Cow::Borrowed(value)
          };
          (&variant.name, value)
        })
      })
      .unique_by(|(_, value)| value.clone())
      .chunk_by(|(variant_name, _)| *variant_name)
      .into_iter()
      .map(|(variant_name, arm)| Self {
        variant_name,
        wire_values: arm.map(|(_, value)| value).collect(),
        infallible: def.non_exhaustive,
      })
      .collect()
  }
}

impl ToTokens for WireValueArmFragment<'_> {
  fn to_tokens(&self, tokens: &mut TokenStream) {
    let variant_name = self.variant_name;
    let wire_values = &self.wire_values;

    let result = if self.infallible {
      quote! { Self::#variant_name }
    } else {
      quote! { Ok(Self::#variant_name) }
    };

    tokens.extend(quote! { #(#wire_values)|* => #result, });
  }
}

/// `impl From<String>` for a non-exhaustive enum: a listed value selects its variant,
/// and any other string lands in the catch-all, e.g. `Other(value)`.
#[derive(Clone, Copy, Debug)]
pub(crate) struct FromStringImplFragment<'a>(&'a EnumDef);

impl<'a> FromStringImplFragment<'a> {
  pub(crate) fn new(def: &'a EnumDef) -> Self {
    Self(def)
  }
}

impl ToTokens for FromStringImplFragment<'_> {
  fn to_tokens(&self, tokens: &mut TokenStream) {
    let Some(other) = self.0.other_variant() else {
      return;
    };
    let name = &self.0.name;
    let other = &other.name;
    let arms = WireValueArmFragment::for_enum(self.0);
    let subject = if self.0.case_insensitive {
      quote! { value.to_ascii_lowercase().as_str() }
    } else {
      quote! { value.as_str() }
    };

    tokens.extend(quote! {
      impl From<String> for #name {
        fn from(value: String) -> Self {
          match #subject {
            #(#arms)*
            _ => Self::#other(value),
          }
        }
      }
    });
  }
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct FromStrImplFragment<'a>(&'a EnumDef);

impl<'a> FromStrImplFragment<'a> {
  pub(crate) fn new(def: &'a EnumDef) -> Self {
    Self(def)
  }
}

impl ToTokens for FromStrImplFragment<'_> {
  fn to_tokens(&self, tokens: &mut TokenStream) {
    let name = &self.0.name;

    let body = if self.0.non_exhaustive {
      quote! { Ok(Self::from(s.to_string())) }
    } else {
      let arms = WireValueArmFragment::for_enum(self.0);
      let subject = if self.0.case_insensitive {
        quote! { s.to_ascii_lowercase().as_str() }
      } else {
        quote! { s }
      };
      let expected = self.0.variants.iter().map(VariantDef::serde_name).join(", ");
      quote! {
        match #subject {
          #(#arms)*
          _ => Err(format!("unknown variant '{}', expected one of: {}", s, #expected)),
        }
      }
    };

    let ts = quote! {
      impl core::str::FromStr for #name {
        type Err = String;

        fn from_str(s: &str) -> core::result::Result<Self, Self::Err> {
          #body
        }
      }
    };

    tokens.extend(ts);
  }
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct CaseInsensitiveDeserializeImplFragment<'a>(&'a EnumDef);

impl<'a> CaseInsensitiveDeserializeImplFragment<'a> {
  pub(crate) fn new(def: &'a EnumDef) -> Self {
    Self(def)
  }
}

impl ToTokens for CaseInsensitiveDeserializeImplFragment<'_> {
  fn to_tokens(&self, tokens: &mut TokenStream) {
    let name = &self.0.name;

    let body = if self.0.non_exhaustive {
      quote! { String::deserialize(deserializer).map(Self::from) }
    } else {
      let arms = WireValueArmFragment::for_enum(self.0);
      let fallback_arm = if let Some(fallback) = self.0.fallback_variant() {
        let fallback = &fallback.name;
        quote! { _ => Ok(Self::#fallback), }
      } else {
        let serde_names = self.0.variants.iter().map(VariantDef::serde_name);
        quote! { _ => Err(serde::de::Error::unknown_variant(&s, &[ #(#serde_names),* ])), }
      };
      quote! {
        let s = String::deserialize(deserializer)?;
        match s.to_ascii_lowercase().as_str() {
          #(#arms)*
          #fallback_arm
        }
      }
    };

    let ts = quote! {
      impl<'de> serde::Deserialize<'de> for #name {
        fn deserialize<D>(deserializer: D) -> core::result::Result<Self, D::Error>
        where
          D: serde::Deserializer<'de>,
        {
          #body
        }
      }
    };

    tokens.extend(ts);
  }
}

/// `Serialize` and `Deserialize` for a value enum whose values share one JSON scalar
/// type: integers, floats, or booleans.
#[derive(Clone, Debug)]
pub(crate) struct ScalarEnumSerdeImplFragment<'a> {
  name: &'a EnumToken,
  primitive: &'a RustPrimitive,
  arms: Vec<(&'a VariantDef, TokenStream)>,
  expected: String,
}

impl<'a> ScalarEnumSerdeImplFragment<'a> {
  pub(crate) fn new(name: &'a EnumToken, primitive: &'a RustPrimitive, variants: &'a [VariantDef]) -> Self {
    let mut arms = vec![];
    let mut expected_values = vec![];
    for variant in variants {
      let raw = variant.serde_name();
      let literal = if *primitive == RustPrimitive::Bool {
        raw.parse::<bool>().ok().map(|value| quote! { #value })
      } else if primitive.is_float() {
        raw
          .parse::<f64>()
          .ok()
          .map(|value| Literal::f64_suffixed(value).into_token_stream())
      } else if primitive.is_unsigned_integer() {
        raw
          .parse::<u64>()
          .ok()
          .map(|value| Literal::u64_suffixed(value).into_token_stream())
      } else {
        raw
          .parse::<i64>()
          .ok()
          .map(|value| Literal::i64_suffixed(value).into_token_stream())
      };
      if let Some(literal) = literal {
        expected_values.push(raw);
        arms.push((variant, literal));
      }
    }

    Self {
      name,
      primitive,
      arms,
      expected: expected_values.join(", "),
    }
  }

  fn wire_type(&self) -> (Ident, Ident) {
    let (wire_name, serialize_name) = if *self.primitive == RustPrimitive::Bool {
      ("bool", "serialize_bool")
    } else if self.primitive.is_float() {
      ("f64", "serialize_f64")
    } else if self.primitive.is_unsigned_integer() {
      ("u64", "serialize_u64")
    } else {
      ("i64", "serialize_i64")
    };
    (
      Ident::new(wire_name, Span::call_site()),
      Ident::new(serialize_name, Span::call_site()),
    )
  }

  fn deserialize_body(&self, wire_type: &Ident) -> TokenStream {
    let expected = &self.expected;
    let unknown = quote! {
      Err(serde::de::Error::custom(format!("unknown variant {}, expected one of: {}", value, #expected)))
    };

    if *self.primitive == RustPrimitive::Bool {
      let outcome = |wanted: &str| {
        self
          .arms
          .iter()
          .find(|(variant, _)| variant.serde_name() == wanted)
          .map_or_else(
            || unknown.clone(),
            |(variant, _)| {
              let variant_name = &variant.name;
              quote! { Ok(Self::#variant_name) }
            },
          )
      };
      let (on_true, on_false) = (outcome("true"), outcome("false"));
      return quote! {
        let value = bool::deserialize(deserializer)?;
        if value { #on_true } else { #on_false }
      };
    }

    let (subject, arms) = if self.primitive.is_float() {
      let arms = self
        .arms
        .iter()
        .map(|(variant, literal)| {
          let variant_name = &variant.name;
          quote! { bits if bits == (#literal).to_bits() => Ok(Self::#variant_name), }
        })
        .collect::<Vec<TokenStream>>();
      (quote! { value.to_bits() }, arms)
    } else {
      let arms = self
        .arms
        .iter()
        .map(|(variant, literal)| {
          let variant_name = &variant.name;
          quote! { #literal => Ok(Self::#variant_name), }
        })
        .collect::<Vec<TokenStream>>();
      (quote! { value }, arms)
    };

    quote! {
      let value = #wire_type::deserialize(deserializer)?;
      match #subject {
        #(#arms)*
        _ => #unknown,
      }
    }
  }
}

impl ToTokens for ScalarEnumSerdeImplFragment<'_> {
  fn to_tokens(&self, tokens: &mut TokenStream) {
    let name = self.name;
    let (wire_type, serialize_method) = self.wire_type();
    let deserialize_body = self.deserialize_body(&wire_type);

    let serialize_arms = self
      .arms
      .iter()
      .map(|(variant, literal)| {
        let variant_name = &variant.name;
        quote! { Self::#variant_name => #literal, }
      })
      .collect::<Vec<TokenStream>>();

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

/// JSON literal a variant of a mixed-type value enum stands for.
#[derive(Clone, Copy, Debug)]
enum MixedLiteral {
  Bool(bool),
  Integer(i64),
  Float(f64),
  String,
}

/// `f64` literal with the digit separators `clippy::unreadable_literal` asks for. `f64`
/// display never uses exponent notation.
fn float_literal(value: f64) -> Literal {
  let sign = if value.is_sign_negative() { "-" } else { "" };
  let digits = value.abs().to_string();
  let (integer, fraction) = digits.split_once('.').unwrap_or((digits.as_str(), ""));
  let integer = integer
    .as_bytes()
    .rchunks(3)
    .rev()
    .map(String::from_utf8_lossy)
    .join("_");
  let fraction = fraction.as_bytes().chunks(3).map(String::from_utf8_lossy).join("_");
  let text = if fraction.is_empty() {
    format!("{sign}{integer}f64")
  } else {
    format!("{sign}{integer}.{fraction}f64")
  };
  text
    .parse()
    .expect("a decimal f64 with digit separators is a valid literal")
}

impl From<&VariantDef> for MixedLiteral {
  fn from(variant: &VariantDef) -> Self {
    let raw = variant.serde_name();
    let literal = match variant.value_kind {
      ValueKind::Bool => raw.parse().ok().map(Self::Bool),
      ValueKind::Number => raw
        .parse()
        .ok()
        .map(Self::Integer)
        .or_else(|| raw.parse().ok().map(Self::Float)),
      ValueKind::String => None,
    };
    literal.unwrap_or(Self::String)
  }
}

/// `Serialize` and `Deserialize` for a value enum whose values mix JSON types, such
/// as `"auto"`, `1`, and `true`. Each variant reads and writes its own JSON type. Numeric
/// and boolean variants also match a string that parses to their value, since query
/// strings and path segments send every value as text.
#[derive(Clone, Copy, Debug)]
pub(crate) struct MixedEnumSerdeImplFragment<'a>(&'a EnumDef);

impl<'a> MixedEnumSerdeImplFragment<'a> {
  pub(crate) fn new(def: &'a EnumDef) -> Self {
    Self(def)
  }

  fn serialize_arm(variant: &VariantDef) -> TokenStream {
    let variant_name = &variant.name;
    let call = match MixedLiteral::from(variant) {
      MixedLiteral::Bool(value) => quote! { serializer.serialize_bool(#value) },
      MixedLiteral::Integer(value) => {
        let value = Literal::i64_suffixed(value);
        quote! { serializer.serialize_i64(#value) }
      }
      MixedLiteral::Float(value) => {
        let value = float_literal(value);
        quote! { serializer.serialize_f64(#value) }
      }
      MixedLiteral::String => {
        let value = variant.serde_name();
        quote! { serializer.serialize_str(#value) }
      }
    };
    quote! { Self::#variant_name => #call, }
  }

  fn deserialize_arm(self, variant: &VariantDef) -> TokenStream {
    let variant_name = &variant.name;
    let pattern = match MixedLiteral::from(variant) {
      MixedLiteral::Bool(value) => quote! { serde_json::Value::Bool(#value) },
      MixedLiteral::Integer(value) => {
        let value = Literal::i64_suffixed(value);
        quote! { serde_json::Value::Number(n) if n.as_i64() == Some(#value) }
      }
      MixedLiteral::Float(value) => {
        let value = float_literal(value);
        quote! { serde_json::Value::Number(n) if n.as_f64().map(f64::to_bits) == Some((#value).to_bits()) }
      }
      MixedLiteral::String => {
        let wire_values = variant
          .wire_values()
          .map(|value| {
            if self.0.case_insensitive {
              value.to_ascii_lowercase()
            } else {
              value.to_string()
            }
          })
          .unique();
        let subject = if self.0.case_insensitive {
          quote! { s.to_ascii_lowercase().as_str() }
        } else {
          quote! { s.as_str() }
        };
        quote! { serde_json::Value::String(s) if matches!(#subject, #(#wire_values)|*) }
      }
    };
    quote! { #pattern => Ok(Self::#variant_name), }
  }

  fn text_arm(self, variant: &VariantDef) -> Option<TokenStream> {
    let matches_text = match MixedLiteral::from(variant) {
      MixedLiteral::Bool(value) => {
        let text = value.to_string();
        if self.0.case_insensitive {
          quote! { s.eq_ignore_ascii_case(#text) }
        } else {
          quote! { s == #text }
        }
      }
      MixedLiteral::Integer(value) => {
        let value = Literal::i64_suffixed(value);
        quote! { s.parse::<i64>().ok() == Some(#value) }
      }
      MixedLiteral::Float(value) => {
        let value = float_literal(value);
        quote! { s.parse::<f64>().ok().map(f64::to_bits) == Some((#value).to_bits()) }
      }
      MixedLiteral::String => return None,
    };
    let variant_name = &variant.name;
    Some(quote! { serde_json::Value::String(s) if #matches_text => Ok(Self::#variant_name), })
  }
}

impl ToTokens for MixedEnumSerdeImplFragment<'_> {
  fn to_tokens(&self, tokens: &mut TokenStream) {
    let name = &self.0.name;
    let serialize_arms = self.0.variants.iter().map(Self::serialize_arm);
    let deserialize_arms = self.0.variants.iter().map(|v| self.deserialize_arm(v));
    let text_arms = self.0.variants.iter().filter_map(|v| self.text_arm(v));
    let expected = self.0.variants.iter().map(VariantDef::serde_name).join(", ");

    tokens.extend(quote! {
      impl serde::Serialize for #name {
        fn serialize<S>(&self, serializer: S) -> core::result::Result<S::Ok, S::Error>
        where
          S: serde::Serializer,
        {
          match self {
            #(#serialize_arms)*
          }
        }
      }

      impl<'de> serde::Deserialize<'de> for #name {
        fn deserialize<D>(deserializer: D) -> core::result::Result<Self, D::Error>
        where
          D: serde::Deserializer<'de>,
        {
          let value = serde_json::Value::deserialize(deserializer)?;
          match &value {
            #(#deserialize_arms)*
            #(#text_arms)*
            _ => Err(serde::de::Error::custom(format!("unknown variant {}, expected one of: {}", value, #expected))),
          }
        }
      }
    });
  }
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct EnumFragment<'a> {
  def: &'a EnumDef,
  vis: Visibility,
}

impl<'a> EnumFragment<'a> {
  pub(crate) fn new(def: &'a EnumDef, visibility: Visibility) -> Self {
    Self { def, vis: visibility }
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
    let non_exhaustive = self.def.non_exhaustive.then(|| quote! { #[non_exhaustive] });

    let default_idx = match &self.def.default_mode {
      EnumDefault::Derive => Some(self.def.derived_default_index()),
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
      #non_exhaustive
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

    let from_str_impl = if self.def.is_text_form() {
      FromStrImplFragment::new(self.def).to_token_stream()
    } else {
      quote! {}
    };

    let from_string_impl = FromStringImplFragment::new(self.def);

    let serde_impl = match &self.def.scalar_repr {
      Some(RustPrimitive::Value) => MixedEnumSerdeImplFragment::new(self.def).to_token_stream(),
      Some(primitive) => ScalarEnumSerdeImplFragment::new(name, primitive, &self.def.variants).to_token_stream(),
      None if self.def.case_insensitive => CaseInsensitiveDeserializeImplFragment::new(self.def).to_token_stream(),
      None => quote! {},
    };

    tokens.extend(quote! {
      #enum_def
      #display_impl
      #from_str_impl
      #from_string_impl
      #serde_impl
    });
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
        None => Err(serde::de::Error::missing_field(#disc_field))
      }
    };

    let ts = quote! {
      impl<'de> serde::Deserialize<'de> for #name {
        fn deserialize<D>(deserializer: D) -> core::result::Result<Self, D::Error>
        where
          D: serde::Deserializer<'de>,
        {
          let value = serde_json::Value::deserialize(deserializer)?;
          match value.get(#disc_field).and_then(|v| v.as_str()) {
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
    let with_headers = self
      .def
      .with_headers
      .as_ref()
      .map(|name| WithHeadersFragment::new(self.vis, name, &derives));

    let ts = quote! {
      #docs
      #derives
      #vis enum #name #generics {
        #variants
      }
      #with_headers
    };

    tokens.extend(ts);
  }
}

/// Emits the struct pairing a response body with its headers, e.g.
/// `pub struct WithHeaders<Headers, Body> { .. }`.
#[derive(Clone, Copy, Debug)]
struct WithHeadersFragment<'a> {
  vis: Visibility,
  name: &'a StructToken,
  derives: &'a DeriveAttribute<DeriveTrait>,
}

impl<'a> WithHeadersFragment<'a> {
  fn new(vis: Visibility, name: &'a StructToken, derives: &'a DeriveAttribute<DeriveTrait>) -> Self {
    Self { vis, name, derives }
  }
}

impl ToTokens for WithHeadersFragment<'_> {
  fn to_tokens(&self, tokens: &mut TokenStream) {
    let name = self.name;
    let derives = self.derives;
    let vis = &self.vis;

    tokens.extend(quote! {
      /// A response body together with the headers its response declares.
      #derives
      #vis struct #name<Headers, Body> {
        #vis headers: Headers,
        #vis body: Body,
      }
    });
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

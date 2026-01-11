use std::rc::Rc;

use proc_macro2::TokenStream;
use quote::{ToTokens, format_ident, quote};

use super::{
  CodeGenerationContext, Visibility,
  attributes::{generate_deprecated_attr, generate_derives_from_slice, generate_outer_attrs, generate_serde_attrs},
};
use crate::generator::{
  ast::{
    DeriveTrait, DerivesProvider, DiscriminatedEnumDef, EnumDef, EnumMethod, EnumMethodKind, ResponseEnumDef,
    ResponseVariant, SerdeAttribute, SerdeMode, StatusCodeToken, VariantContent, VariantDef,
  },
  converter::GenerationTarget,
};

fn box_if_needed(boxed: bool, inner: TokenStream) -> TokenStream {
  if boxed {
    quote! { Box::new(#inner) }
  } else {
    inner
  }
}

fn emit_enum_methods(name: impl ToTokens, vis: &TokenStream, methods: &[EnumMethod]) -> TokenStream {
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

fn emit_enum_method(vis: &TokenStream, method: &EnumMethod) -> TokenStream {
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
  vis: TokenStream,
}

impl EnumGenerator {
  pub fn new(_context: &Rc<CodeGenerationContext>, def: &EnumDef, visibility: Visibility) -> Self {
    Self {
      def: def.clone(),
      vis: visibility.to_tokens(),
    }
  }

  pub fn generate(&self) -> TokenStream {
    let name = &self.def.name;
    let docs = &self.def.docs;

    let derives = generate_derives_from_slice(&self.def.derives());

    let outer_attrs = generate_outer_attrs(&self.def.outer_attrs);
    let serde_attrs = self.emit_serde_attrs();
    let methods = emit_enum_methods(name, &self.vis, &self.def.methods);
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
  vis: TokenStream,
}

impl DiscriminatedEnumGenerator {
  pub fn new(_context: &Rc<CodeGenerationContext>, def: &DiscriminatedEnumDef, visibility: Visibility) -> Self {
    Self {
      def: def.clone(),
      vis: visibility.to_tokens(),
    }
  }

  pub fn generate(&self) -> TokenStream {
    let name = &self.def.name;
    let disc_field = &self.def.discriminator_field;
    let docs = &self.def.docs;

    let variants: Vec<TokenStream> = self
      .def
      .all_variants()
      .map(|v| {
        let variant_name = &v.variant_name;
        let type_name = &v.type_name;
        quote! { #variant_name(#type_name) }
      })
      .collect();

    let derives = generate_derives_from_slice(&self.def.derives());

    let vis = &self.vis;
    let enum_def = quote! {
      #docs
      #derives
      #vis enum #name {
        #(#variants),*
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
    let methods_impl = emit_enum_methods(name, &self.vis, &self.def.methods);

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
  vis: TokenStream,
}

impl ResponseEnumGenerator {
  pub fn new(context: &Rc<CodeGenerationContext>, def: &ResponseEnumDef, visibility: Visibility) -> Self {
    Self {
      context: context.clone(),
      def: def.clone(),
      vis: visibility.to_tokens(),
    }
  }

  pub fn generate(&self) -> TokenStream {
    let name = &self.def.name;
    let docs = &self.def.docs;

    let variants: Vec<TokenStream> = self
      .def
      .variants
      .iter()
      .map(|v| {
        let variant_name = &v.variant_name;
        let doc_line = v.doc_line();
        let content = v.schema_type.as_ref().map(|schema| {
          quote! { (#schema) }
        });

        quote! {
          #[doc = #doc_line]
          #variant_name #content
        }
      })
      .collect();

    let derives = generate_derives_from_slice(&self.def.derives());

    let into_response_impl = if self.context.config.target == GenerationTarget::Server {
      self.emit_into_response_impl()
    } else {
      quote! {}
    };

    let vis = &self.vis;
    quote! {
      #docs
      #derives
      #vis enum #name {
        #(#variants),*
      }
      #into_response_impl
    }
  }

  fn emit_into_response_impl(&self) -> TokenStream {
    self
      .def
      .try_from
      .iter()
      .map(|_| {
        let name = &self.def.name;
        let match_arms: Vec<TokenStream> = self.def.variants.iter().map(Self::emit_response_arm).collect();

        quote! {
          impl core::convert::TryFrom<&#name> for axum::response::IntoResponse {
            type Error = http::header::InvalidHeaderValue;

            fn try_from(value: &#name) -> core::result::Result<Self, Self::Error> {
              match self {
                #(#match_arms)*
              }
            }
          }

          impl core::convert::TryFrom<#name> for axum::response::IntoResponse {
            type Error = http::header::InvalidHeaderValue;

            fn try_from(value: #name) -> core::result::Result<Self, Self::Error> {
              http::HeaderMap::try_from(&headers)
            }
          }
        }
      })
      .collect()
  }

  fn emit_response_arm(variant: &ResponseVariant) -> TokenStream {
    let variant_name = &variant.variant_name;
    let status_code = emit_status_code(variant.status_code);

    if let Some(_) = &variant.schema_type {
      quote! {
        Self::#variant_name(data) => (#status_code, axum::Json(data)).into_response(),
      }
    } else {
      quote! {
        Self::#variant_name => #status_code.into_response(),
      }
    }
  }
}

fn emit_status_code(code: StatusCodeToken) -> TokenStream {
  match code {
    StatusCodeToken::Ok200 => quote! { axum::http::StatusCode::OK },
    StatusCodeToken::Created201 => quote! { axum::http::StatusCode::CREATED },
    StatusCodeToken::Accepted202 => quote! { axum::http::StatusCode::ACCEPTED },
    StatusCodeToken::NoContent204 => quote! { axum::http::StatusCode::NO_CONTENT },
    StatusCodeToken::BadRequest400 => quote! { axum::http::StatusCode::BAD_REQUEST },
    StatusCodeToken::Unauthorized401 => quote! { axum::http::StatusCode::UNAUTHORIZED },
    StatusCodeToken::Forbidden403 => quote! { axum::http::StatusCode::FORBIDDEN },
    StatusCodeToken::NotFound404 => quote! { axum::http::StatusCode::NOT_FOUND },
    StatusCodeToken::Conflict409 => quote! { axum::http::StatusCode::CONFLICT },
    StatusCodeToken::UnprocessableEntity422 => quote! { axum::http::StatusCode::UNPROCESSABLE_ENTITY },
    StatusCodeToken::InternalServerError500 => quote! { axum::http::StatusCode::INTERNAL_SERVER_ERROR },
    StatusCodeToken::Default => quote! { axum::http::StatusCode::INTERNAL_SERVER_ERROR },
    other => {
      if let Some(code) = other.code() {
        quote! { axum::http::StatusCode::from_u16(#code).unwrap_or(axum::http::StatusCode::INTERNAL_SERVER_ERROR) }
      } else {
        quote! { axum::http::StatusCode::INTERNAL_SERVER_ERROR }
      }
    }
  }
}

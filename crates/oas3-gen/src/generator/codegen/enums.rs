use proc_macro2::TokenStream;
use quote::{format_ident, quote};

use super::{
  Visibility,
  attributes::{
    generate_deprecated_attr, generate_derives_from_slice, generate_docs, generate_outer_attrs, generate_serde_attrs,
  },
};
use crate::generator::ast::{
  DerivesProvider, DiscriminatedEnumDef, EnumDef, EnumMethodKind, ResponseEnumDef, SerdeMode,
};

/// Generates standard Rust enums that use serde's derive macros for serialization.
///
/// This handles two OpenAPI patterns:
///
/// **1. Simple string/value enums** - OpenAPI `enum` with string values:
/// ```json
/// {
///   "Status": {
///     "type": "string",
///     "enum": ["pending", "active", "cancelled"]
///   }
/// }
/// ```
/// Generates:
/// ```ignore
/// #[derive(Serialize, Deserialize)]
/// pub enum Status {
///     Pending,
///     Active,
///     Cancelled,
/// }
/// ```
///
/// **2. Untagged unions** - OpenAPI `oneOf` without a discriminator:
/// ```json
/// {
///   "StringOrNumber": {
///     "oneOf": [
///       { "type": "string" },
///       { "type": "number" }
///     ]
///   }
/// }
/// ```
/// Generates:
/// ```ignore
/// #[derive(Serialize, Deserialize)]
/// #[serde(untagged)]
/// pub enum StringOrNumber {
///     String(String),
///     Number(f64),
/// }
/// ```
///
/// Serde's derive macros handle serialization for these patterns. For case-insensitive
/// enums, a custom `Deserialize` impl is generated instead of using the derive.
pub(crate) struct EnumGenerator<'a> {
  def: &'a EnumDef,
  visibility: Visibility,
}

impl<'a> EnumGenerator<'a> {
  pub fn new(def: &'a EnumDef, visibility: Visibility) -> Self {
    Self { def, visibility }
  }

  pub fn generate(&self) -> TokenStream {
    let name = &self.def.name;
    let docs = generate_docs(&self.def.docs);
    let vis = self.visibility.to_tokens();

    let derives = generate_derives_from_slice(&self.def.derives());

    let outer_attrs = generate_outer_attrs(&self.def.outer_attrs);
    let serde_attrs = self.generate_serde_attrs();
    let methods = self.generate_methods();
    let variants = self.generate_variants();

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

    if self.def.case_insensitive {
      let deserialize_impl = self.generate_case_insensitive_deserialize();
      quote! {
        #enum_def
        #deserialize_impl
      }
    } else {
      enum_def
    }
  }

  fn generate_variants(&self) -> Vec<TokenStream> {
    self
      .def
      .variants
      .iter()
      .enumerate()
      .map(|(idx, v)| {
        let variant_name = format_ident!("{}", v.name);
        let variant_docs = generate_docs(&v.docs);
        let variant_serde_attrs = generate_serde_attrs(&v.serde_attrs);
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

  fn generate_methods(&self) -> TokenStream {
    if self.def.methods.is_empty() {
      return quote! {};
    }

    let name = &self.def.name;
    let vis = self.visibility.to_tokens();

    let methods = self.def.methods.iter().map(|m| {
      let method_name = format_ident!("{}", m.name);
      let docs = generate_docs(&m.docs);

      match &m.kind {
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
      }
    });

    quote! {
      impl #name {
        #(#methods)*
      }
    }
  }

  fn generate_serde_attrs(&self) -> TokenStream {
    let mut attrs = vec![];

    if let Some(ref discriminator) = self.def.discriminator {
      attrs.push(quote! { tag = #discriminator });
    }

    for attr in &self.def.serde_attrs {
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

  fn generate_case_insensitive_deserialize(&self) -> TokenStream {
    let name = &self.def.name;

    let (match_arms, serde_names): (Vec<TokenStream>, Vec<String>) = self
      .def
      .variants
      .iter()
      .map(|v| {
        let variant_name = format_ident!("{}", v.name);
        let serde_name = v.serde_name();
        let lower_val = serde_name.to_ascii_lowercase();
        let match_arm = quote! {
          #lower_val => Ok(#name::#variant_name),
        };
        (match_arm, serde_name)
      })
      .unzip();

    let fallback_arm = if let Some(fb) = self.def.fallback_variant() {
      let variant_name = format_ident!("{}", fb.name);
      quote! { _ => Ok(#name::#variant_name), }
    } else {
      quote! { _ => Err(serde::de::Error::unknown_variant(&s, &[ #(#serde_names),* ])), }
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
}

/// Generates tagged union enums with custom `Serialize`/`Deserialize` implementations.
///
/// This handles OpenAPI `oneOf` schemas that specify a `discriminator` with explicit mapping:
/// ```json
/// {
///   "Pet": {
///     "oneOf": [
///       { "$ref": "#/components/schemas/Dog" },
///       { "$ref": "#/components/schemas/Cat" }
///     ],
///     "discriminator": {
///       "propertyName": "petType",
///       "mapping": {
///         "dog": "#/components/schemas/Dog",
///         "cat": "#/components/schemas/Cat"
///       }
///     }
///   }
/// }
/// ```
///
/// Generates:
/// ```ignore
/// pub enum Pet {
///     Dog(Dog),
///     Cat(Cat),
/// }
///
/// impl Pet {
///     pub const DISCRIMINATOR_FIELD: &'static str = "petType";
/// }
///
/// impl Serialize for Pet { /* delegates to inner type */ }
/// impl Deserialize for Pet { /* reads petType, dispatches to variant */ }
/// ```
///
/// **Why custom serde impls instead of `#[serde(tag = "...")]`?**
///
/// Serde's internally-tagged representation (`#[serde(tag = "petType")]`) requires the tag
/// field to be added by serde during serialization. But in OpenAPI discriminator patterns,
/// the discriminator field (`petType`) is defined as a property *inside* each variant's
/// schema (Dog has `petType: "dog"`, Cat has `petType: "cat"`).
///
/// The custom impl:
/// - **Deserialize**: Parses JSON as `serde_json::Value`, extracts the discriminator field,
///   matches it to the correct variant, then deserializes the full value as that type.
/// - **Serialize**: Delegates directly to the inner type's `Serialize`, which already
///   includes the discriminator field.
///
/// An optional `fallback` variant captures unknown discriminator values for forward
/// compatibility with API changes.
pub(crate) struct DiscriminatedEnumGenerator<'a> {
  def: &'a DiscriminatedEnumDef,
  visibility: Visibility,
}

impl<'a> DiscriminatedEnumGenerator<'a> {
  pub fn new(def: &'a DiscriminatedEnumDef, visibility: Visibility) -> Self {
    Self { def, visibility }
  }

  pub fn generate(&self) -> TokenStream {
    let name = &self.def.name;
    let disc_field = &self.def.discriminator_field;
    let docs = generate_docs(&self.def.docs);
    let vis = self.visibility.to_tokens();

    let variants: Vec<TokenStream> = self
      .def
      .all_variants()
      .map(|v| {
        let variant_name = format_ident!("{}", v.variant_name);
        let type_name = &v.type_name;
        quote! { #variant_name(#type_name) }
      })
      .collect();

    let derives = generate_derives_from_slice(&self.def.derives());

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

    let default_impl = self.generate_default_impl();
    let serialize_impl = self.generate_serialize_impl();
    let deserialize_impl = self.generate_deserialize_impl();

    quote! {
      #enum_def
      #discriminator_const
      #default_impl
      #serialize_impl
      #deserialize_impl
    }
  }

  fn generate_default_impl(&self) -> TokenStream {
    let Some(default_variant) = self.def.default_variant() else {
      return quote! {};
    };

    let name = &self.def.name;
    let variant_ident = format_ident!("{}", default_variant.variant_name);
    let type_tokens = &default_variant.type_name;

    quote! {
      impl Default for #name {
        fn default() -> Self {
          Self::#variant_ident(<#type_tokens>::default())
        }
      }
    }
  }

  fn generate_serialize_impl(&self) -> TokenStream {
    if self.def.serde_mode == SerdeMode::DeserializeOnly {
      return quote! {};
    }

    let name = &self.def.name;

    let arms: Vec<TokenStream> = self
      .def
      .all_variants()
      .map(|v| {
        let variant_name = format_ident!("{}", v.variant_name);
        quote! { Self::#variant_name(v) => v.serialize(serializer) }
      })
      .collect();

    quote! {
      impl serde::Serialize for #name {
        fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
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

  fn generate_deserialize_impl(&self) -> TokenStream {
    if self.def.serde_mode == SerdeMode::SerializeOnly {
      return quote! {};
    }

    let name = &self.def.name;
    let disc_field = &self.def.discriminator_field;

    let variant_arms: Vec<TokenStream> = self
      .def
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

    let none_handling = if let Some(ref fb) = self.def.fallback {
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
}

/// Generates response enums that represent the possible HTTP responses from an operation.
///
/// Each variant corresponds to a status code, optionally wrapping the response body type:
/// ```json
/// {
///   "responses": {
///     "200": {
///       "description": "Success",
///       "content": {
///         "application/json": {
///           "schema": { "$ref": "#/components/schemas/User" }
///         }
///       }
///     },
///     "404": {
///       "description": "Not found"
///     }
///   }
/// }
/// ```
///
/// Generates:
/// ```ignore
/// #[derive(Clone, Debug)]
/// pub enum GetUserResponse {
///     /// 200: Success
///     Ok200(User),
///     /// 404: Not found
///     NotFound404,
/// }
/// ```
///
/// These enums are used by generated client code to represent typed API responses.
/// They intentionally don't derive `Serialize`/`Deserialize` since response parsing
/// is handled by the client's `parse_response` method which inspects status codes.
pub(crate) struct ResponseEnumGenerator<'a> {
  def: &'a ResponseEnumDef,
  visibility: Visibility,
}

impl<'a> ResponseEnumGenerator<'a> {
  pub fn new(def: &'a ResponseEnumDef, visibility: Visibility) -> Self {
    Self { def, visibility }
  }

  pub fn generate(&self) -> TokenStream {
    let name = &self.def.name;
    let docs = generate_docs(&self.def.docs);
    let vis = self.visibility.to_tokens();

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

    quote! {
      #docs
      #derives
      #vis enum #name {
        #(#variants),*
      }
    }
  }
}

fn box_if_needed(boxed: bool, inner: TokenStream) -> TokenStream {
  if boxed {
    quote! { Box::new(#inner) }
  } else {
    inner
  }
}

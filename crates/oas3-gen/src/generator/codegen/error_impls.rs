use proc_macro2::TokenStream;
use quote::{format_ident, quote};

use crate::generator::ast::{
  DiscriminatedEnumDef, EnumDef, FieldDef, RustPrimitive, RustType, StructDef, VariantContent,
};

const FALLBACK_ERROR_MESSAGE: &str = "Error";
const MESSAGE_FIELD_NAMES: &[&str] = &["message", "detail", "title"];

pub(crate) fn generate_error_impl(rust_type: &RustType) -> Option<TokenStream> {
  match rust_type {
    RustType::Struct(def) => struct_impl::Generator::new(def).map(|g| g.emit()),
    RustType::Enum(def) => enum_impl::Generator::new(def).map(|g| g.emit()),
    RustType::DiscriminatedEnum(def) => discriminated_enum_impl::Generator::new(def).map(|g| g.emit()),
    _ => None,
  }
}

mod struct_impl {
  use super::*;

  pub(super) struct Generator<'a> {
    def: &'a StructDef,
    source: ErrorSource<'a>,
  }

  impl<'a> Generator<'a> {
    pub(super) fn new(def: &'a StructDef) -> Option<Self> {
      let source = ErrorSource::find(&def.fields)?;
      Some(Self { def, source })
    }

    pub(super) fn emit(&self) -> TokenStream {
      let type_ident = format_ident!("{}", &self.def.name);
      let display_impl = self.source.display_impl(&type_ident);
      let error_impl = self.source.error_impl(&type_ident);

      quote! {
        #display_impl
        #error_impl
      }
    }
  }

  enum ErrorSource<'a> {
    ErrorField { field: &'a FieldDef },
    MessageField { field: &'a FieldDef },
  }

  impl<'a> ErrorSource<'a> {
    fn find(fields: &'a [FieldDef]) -> Option<Self> {
      if let Some(field) = find_error_field(fields) {
        return Some(Self::ErrorField { field });
      }
      if let Some(field) = find_message_field(fields) {
        return Some(Self::MessageField { field });
      }
      None
    }

    fn display_impl(&self, type_ident: &proc_macro2::Ident) -> TokenStream {
      match self {
        Self::ErrorField { field } => error_field_display(type_ident, field),
        Self::MessageField { field } => message_field_display(type_ident, field),
      }
    }

    fn error_impl(&self, type_ident: &proc_macro2::Ident) -> TokenStream {
      match self {
        Self::ErrorField { field } => error_field_error(type_ident, field),
        Self::MessageField { .. } => {
          quote! { impl std::error::Error for #type_ident {} }
        }
      }
    }
  }

  fn find_error_field(fields: &[FieldDef]) -> Option<&FieldDef> {
    fields.iter().find(|f| {
      let is_error_name = f.name == "error" || f.name == "errors";
      let is_custom_type = matches!(f.rust_type.base_type, RustPrimitive::Custom(_));
      is_error_name && is_custom_type
    })
  }

  fn find_message_field(fields: &[FieldDef]) -> Option<&FieldDef> {
    MESSAGE_FIELD_NAMES.iter().find_map(|&candidate| {
      fields
        .iter()
        .find(|f| f.name == candidate && matches!(f.rust_type.base_type, RustPrimitive::String))
    })
  }

  fn error_field_display(type_ident: &proc_macro2::Ident, field: &FieldDef) -> TokenStream {
    let field_ident = format_ident!("{}", &field.name);
    let fallback = FALLBACK_ERROR_MESSAGE;
    let is_optional = field.rust_type.nullable;

    if field.rust_type.is_array {
      if is_optional {
        quote! {
          impl std::fmt::Display for #type_ident {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
              if let Some(ref errs) = self.#field_ident {
                if let Some(first) = errs.first() {
                  write!(f, "{first}")
                } else {
                  write!(f, #fallback)
                }
              } else {
                write!(f, #fallback)
              }
            }
          }
        }
      } else {
        quote! {
          impl std::fmt::Display for #type_ident {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
              if let Some(first) = self.#field_ident.first() {
                write!(f, "{first}")
              } else {
                write!(f, #fallback)
              }
            }
          }
        }
      }
    } else if is_optional {
      quote! {
        impl std::fmt::Display for #type_ident {
          fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            if let Some(ref err) = self.#field_ident {
              write!(f, "{err}")
            } else {
              write!(f, #fallback)
            }
          }
        }
      }
    } else {
      quote! {
        impl std::fmt::Display for #type_ident {
          fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(f, "{}", self.#field_ident)
          }
        }
      }
    }
  }

  fn error_field_error(type_ident: &proc_macro2::Ident, field: &FieldDef) -> TokenStream {
    let field_ident = format_ident!("{}", &field.name);
    let is_optional = field.rust_type.nullable;

    if field.rust_type.is_array {
      if is_optional {
        quote! {
          impl std::error::Error for #type_ident {
            fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
              self.#field_ident.as_ref()?.first().map(|e| e as &(dyn std::error::Error + 'static))
            }
          }
        }
      } else {
        quote! {
          impl std::error::Error for #type_ident {
            fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
              self.#field_ident.first().map(|e| e as &(dyn std::error::Error + 'static))
            }
          }
        }
      }
    } else if is_optional {
      quote! {
        impl std::error::Error for #type_ident {
          fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
            self.#field_ident.as_ref().map(|e| e as &(dyn std::error::Error + 'static))
          }
        }
      }
    } else {
      quote! {
        impl std::error::Error for #type_ident {
          fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
            Some(&self.#field_ident as &(dyn std::error::Error + 'static))
          }
        }
      }
    }
  }

  fn message_field_display(type_ident: &proc_macro2::Ident, field: &FieldDef) -> TokenStream {
    let field_ident = format_ident!("{}", &field.name);
    let fallback = FALLBACK_ERROR_MESSAGE;

    if field.rust_type.nullable {
      quote! {
        impl std::fmt::Display for #type_ident {
          fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            if let Some(ref msg) = self.#field_ident {
              write!(f, "{msg}")
            } else {
              write!(f, #fallback)
            }
          }
        }
      }
    } else {
      quote! {
        impl std::fmt::Display for #type_ident {
          fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(f, "{}", self.#field_ident)
          }
        }
      }
    }
  }
}

mod enum_impl {
  use super::*;

  pub(super) struct Generator<'a> {
    def: &'a EnumDef,
  }

  impl<'a> Generator<'a> {
    pub(super) fn new(def: &'a EnumDef) -> Option<Self> {
      let has_tuple_variants = def.variants.iter().any(|v| match &v.content {
        VariantContent::Tuple(types) => !types.is_empty(),
        VariantContent::Unit => false,
      });

      if has_tuple_variants { Some(Self { def }) } else { None }
    }

    pub(super) fn emit(&self) -> TokenStream {
      let type_ident = &self.def.name;
      let (display_arms, source_arms): (Vec<_>, Vec<_>) = self
        .def
        .variants
        .iter()
        .map(|variant| {
          let variant_ident = &variant.name;
          match &variant.content {
            VariantContent::Tuple(types) if !types.is_empty() => (
              quote! { Self::#variant_ident(err) => write!(f, "{err}"), },
              quote! { Self::#variant_ident(err) => Some(err as &(dyn std::error::Error + 'static)), },
            ),
            VariantContent::Tuple(_) | VariantContent::Unit => (
              quote! { Self::#variant_ident => write!(f, "{}", stringify!(#variant_ident)), },
              quote! { Self::#variant_ident => None, },
            ),
          }
        })
        .unzip();

      quote! {
        impl std::fmt::Display for #type_ident {
          fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            match self {
              #(#display_arms)*
            }
          }
        }

        impl std::error::Error for #type_ident {
          fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
            match self {
              #(#source_arms)*
            }
          }
        }
      }
    }
  }
}

mod discriminated_enum_impl {
  use super::*;

  pub(super) struct Generator<'a> {
    def: &'a DiscriminatedEnumDef,
  }

  impl<'a> Generator<'a> {
    pub(super) fn new(def: &'a DiscriminatedEnumDef) -> Option<Self> {
      if def.variants.is_empty() && def.fallback.is_none() {
        return None;
      }
      Some(Self { def })
    }

    pub(super) fn emit(&self) -> TokenStream {
      let type_ident = &self.def.name;
      let (display_arms, source_arms): (Vec<_>, Vec<_>) = self
        .def
        .all_variants()
        .map(|variant| {
          let variant_ident = format_ident!("{}", &variant.variant_name);
          (
            quote! { Self::#variant_ident(err) => write!(f, "{err}"), },
            quote! { Self::#variant_ident(err) => Some(err as &(dyn std::error::Error + 'static)), },
          )
        })
        .unzip();

      quote! {
        impl std::fmt::Display for #type_ident {
          fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            match self {
              #(#display_arms)*
            }
          }
        }

        impl std::error::Error for #type_ident {
          fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
            match self {
              #(#source_arms)*
            }
          }
        }
      }
    }
  }
}

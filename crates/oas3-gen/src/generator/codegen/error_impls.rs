use proc_macro2::TokenStream;
use quote::{format_ident, quote};

use crate::generator::ast::{EnumDef, FieldDef, RustPrimitive, RustType, VariantContent};

const FALLBACK_ERROR_MESSAGE: &str = "Error";
const MESSAGE_FIELD_NAMES: &[&str] = &["message", "detail", "title"];

pub(crate) fn generate_error_impl(rust_type: &RustType) -> Option<TokenStream> {
  match rust_type {
    RustType::Struct(def) => generate_for_struct(def),
    RustType::Enum(def) => generate_for_enum(def),
    _ => None,
  }
}

fn generate_for_struct(def: &crate::generator::ast::StructDef) -> Option<TokenStream> {
  let type_ident = format_ident!("{}", &def.name);

  if let Some(error_field_name) = find_error_field(&def.fields) {
    let field = def.fields.iter().find(|f| f.name == error_field_name)?;
    let field_ident = format_ident!("{error_field_name}");
    let fallback = FALLBACK_ERROR_MESSAGE;
    let is_optional = field.rust_type.nullable;

    let (display_impl, source_impl) = if field.rust_type.is_array {
      if is_optional {
        (
          quote! {
            impl std::fmt::Display for #type_ident {
              fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                if let Some(ref errs) = self.#field_ident {
                  if let Some(first) = errs.first() {
                    write!(f, "{}", first)
                  } else {
                    write!(f, #fallback)
                  }
                } else {
                  write!(f, #fallback)
                }
              }
            }
          },
          quote! {
            impl std::error::Error for #type_ident {
              fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
                self.#field_ident.as_ref()?.first().map(|e| e as &(dyn std::error::Error + 'static))
              }
            }
          },
        )
      } else {
        (
          quote! {
            impl std::fmt::Display for #type_ident {
              fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                if let Some(first) = self.#field_ident.first() {
                  write!(f, "{}", first)
                } else {
                  write!(f, #fallback)
                }
              }
            }
          },
          quote! {
            impl std::error::Error for #type_ident {
              fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
                self.#field_ident.first().map(|e| e as &(dyn std::error::Error + 'static))
              }
            }
          },
        )
      }
    } else if is_optional {
      (
        quote! {
          impl std::fmt::Display for #type_ident {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
              if let Some(ref err) = self.#field_ident {
                write!(f, "{}", err)
              } else {
                write!(f, #fallback)
              }
            }
          }
        },
        quote! {
          impl std::error::Error for #type_ident {
            fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
              self.#field_ident.as_ref().map(|e| e as &(dyn std::error::Error + 'static))
            }
          }
        },
      )
    } else {
      (
        quote! {
          impl std::fmt::Display for #type_ident {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
              write!(f, "{}", self.#field_ident)
            }
          }
        },
        quote! {
          impl std::error::Error for #type_ident {
            fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
              Some(&self.#field_ident as &(dyn std::error::Error + 'static))
            }
          }
        },
      )
    };

    return Some(quote! {
      #display_impl
      #source_impl
    });
  }

  let message_field = find_message_field(&def.fields)?;
  let field_name = format_ident!("{}", &message_field.name);
  let fallback = FALLBACK_ERROR_MESSAGE;

  let display_impl = if message_field.rust_type.nullable {
    quote! {
      impl std::fmt::Display for #type_ident {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
          if let Some(ref msg) = self.#field_name {
            write!(f, "{}", msg)
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
          write!(f, "{}", self.#field_name)
        }
      }
    }
  };

  Some(quote! {
    #display_impl

    impl std::error::Error for #type_ident {}
  })
}

fn generate_for_enum(def: &EnumDef) -> Option<TokenStream> {
  if !is_error_enum(def) {
    return None;
  }

  let type_ident = format_ident!("{}", &def.name);
  let (display_arms, source_arms) = generate_enum_error_arms(def);

  Some(quote! {
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
  })
}

fn is_error_enum(def: &EnumDef) -> bool {
  def.variants.iter().any(|v| match &v.content {
    VariantContent::Tuple(types) => !types.is_empty(),
    VariantContent::Unit => false,
  })
}

fn generate_enum_error_arms(def: &EnumDef) -> (Vec<TokenStream>, Vec<TokenStream>) {
  def
    .variants
    .iter()
    .map(|variant| {
      let variant_ident = format_ident!("{}", &variant.name);

      match &variant.content {
        VariantContent::Tuple(types) if !types.is_empty() => (
          quote! { Self::#variant_ident(err) => write!(f, "{}", err), },
          quote! { Self::#variant_ident(err) => Some(err as &(dyn std::error::Error + 'static)), },
        ),
        VariantContent::Tuple(_) | VariantContent::Unit => (
          quote! { Self::#variant_ident => write!(f, "{}", stringify!(#variant_ident)), },
          quote! { Self::#variant_ident => None, },
        ),
      }
    })
    .unzip()
}

fn find_error_field(fields: &[FieldDef]) -> Option<&str> {
  fields.iter().find_map(|f| {
    let is_error_name = f.name == "error" || f.name == "errors";
    let is_custom_type = matches!(f.rust_type.base_type, RustPrimitive::Custom(_));
    if is_error_name && is_custom_type {
      Some(f.name.as_str())
    } else {
      None
    }
  })
}

fn find_message_field(fields: &[FieldDef]) -> Option<&FieldDef> {
  MESSAGE_FIELD_NAMES.iter().find_map(|&candidate| {
    fields
      .iter()
      .find(|f| f.name == candidate && matches!(f.rust_type.base_type, RustPrimitive::String))
  })
}

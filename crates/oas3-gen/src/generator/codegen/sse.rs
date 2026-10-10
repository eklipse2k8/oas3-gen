use proc_macro2::TokenStream;
use quote::{ToTokens, quote};

use crate::generator::{
  ast::{DerivesProvider as _, DiscriminatedEnumDef, FieldDef, FrameData, RustType, StructDef},
  codegen::{
    Visibility,
    attributes::DeriveAttribute,
    enums::{DiscriminatedVariantFragment, EnumVariants},
  },
};

/// A client's union of `text/event-stream` frame types: an enum of the frames its members
/// read as, and the `FromSseFrame` impl that picks the member by the frame's event name.
#[derive(Clone, Copy, Debug)]
pub(crate) struct FrameUnionFragment<'a> {
  def: &'a DiscriminatedEnumDef,
  vis: Visibility,
}

impl<'a> FrameUnionFragment<'a> {
  pub(crate) fn new(def: &'a DiscriminatedEnumDef, vis: Visibility) -> Self {
    Self { def, vis }
  }
}

impl ToTokens for FrameUnionFragment<'_> {
  fn to_tokens(&self, tokens: &mut TokenStream) {
    let name = &self.def.name;
    let docs = &self.def.docs;
    let vis = &self.vis;
    let derives = DeriveAttribute::new(self.def.derives());
    let variants = EnumVariants::new(
      self
        .def
        .variants
        .iter()
        .map(DiscriminatedVariantFragment::new)
        .collect::<Vec<_>>(),
    );
    let arms = self.def.variants.iter().map(|variant| {
      let events = &variant.discriminator_values;
      let variant_name = &variant.variant_name;
      let decode = if variant.frame.is_some_and(|member| member.data == FrameData::Text) {
        quote! { text }
      } else {
        quote! { json }
      };
      let boxed = variant.type_name.boxed.then(|| quote! { .map(Box::new) });
      quote! { #(#events)|* => frame.#decode() #boxed .map(Self::#variant_name), }
    });
    let expected = self
      .def
      .variants
      .iter()
      .flat_map(|variant| &variant.discriminator_values);

    tokens.extend(quote! {
      #docs
      #derives
      #vis enum #name {
        #variants
      }

      impl oas3_gen_support::FromSseFrame for #name {
        fn from_sse_frame(
          frame: &oas3_gen_support::RawSseFrame,
        ) -> core::result::Result<Self, oas3_gen_support::DiagnosticsError> {
          match frame.event() {
            #(#arms)*
            _ => Err(frame.unknown_event(&[#(#expected),*])),
          }
        }
      }
    });
  }
}

/// The axum event a server sends for each item of a `text/event-stream` frame type: a
/// struct's `data`, `event`, `id`, and `retry` fill the event's own fields, and a union
/// sends its member.
#[derive(Clone, Copy, Debug)]
pub(crate) struct SseEventFragment<'a> {
  rust_type: &'a RustType,
}

impl<'a> SseEventFragment<'a> {
  pub(crate) fn new(rust_type: &'a RustType) -> Self {
    Self { rust_type }
  }
}

impl ToTokens for SseEventFragment<'_> {
  fn to_tokens(&self, tokens: &mut TokenStream) {
    let (name, body) = match self.rust_type {
      RustType::Struct(def) if def.sse_frame => (def.name.to_token_stream(), struct_event(def)),
      RustType::DiscriminatedEnum(def) if def.sse_frame => (def.name.to_token_stream(), union_event(def)),
      _ => return,
    };
    tokens.extend(quote! {
      impl TryFrom<#name> for axum::response::sse::Event {
        type Error = axum::Error;

        fn try_from(frame: #name) -> core::result::Result<Self, Self::Error> {
          #body
        }
      }
    });
  }
}

fn struct_event(def: &StructDef) -> TokenStream {
  let writes = def.fields.iter().map(|field| {
    let ident = &field.name;
    if field.rust_type.nullable {
      let write = field_write(field, &quote! { value });
      quote! {
        if let Some(value) = frame.#ident {
          #write
        }
      }
    } else {
      field_write(field, &quote! { frame.#ident })
    }
  });
  quote! {
    let mut event = Self::default();
    #(#writes)*
    Ok(event)
  }
}

/// Writes `value`, the value of `field`, into `event`. An `event` or `id` whose text has
/// a line break, which axum would panic on, is an error instead.
fn field_write(field: &FieldDef, value: &TokenStream) -> TokenStream {
  match field.serde_name() {
    "data" => match FrameData::of(&field.rust_type) {
      FrameData::Text => quote! { event = event.data(#value); },
      FrameData::Json => quote! { event = event.json_data(#value)?; },
    },
    name @ ("event" | "id") => {
      let (method, forbidden, message) = if name == "id" {
        (
          quote! { id },
          quote! { ['\0', '\r', '\n'] },
          "a Server-Sent Events `id` can't contain a line break or NUL",
        )
      } else {
        (
          quote! { event },
          quote! { ['\r', '\n'] },
          "a Server-Sent Events `event` can't contain a line break",
        )
      };
      let (binding, value) = if field.rust_type.is_string_like() {
        (None, value.clone())
      } else {
        (Some(quote! { let text = #value.to_string(); }), quote! { text })
      };
      quote! {
        #binding
        if #value.contains(#forbidden) {
          return Err(axum::Error::new(#message));
        }
        event = event.#method(#value);
      }
    }
    _ => {
      let millis = if field.rust_type.base_type.is_integer() {
        quote! { u64::try_from(#value) }
      } else {
        quote! { #value.to_string().parse::<u64>() }
      };
      quote! { event = event.retry(std::time::Duration::from_millis(#millis.map_err(axum::Error::new)?)); }
    }
  }
}

fn union_event(def: &DiscriminatedEnumDef) -> TokenStream {
  let name = &def.name;
  let arms = def.variants.iter().map(|variant| {
    let variant_name = &variant.variant_name;
    let member = if variant.type_name.boxed {
      quote! { *member }
    } else {
      quote! { member }
    };
    let unnamed = variant.frame.is_some_and(|member| !member.declares_event);
    let named = variant
      .discriminator_values
      .first()
      .filter(|_| unnamed)
      .map(|event| quote! { .map(|sse| sse.event(#event)) });
    quote! { #name::#variant_name(member) => Self::try_from(#member) #named, }
  });
  quote! {
    match frame {
      #(#arms)*
    }
  }
}

use std::collections::BTreeSet;

use proc_macro2::TokenStream;
use quote::ToTokens;
use strum::Display;

use super::{
  DiscriminatedEnumDef, EnumDef, ResponseEnumDef, ResponseMediaType, SerdeMode, StructDef, StructKind, VariantContent,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SerdeImpl {
  #[default]
  None,
  Derive,
  Custom,
}

#[derive(Debug, Clone, Copy, Display, PartialEq, Eq, PartialOrd, Ord)]
pub enum DeriveTrait {
  Debug,
  Clone,
  PartialEq,
  Eq,
  Hash,
  Serialize,
  Deserialize,
  #[strum(serialize = "validator::Validate")]
  Validate,
  #[strum(serialize = "oas3_gen_support::Default")]
  Default,
  #[strum(serialize = "bon::Builder")]
  Builder,
}

impl ToTokens for DeriveTrait {
  fn to_tokens(&self, tokens: &mut proc_macro2::TokenStream) {
    tokens.extend(self.to_string().parse::<TokenStream>().expect("DeriveTrait Token"));
  }
}

pub trait DerivesProvider {
  fn derives(&self) -> BTreeSet<DeriveTrait>;
  fn is_serializable(&self) -> SerdeImpl;
  fn is_deserializable(&self) -> SerdeImpl;
}

impl DerivesProvider for StructDef {
  fn derives(&self) -> BTreeSet<DeriveTrait> {
    let mut derives = BTreeSet::from([DeriveTrait::Debug, DeriveTrait::Clone, DeriveTrait::Default]);

    if self.kind != StructKind::OperationRequest {
      derives.insert(DeriveTrait::PartialEq);
    }

    if self.is_serializable() == SerdeImpl::Derive {
      derives.insert(DeriveTrait::Serialize);
    }
    if self.is_deserializable() == SerdeImpl::Derive {
      derives.insert(DeriveTrait::Deserialize);
    }

    if self.kind == StructKind::OperationRequest || self.has_validation_attrs() {
      derives.insert(DeriveTrait::Validate);
    }

    derives.extend(&self.additional_derives);
    derives
  }

  fn is_serializable(&self) -> SerdeImpl {
    match self.serde_mode {
      SerdeMode::SerializeOnly | SerdeMode::Both => SerdeImpl::Derive,
      SerdeMode::DeserializeOnly | SerdeMode::None => SerdeImpl::None,
    }
  }

  fn is_deserializable(&self) -> SerdeImpl {
    match self.serde_mode {
      SerdeMode::DeserializeOnly | SerdeMode::Both => SerdeImpl::Derive,
      SerdeMode::SerializeOnly | SerdeMode::None => SerdeImpl::None,
    }
  }
}

impl DerivesProvider for EnumDef {
  fn derives(&self) -> BTreeSet<DeriveTrait> {
    let mut derives = BTreeSet::from([
      DeriveTrait::Debug,
      DeriveTrait::Clone,
      DeriveTrait::PartialEq,
      DeriveTrait::Default,
    ]);

    if self.is_simple() {
      derives.insert(DeriveTrait::Eq);
      derives.insert(DeriveTrait::Hash);
    }

    if self.is_serializable() == SerdeImpl::Derive {
      derives.insert(DeriveTrait::Serialize);
    }
    if self.is_deserializable() == SerdeImpl::Derive {
      derives.insert(DeriveTrait::Deserialize);
    }

    derives
  }

  fn is_serializable(&self) -> SerdeImpl {
    match self.serde_mode {
      SerdeMode::SerializeOnly | SerdeMode::Both => SerdeImpl::Derive,
      SerdeMode::DeserializeOnly | SerdeMode::None => SerdeImpl::None,
    }
  }

  fn is_deserializable(&self) -> SerdeImpl {
    match self.serde_mode {
      SerdeMode::DeserializeOnly | SerdeMode::Both => {
        if self.case_insensitive {
          SerdeImpl::Custom
        } else {
          SerdeImpl::Derive
        }
      }
      SerdeMode::SerializeOnly | SerdeMode::None => SerdeImpl::None,
    }
  }
}

impl EnumDef {
  #[must_use]
  pub fn is_simple(&self) -> bool {
    self.variants.iter().all(|v| matches!(v.content, VariantContent::Unit))
  }
}

impl DerivesProvider for DiscriminatedEnumDef {
  fn derives(&self) -> BTreeSet<DeriveTrait> {
    BTreeSet::from([DeriveTrait::Debug, DeriveTrait::Clone, DeriveTrait::PartialEq])
  }

  fn is_serializable(&self) -> SerdeImpl {
    match self.serde_mode {
      SerdeMode::SerializeOnly | SerdeMode::Both => SerdeImpl::Custom,
      SerdeMode::DeserializeOnly | SerdeMode::None => SerdeImpl::None,
    }
  }

  fn is_deserializable(&self) -> SerdeImpl {
    match self.serde_mode {
      SerdeMode::DeserializeOnly | SerdeMode::Both => SerdeImpl::Custom,
      SerdeMode::SerializeOnly | SerdeMode::None => SerdeImpl::None,
    }
  }
}

impl DerivesProvider for ResponseEnumDef {
  fn derives(&self) -> BTreeSet<DeriveTrait> {
    let mut derives = BTreeSet::from([DeriveTrait::Debug]);

    let has_event_stream = self
      .variants
      .iter()
      .any(|v| ResponseMediaType::has_event_stream(&v.media_types));

    if !has_event_stream {
      derives.insert(DeriveTrait::Clone);
    }

    derives
  }

  fn is_serializable(&self) -> SerdeImpl {
    SerdeImpl::None
  }

  fn is_deserializable(&self) -> SerdeImpl {
    SerdeImpl::None
  }
}

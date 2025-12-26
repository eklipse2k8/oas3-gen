use std::collections::BTreeSet;

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
}

pub trait DerivesProvider {
  fn derives(&self) -> BTreeSet<DeriveTrait>;
  fn is_serializable(&self) -> SerdeImpl;
  fn is_deserializable(&self) -> SerdeImpl;
}

impl DerivesProvider for StructDef {
  fn derives(&self) -> BTreeSet<DeriveTrait> {
    let mut derives = BTreeSet::from([DeriveTrait::Debug, DeriveTrait::Clone, DeriveTrait::Default]);

    match self.kind {
      StructKind::OperationRequest => {
        derives.insert(DeriveTrait::Validate);
      }
      StructKind::Schema => {
        derives.insert(DeriveTrait::PartialEq);
        if self.is_serializable() == SerdeImpl::Derive {
          derives.insert(DeriveTrait::Serialize);
          if self.has_validation_attrs() {
            derives.insert(DeriveTrait::Validate);
          }
        }
        if self.is_deserializable() == SerdeImpl::Derive {
          derives.insert(DeriveTrait::Deserialize);
        }
      }
      StructKind::PathParams | StructKind::HeaderParams => {
        derives.insert(DeriveTrait::PartialEq);
        if self.has_validation_attrs() {
          derives.insert(DeriveTrait::Validate);
        }
      }
      StructKind::QueryParams => {
        derives.insert(DeriveTrait::PartialEq);
        derives.insert(DeriveTrait::Serialize);
        if self.has_validation_attrs() {
          derives.insert(DeriveTrait::Validate);
        }
      }
    }

    derives
  }

  fn is_serializable(&self) -> SerdeImpl {
    match self.kind {
      StructKind::OperationRequest | StructKind::PathParams | StructKind::HeaderParams => SerdeImpl::None,
      StructKind::QueryParams => SerdeImpl::Derive,
      StructKind::Schema => match self.serde_mode {
        SerdeMode::SerializeOnly | SerdeMode::Both => SerdeImpl::Derive,
        SerdeMode::DeserializeOnly => SerdeImpl::None,
      },
    }
  }

  fn is_deserializable(&self) -> SerdeImpl {
    match self.kind {
      StructKind::OperationRequest | StructKind::PathParams | StructKind::HeaderParams | StructKind::QueryParams => {
        SerdeImpl::None
      }
      StructKind::Schema => match self.serde_mode {
        SerdeMode::DeserializeOnly | SerdeMode::Both => SerdeImpl::Derive,
        SerdeMode::SerializeOnly => SerdeImpl::None,
      },
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
      SerdeMode::DeserializeOnly => SerdeImpl::None,
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
      SerdeMode::SerializeOnly => SerdeImpl::None,
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
      SerdeMode::DeserializeOnly => SerdeImpl::None,
    }
  }

  fn is_deserializable(&self) -> SerdeImpl {
    match self.serde_mode {
      SerdeMode::DeserializeOnly | SerdeMode::Both => SerdeImpl::Custom,
      SerdeMode::SerializeOnly => SerdeImpl::None,
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

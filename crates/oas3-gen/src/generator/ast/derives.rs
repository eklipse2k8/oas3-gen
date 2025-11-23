use std::collections::BTreeSet;

use strum::Display;

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

pub fn default_struct_derives() -> BTreeSet<DeriveTrait> {
  [
    DeriveTrait::Debug,
    DeriveTrait::Clone,
    DeriveTrait::PartialEq,
    DeriveTrait::Default,
  ]
  .into_iter()
  .collect()
}

pub fn default_enum_derives(is_simple: bool) -> BTreeSet<DeriveTrait> {
  let mut derives: BTreeSet<_> = [
    DeriveTrait::Debug,
    DeriveTrait::Clone,
    DeriveTrait::PartialEq,
    DeriveTrait::Serialize,
    DeriveTrait::Deserialize,
    DeriveTrait::Default,
  ]
  .into_iter()
  .collect();

  if is_simple {
    derives.insert(DeriveTrait::Eq);
    derives.insert(DeriveTrait::Hash);
  }

  derives
}

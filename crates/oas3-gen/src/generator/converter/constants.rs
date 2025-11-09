pub(crate) mod serde_attrs {
  pub(crate) const DEFAULT: &str = "default";
  pub(crate) const FLATTEN: &str = "flatten";
  pub(crate) const SKIP: &str = "skip";
  pub(crate) const SKIP_DESERIALIZING: &str = "skip_deserializing";
  pub(crate) const DENY_UNKNOWN_FIELDS: &str = "deny_unknown_fields";
}

pub(crate) mod doc_attrs {
  pub(crate) const HIDDEN: &str = "#[doc(hidden)]";
}

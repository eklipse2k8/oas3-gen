pub(crate) mod doc_attrs {
  pub(crate) const HIDDEN: &str = "#[doc(hidden)]";
}

pub(crate) const REQUEST_SUFFIX: &str = "Request";
pub(crate) const REQUEST_BODY_SUFFIX: &str = "RequestBody";
pub(crate) const RESPONSE_SUFFIX: &str = "Response";
pub(crate) const BODY_FIELD_NAME: &str = "body";
pub(crate) const SUCCESS_RESPONSE_PREFIX: char = '2';

pub(crate) const REQUEST_PARAMS_SUFFIX: &str = "Params";
pub(crate) const RESPONSE_ENUM_SUFFIX: &str = "Enum";
pub(crate) const DISCRIMINATED_BASE_SUFFIX: &str = "Base";
pub(crate) const MERGED_SCHEMA_CACHE_SUFFIX: &str = "_merged";
pub(crate) const RESPONSE_PREFIX: &str = "Response";

pub(crate) const DEFAULT_RESPONSE_VARIANT: &str = "Unknown";
pub(crate) const DEFAULT_RESPONSE_DESCRIPTION: &str = "Unknown response";

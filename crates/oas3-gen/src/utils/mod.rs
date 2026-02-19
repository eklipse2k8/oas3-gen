pub mod refs;
pub mod schema_ext;
pub mod spec;

pub(crate) use refs::{
  UnionFingerprints, build_union_fingerprints, extract_schema_ref_name, extract_union_fingerprint,
  parse_schema_ref_path,
};
pub(crate) use schema_ext::{SchemaExt, variant_is_nullable};

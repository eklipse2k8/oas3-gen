use crate::generator::ast::{FieldNameToken, StructDef, TypeRef};

#[derive(Debug, Clone)]
pub(crate) struct StructSummary {
  pub has_default: bool,
  pub required_fields: Vec<(FieldNameToken, TypeRef)>,
  pub user_fields: Vec<(FieldNameToken, TypeRef)>,
}

impl From<&StructDef> for StructSummary {
  fn from(struct_def: &StructDef) -> Self {
    Self {
      has_default: struct_def.has_default(),
      required_fields: struct_def
        .required_fields()
        .map(|field| (field.name.clone(), field.rust_type.clone()))
        .collect(),
      user_fields: struct_def
        .fields
        .iter()
        .filter(|field| !field.doc_hidden)
        .map(|field| (field.name.clone(), field.rust_type.clone()))
        .collect(),
    }
  }
}

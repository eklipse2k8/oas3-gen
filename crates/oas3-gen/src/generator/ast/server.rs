use super::{Documentation, EnumToken, FileHeaderNode, MethodNameToken, StructToken};
use crate::generator::ast::tokens::TraitToken;

#[derive(Debug, Clone, Default, PartialEq, Eq, bon::Builder)]
pub struct ServerTraitMethod {
  pub name: MethodNameToken,
  #[builder(default)]
  pub docs: Documentation,
  pub request_type: Option<StructToken>,
  pub response_type: Option<EnumToken>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, bon::Builder)]
pub struct ServerRequestTraitDef {
  pub name: TraitToken,
  #[builder(default)]
  pub docs: Documentation,
  #[builder(default)]
  pub methods: Vec<ServerTraitMethod>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, bon::Builder)]
pub struct ServerRootNode {
  pub header: FileHeaderNode,
}

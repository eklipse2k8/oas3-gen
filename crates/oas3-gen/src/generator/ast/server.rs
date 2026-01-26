use http::Method;

use super::{
  ContentCategory, Documentation, EnumToken, FileHeaderNode, MethodNameToken, ParsedPath, StructToken, TypeRef,
};
use crate::generator::ast::tokens::TraitToken;

#[derive(Debug, Clone, Default, PartialEq, Eq, bon::Builder)]
pub struct HandlerBodyInfo {
  pub body_type: TypeRef,
  pub content_category: ContentCategory,
  #[builder(default)]
  pub optional: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, bon::Builder)]
pub struct ServerTraitMethod {
  pub name: MethodNameToken,
  #[builder(default)]
  pub docs: Documentation,
  pub request_type: Option<StructToken>,
  pub response_type: Option<EnumToken>,
  pub http_method: Method,
  pub path: ParsedPath,
  pub path_params_type: Option<StructToken>,
  pub query_params_type: Option<StructToken>,
  pub header_params_type: Option<StructToken>,
  pub body_info: Option<HandlerBodyInfo>,
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

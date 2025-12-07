use std::collections::{BTreeMap, BTreeSet};

use super::StructToken;

#[derive(Debug, Clone)]
pub struct LinkDef {
  pub name: String,
  pub target_operation_id: String,
  pub parameters: BTreeMap<String, RuntimeExpression>,
  pub description: Option<String>,
  pub server_url: Option<String>,
}

#[derive(Debug, Clone)]
pub enum RuntimeExpression {
  ResponseBodyPath { json_pointer: String },
  RequestQueryParam { name: String },
  RequestPathParam { name: String },
  RequestHeader { name: String },
  RequestBody { json_pointer: Option<String> },
  Literal { value: String },
  Unsupported,
}

#[derive(Debug, Clone)]
pub struct ResolvedLink {
  pub link_def: LinkDef,
  pub target_request_type: StructToken,
}

#[derive(Debug, Clone)]
pub struct ResponseVariantLinks {
  pub links_struct_name: StructToken,
  pub resolved_links: Vec<ResolvedLink>,
  pub response_body_fields: BTreeSet<String>,
}

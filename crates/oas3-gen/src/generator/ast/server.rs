use crate::generator::ast::FileHeaderNode;

/// Contains AST nodes related to server code generation.
#[derive(Debug, Clone, Default, PartialEq, Eq, bon::Builder)]
pub struct ServerRootNode {
  pub header: FileHeaderNode,
}

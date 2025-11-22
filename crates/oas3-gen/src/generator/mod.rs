#![allow(clippy::struct_excessive_bools)]

pub(crate) mod analyzer;
pub(crate) mod ast;
pub(crate) mod codegen;
pub(crate) mod converter;
pub mod operation_registry;
pub mod orchestrator;
pub(crate) mod schema_graph;
pub(crate) mod utils;

#[cfg(test)]
mod tests;

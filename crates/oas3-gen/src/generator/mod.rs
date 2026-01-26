#![allow(clippy::struct_excessive_bools)]

pub(crate) mod ast;
pub(crate) mod codegen;
pub(crate) mod converter;
pub(crate) mod naming;
pub mod operation_registry;
pub mod orchestrator;
pub(crate) mod postprocess;
pub(crate) mod schema_registry;

#[cfg(test)]
mod tests;

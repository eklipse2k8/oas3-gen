#![allow(clippy::struct_excessive_bools)]

pub(crate) mod ast;
pub(crate) mod codegen;
pub(crate) mod converter;
pub mod metrics;
pub mod mode;
pub(crate) mod naming;
pub mod operation_registry;
pub mod orchestrator;
pub(crate) mod postprocess;
pub(crate) mod schema_registry;

pub use converter::{
  CodegenConfig, EnumCasePolicy, EnumDeserializePolicy, EnumHelperPolicy, GenerationTarget, ODataPolicy,
};
pub use mode::{ClientModMode, ClientMode, GenerationMode, ServerModMode, TypesMode};

#[cfg(test)]
mod tests;

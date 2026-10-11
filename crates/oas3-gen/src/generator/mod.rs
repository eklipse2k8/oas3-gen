#![allow(clippy::struct_excessive_bools)]

pub mod ast;
pub mod codegen;
pub mod converter;
pub mod metrics;
pub mod mode;
pub mod naming;
pub mod operation_registry;
pub mod orchestrator;
pub mod postprocess;
pub mod schema_registry;

pub use converter::{
  CodegenConfig, CollectionTypePolicy, EnumCasePolicy, EnumDeserializePolicy, EnumHelperPolicy, EnumLayoutPolicy,
  GenerationTarget, HeaderScope, ODataPolicy, SchemaScope,
};
pub use mode::{ClientModMode, ClientMode, GenerationMode, ServerModMode, TypesMode};

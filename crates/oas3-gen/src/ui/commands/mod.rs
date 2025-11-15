#![allow(clippy::fn_params_excessive_bools)]
#![allow(clippy::struct_excessive_bools)]

pub mod generate;
pub mod list;

pub use generate::{GenerateConfig, generate_code};
pub use list::list_operations;

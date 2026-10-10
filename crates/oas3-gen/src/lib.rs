//! OpenAPI 3.1 to Rust code generator.
//!
//! The pipeline parses a spec, converts it to a Rust AST, and generates Rust source. The
//! `oas3-gen` binary drives it through [`ui`]; [`generator`] holds the pipeline and [`utils`]
//! the spec helpers it shares.
#![allow(clippy::doc_markdown)]
#![allow(clippy::missing_errors_doc)]
#![allow(clippy::missing_panics_doc)]
#![allow(clippy::must_use_candidate)]
#![allow(clippy::return_self_not_must_use)]
#![allow(clippy::too_many_lines)]

pub mod generator;
pub mod ui;
pub mod utils;

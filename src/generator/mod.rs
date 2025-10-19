//! OpenAPI to Rust code generator
//!
//! This module provides functionality for converting OpenAPI 3.x specifications into
//! idiomatic Rust code, including type definitions, validation, and serde support.
//!
//! ## Module Structure
//!
//! - [`utils`] - Helper functions for documentation generation
//! - [`ast`] - Abstract Syntax Tree types for representing Rust code
//! - [`schema_graph`] - Schema dependency tracking and cycle detection
//! - [`schema_converter`] - Converts OpenAPI schemas to Rust AST
//! - [`operation_converter`] - Converts OpenAPI operations to request/response types
//! - [`code_generator`] - Generates Rust source code from AST

#![allow(dead_code)]

// Declare sub-modules
mod ast;
mod code_generator;
mod operation_converter;
mod schema_converter;
mod schema_graph;
mod utils;

// Re-export public API
pub use code_generator::CodeGenerator;
pub use operation_converter::OperationConverter;
pub use schema_converter::SchemaConverter;
pub use schema_graph::SchemaGraph;

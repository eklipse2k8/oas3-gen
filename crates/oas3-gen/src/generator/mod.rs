//! OpenAPI to Rust code generator
//!
//! This module provides functionality for converting OpenAPI 3.x specifications into
//! idiomatic Rust code, including type definitions, validation, and serde support.
//!
//! ## Module Structure
//!
//! - [`orchestrator`] - High-level pipeline orchestration (public API)
//! - [`utils`] - Helper functions for documentation generation
//! - [`ast`] - Abstract Syntax Tree types for representing Rust code
//! - [`schema_graph`] - Schema dependency tracking and cycle detection
//! - [`schema_converter`] - Converts OpenAPI schemas to Rust AST
//! - [`operation_converter`] - Converts OpenAPI operations to request/response types
//! - [`code_generator`] - Generates Rust source code from AST
//!
//! ## Public API
//!
//! The primary entry point for code generation is the [`orchestrator::Orchestrator`] struct,
//! which provides a simple, opaque interface for generating Rust code from OpenAPI specs.
//! All internal types and conversion logic are private implementation details.

// Declare sub-modules (all internal except orchestrator)
pub(crate) mod ast;
pub(crate) mod code_generator;
pub(crate) mod operation_converter;
pub mod orchestrator;
pub(crate) mod schema_converter;
pub(crate) mod schema_graph;
pub(crate) mod utils;

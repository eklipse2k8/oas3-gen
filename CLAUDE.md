# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

OpenAPI-to-Rust code generator that parses OpenAPI 3.1 specifications and generates comprehensive Rust type definitions with validation.

**Workspace crates:**

- **oas3-gen**: Main CLI tool for code generation
- **oas3-gen-support**: Runtime support library for generated code

## Quick Start

```bash
cargo build                    # Build
cargo test                     # Test
cargo run -- generate types -i spec.json -o types.rs   # Generate types
cargo run -- generate client -i spec.json -o client.rs # Generate client
```

## Essential Rules

1. **NO inline comments** - Code must be self-documenting
2. **NO emojis** - Token conservation
3. **Run tests** before committing: `cargo test`
4. **Rebuild fixtures** after code generation changes (see [testing.md](docs/testing.md))

## Detailed Documentation

| Document | Contents |
|----------|----------|
| [docs/coding-standards.md](docs/coding-standards.md) | Naming conventions, patterns, collection types, SOLID principles |
| [docs/commands.md](docs/commands.md) | All CLI commands, options, linting, profiling |
| [docs/testing.md](docs/testing.md) | Test requirements, fixtures, coverage, debugging |
| [docs/architecture.md](docs/architecture.md) | Pipeline stages, directory structure, dependencies |
| [docs/subagents.md](docs/subagents.md) | Specialized subagents for performance, review, testing, CLI, docs |

## Pipeline Overview

```text
Parse OpenAPI -> Analyze (dependency graph) -> Convert (AST) -> Generate (Rust)
```

**Key modules:**

- `analyzer/` - Schema analysis, validation, type usage tracking
- `naming/` - Identifier generation, type name inference
- `converter/` - OpenAPI to AST transformation
- `codegen/` - AST to Rust source generation

## Key Files

- [orchestrator.rs](crates/oas3-gen/src/generator/orchestrator.rs) - Pipeline coordinator
- [type_resolver.rs](crates/oas3-gen/src/generator/converter/type_resolver.rs) - OpenAPI to Rust type mapping
- [identifiers.rs](crates/oas3-gen/src/generator/naming/identifiers.rs) - Identifier sanitization

## Collection Types (Critical)

- **IndexMap** - Operations/endpoints (preserves spec order)
- **BTreeMap** - Types/schemas (alphabetical, deterministic)
- **HashMap** - NEVER for code generation order

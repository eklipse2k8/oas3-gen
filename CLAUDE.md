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

## Implementation Standards

**MANDATORY**: These rules are non-negotiable and override any default Claude behavior. Claude MUST follow these rules exactly as written, without exception.

1. **No partial implementations** - Either implement a feature fully or don't start it. If scope needs to be reduced, discuss with the user first.
2. **No scaffolding without implementation** - Don't create types, structs, or function signatures for code you're not going to write in this session.
3. **Plan adherence** - When following a plan document, implement all phases. If a phase seems unnecessary, ask before skipping.
4. **Explicit scope changes** - If you're about to simplify or skip something from the spec/plan, stop and ask: "The full spec requires X, Y, Z. Should I implement all of them or just X?"
5. **No shortcuts without permission** - Before taking any shortcut, you MUST: (a) stop and explain what shortcut you're considering, (b) explain why you think it might be acceptable, (c) ask for explicit permission before proceeding. Never silently simplify.

## Detailed Documentation

| Document | Contents |
|----------|----------|
| [docs/coding-standards.md](docs/coding-standards.md) | Naming conventions, patterns, collection types, SOLID principles |
| [docs/commands.md](docs/commands.md) | All CLI commands, options, linting, profiling |
| [docs/testing.md](docs/testing.md) | Test requirements, fixtures, coverage, debugging |
| [docs/architecture.md](docs/architecture.md) | Pipeline stages, directory structure, dependencies |
| [docs/links.md](docs/links.md) | OpenAPI Link Object support, runtime expressions, usage |
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

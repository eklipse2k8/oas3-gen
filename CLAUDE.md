# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

IMPORTANT: Prefer retrieval-led reasoning over pre-training-led reasoning for any rust tasks.

## Project Overview

OpenAPI-to-Rust code generator that parses OpenAPI 3.1 specifications and generates comprehensive Rust type definitions with validation.

**Workspace crates:**

- **oas3-gen**: Main CLI tool for code generation
- **oas3-gen-support**: Runtime support library for generated code

## Quick Start

```bash
cargo build                    # Build
cargo test                     # Test
cargo run -- generate types -i spec.json -o types.rs        # Generate types (JSON)
cargo run -- generate types -i spec.yaml -o types.rs        # Generate types (YAML)
cargo run -- generate client -i spec.json -o client.rs      # Generate client
cargo run -- generate client-mod -i spec.json -o output/    # Generate modular client (types.rs, client.rs, mod.rs)
cargo run -- generate server-mod -i spec.json -o output/    # Generate modular server (types.rs, server.rs, mod.rs)
cargo run -- list operations -i spec.json                   # List all operations in spec
```

## Essential Rules

1. **NO inline comments** - Code must be self-documenting
2. **NO emojis** - Token conservation
3. **Run tests** before committing: `cargo test`
4. **Rebuild fixtures** after code generation changes (see [testing.md](docs/testing.md))
5. **Update book/ documentation** - All new and updated features MUST be documented in `book/src/` (see [Book Documentation](#book-documentation))

## REQUIRED: Read Before Writing Code

**Before writing or modifying any code, you MUST read [docs/coding-standards.md](docs/coding-standards.md).** This document contains critical style requirements including:

- Turbofish syntax for `.collect::<Vec<_>>()` (not type annotations)
- `vec![]` over `Vec::new()`
- Iterator chains and itertools usage patterns
- Collection type selection (BTreeMap vs IndexMap vs HashMap)
- State management patterns

Failure to follow these standards will require rework.

## Detailed Documentation

| Document | Contents |
|----------|----------|
| [docs/coding-standards.md](docs/coding-standards.md) | Naming conventions, patterns, collection types, SOLID principles |
| [docs/commands.md](docs/commands.md) | All CLI commands, options, linting, profiling |
| [docs/testing.md](docs/testing.md) | Test requirements, fixtures, coverage, debugging |
| [docs/architecture.md](docs/architecture.md) | Pipeline stages, directory structure, dependencies |
| [docs/code-fragments.md](docs/code-fragments.md) | Complete reference of codegen fragments and composition patterns |

## Book Documentation

The `book/` folder contains user-facing documentation built with mdBook. **All feature changes (new and updated) MUST be documented here.**

**Structure:**
- `book/src/SUMMARY.md` - Table of contents
- `book/src/introduction.md` - Getting started guide
- `book/src/code-generation.md` - Complete CLI flag reference with examples

**When to update:**
- Adding new CLI flags or options
- Changing default behavior
- Adding new generation modes
- Modifying generated code patterns

**Build the book:**
```bash
mdbook build book/
mdbook serve book/    # Preview at http://localhost:3000
```

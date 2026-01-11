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
cargo run -- generate types -i spec.json -o types.rs        # Generate types (JSON)
cargo run -- generate types -i spec.yaml -o types.rs        # Generate types (YAML)
cargo run -- generate client -i spec.json -o client.rs      # Generate client
cargo run -- generate client-mod -i spec.json -o output/    # Generate modular client (types.rs, client.rs, mod.rs)
cargo run -- generate server-mod -i spec.json -o output/    # Generate modular server (types.rs, server.rs, mod.rs)
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
| [docs/code-fragments.md](docs/code-fragments.md) | Complete reference of codegen fragments and composition patterns |

## One-Way Data Flow Pipeline

The generator follows a strict one-way data flow where each stage produces immutable outputs consumed by the next:

```text
┌─────────────────────────────────────────────────────────────────────────────┐
│                              ORCHESTRATOR                                    │
│  Entry point: collect_generation_artifacts() -> GenerationArtifacts         │
└─────────────────────────────────────────────────────────────────────────────┘
                                     │
                                     ▼
┌─────────────────────────────────────────────────────────────────────────────┐
│ STAGE 1: REGISTRY INITIALIZATION                                            │
│ ┌─────────────────────┐    ┌────────────────────────────────────────────┐  │
│ │   SchemaRegistry    │───▶│ • Resolves all $refs from spec             │  │
│ │  (schema_registry)  │    │ • Builds dependency graph                  │  │
│ └─────────────────────┘    │ • Detects cycles (marks cyclic schemas)    │  │
│           │                │ • Computes inheritance depths              │  │
│           │                │ • Merges allOf schemas                     │  │
│           │                │ • Builds discriminator cache               │  │
│           ▼                │ • Creates union fingerprints               │  │
│ ┌─────────────────────┐    └────────────────────────────────────────────┘  │
│ │ OperationRegistry   │───▶ Filters operations by --only/--exclude opts    │
│ └─────────────────────┘                                                     │
└─────────────────────────────────────────────────────────────────────────────┘
                                     │
                                     ▼
┌─────────────────────────────────────────────────────────────────────────────┐
│ STAGE 2: SCHEMA INTROSPECTION & CACHING                                     │
│ ┌─────────────────────┐    ┌────────────────────────────────────────────┐  │
│ │     SchemaExt       │───▶│ • Schema type predicates (is_array, etc.)  │  │
│ │ (utils/schema_ext)  │    │ • Union variant queries                    │  │
│ └─────────────────────┘    │ • Enum value extraction                    │  │
│           │                │ • Name inference from context              │  │
│           ▼                └────────────────────────────────────────────┘  │
│ ┌─────────────────────┐                                                     │
│ │ SharedSchemaCache   │───▶ Deduplicates types by schema hash              │
│ │     (cache.rs)      │    (ensures deterministic naming across runs)      │
│ └─────────────────────┘                                                     │
└─────────────────────────────────────────────────────────────────────────────┘
                                     │
                                     ▼
┌─────────────────────────────────────────────────────────────────────────────┐
│ STAGE 3: SCHEMA CONVERSION                                                  │
│ ┌─────────────────────┐    ┌────────────────────────────────────────────┐  │
│ │  SchemaConverter    │───▶│ • CodegenConfig controls behavior          │  │
│ │ (converter/mod.rs)  │    │   (enum case, helpers, serde, odata)       │  │
│ └─────────────────────┘    └────────────────────────────────────────────┘  │
│           │                                                                  │
│           ├──▶ StructConverter  ──▶ StructDef (with fields, methods)       │
│           ├──▶ EnumConverter    ──▶ EnumDef (value enums)                  │
│           ├──▶ UnionConverter   ──▶ DiscriminatedEnumDef / EnumDef         │
│           └──▶ TypeResolver     ──▶ TypeRef (type references)              │
│                     │                                                        │
│                     └──▶ InlineTypeResolver (cache-aware inline creation)  │
│                                                                              │
│ OUTPUT: Vec<RustType> (schemas sorted: enums first, then others)           │
└─────────────────────────────────────────────────────────────────────────────┘
                                     │
                                     ▼
┌─────────────────────────────────────────────────────────────────────────────┐
│ STAGE 4: OPERATION CONVERSION                                               │
│ ┌─────────────────────┐    ┌────────────────────────────────────────────┐  │
│ │ OperationConverter  │───▶│ • Converts parameters → OperationParameter │  │
│ │   (operations.rs)   │    │ • Converts request body → StructDef        │  │
│ └─────────────────────┘    │ • Converts responses → ResponseEnumDef     │  │
│           │                │ • Records type usage via TypeUsageRecorder │  │
│           ▼                └────────────────────────────────────────────┘  │
│ ┌─────────────────────┐                                                     │
│ │ TypeUsageRecorder   │───▶ Tracks: type → (in_request, in_response)       │
│ └─────────────────────┘                                                     │
│                                                                              │
│ OUTPUT: Vec<OperationInfo> + additional Vec<RustType> + usage_recorder     │
└─────────────────────────────────────────────────────────────────────────────┘
                                     │
                                     ▼
┌─────────────────────────────────────────────────────────────────────────────┐
│ STAGE 5: TYPE ANALYSIS (TypeAnalyzer)                                       │
│ ┌─────────────────────┐    ┌────────────────────────────────────────────┐  │
│ │    TypeAnalyzer     │───▶│ • Builds DependencyGraph from types        │  │
│ │  (analyzer/mod.rs)  │    │ • Propagates usage through dependencies    │  │
│ └─────────────────────┘    │ • Updates SerdeMode per type:              │  │
│                            │   - RequestOnly → SerializeOnly            │  │
│                            │   - ResponseOnly → DeserializeOnly         │  │
│                            │   - Bidirectional → Both                   │  │
│                            │ • Deduplicates identical ResponseEnums     │  │
│                            │ • Adds #[validate(nested)] transitively    │  │
│                            └────────────────────────────────────────────┘  │
│                                                                              │
│ OUTPUT: Mutated Vec<RustType>                                              │
└─────────────────────────────────────────────────────────────────────────────┘
                                     │
                                     ▼
┌─────────────────────────────────────────────────────────────────────────────┐
│ STAGE 6: CODE GENERATION (codegen/)                                         │
│ ┌─────────────────────┐    ┌────────────────────────────────────────────┐  │
│ │ codegen::generate() │───▶│ • deduplicate_and_order_types() by name    │  │
│ │    (codegen/mod)    │    │   (BTreeMap ensures alphabetical order)    │  │
│ └─────────────────────┘    │ • generate_regex_constants() extracts      │  │
│           │                │   patterns for const REGEX_* declarations  │  │
│           │                │ • Generates imports (serde, validator)     │  │
│           ▼                └────────────────────────────────────────────┘  │
│ ┌─────────────────────┐                                                     │
│ │  Type Generators    │                                                     │
│ │  StructGenerator    │───▶ Struct with derives, serde attrs, methods      │
│ │  EnumGenerator      │───▶ Enum with variants, case-insensitive deser     │
│ │  DiscriminatedEnum  │───▶ Tagged union with macro-based serde            │
│ │  ResponseEnum       │───▶ HTTP response enum with status variants        │
│ │  TypeAlias          │───▶ type Foo = Bar;                                │
│ └─────────────────────┘                                                     │
│           │                                                                  │
│           ▼                                                                  │
│ ┌─────────────────────┐                                                     │
│ │ generate_source()   │───▶ Add header, format with prettyplease           │
│ └─────────────────────┘                                                     │
│                                                                              │
│ OUTPUT: String (formatted Rust source code)                                │
└─────────────────────────────────────────────────────────────────────────────┘
```

**Key Principle:** Data flows forward only. Each stage consumes outputs from previous stages without back-references. The `SharedSchemaCache` enables deduplication within the conversion stage but doesn't feed back to earlier stages.

## Key Modules

| Module                              | Responsibility                                                      |
| ----------------------------------- | ------------------------------------------------------------------- |
| `orchestrator.rs`                   | Pipeline coordinator, creates all stages, combines outputs          |
| `schema_registry.rs`                | Schema storage, dependency graph, cycle detection, discriminator    |
| `utils/schema_ext.rs`               | `SchemaExt` trait: schema query predicates and name inference       |
| `converter/mod.rs`                  | `SchemaConverter`, `ConverterContext`, `CodegenConfig` policy enums |
| `converter/type_resolver.rs`        | Central conversion logic: maps OpenAPI types to Rust `TypeRef`      |
| `converter/inline_resolver.rs`      | Cache-aware inline type creation (structs, enums, unions)           |
| `converter/cache.rs`                | Type deduplication with focused registries (name, schema, enum, union) |
| `converter/common.rs`               | `ConversionOutput<T>` wrapper for inline type tracking              |
| `converter/hashing.rs`              | Schema hashing utilities for cache keying                           |
| `converter/unions.rs`               | `UnionConverter` for oneOf/anyOf handling                           |
| `converter/union_types.rs`          | Shared union types: `UnionKind`, `CollisionStrategy`, variant specs |
| `converter/discriminator.rs`        | Discriminator-based union handling                                  |
| `converter/value_enums.rs`          | Builds value enums from entries with collision handling             |
| `converter/variants.rs`             | Builds union variant definitions (ref, inline, const)               |
| `converter/relaxed_enum.rs`         | Builds anyOf enums with known values + freeform string              |
| `converter/methods.rs`              | Generates helper constructor methods for enum variants              |
| `converter/type_usage_recorder.rs`  | `TypeUsageRecorder` for tracking request/response type usage        |
| `naming/inference.rs`               | Variant prefix extraction and name deduplication helpers            |
| `naming/identifiers.rs`             | Identifier sanitization (reserved words, casing)                    |
| `naming/name_index.rs`              | Name indexing for conflict resolution                               |
| `naming/operations.rs`              | Operation-specific naming logic                                     |
| `naming/responses.rs`               | Response-specific naming logic                                      |
| `analyzer/mod.rs`                   | `TypeAnalyzer`: usage propagation, serde modes, error schemas       |
| `analyzer/dependency_graph.rs`      | Type dependency tracking for analysis                               |
| `codegen/mod.rs`                    | Entry point for Rust code generation                                |
| `codegen/structs.rs`                | Struct code generation with derives and serde attrs                 |
| `codegen/enums.rs`                  | Enum code generation with case-insensitive deser support            |
| `codegen/client.rs`                 | HTTP client code generation                                         |
| `codegen/server.rs`                 | HTTP server trait generation (axum)                                 |
| `codegen/mod_file.rs`               | Modular output file generation (mod.rs)                             |
| `ast/mod.rs`                        | AST types: `RustType`, `StructDef`, `EnumDef`, `TypeRef`, etc.      |
| `ast/server.rs`                     | Server AST definitions (`ServerRootNode`)                           |

## Key Files

- [orchestrator.rs](crates/oas3-gen/src/generator/orchestrator.rs) - Pipeline coordinator
- [schema_registry.rs](crates/oas3-gen/src/generator/schema_registry.rs) - Dependency graph and cycle detection
- [utils/schema_ext.rs](crates/oas3-gen/src/utils/schema_ext.rs) - SchemaExt trait for schema queries and inference
- [converter/mod.rs](crates/oas3-gen/src/generator/converter/mod.rs) - SchemaConverter and ConverterContext
- [converter/type_resolver.rs](crates/oas3-gen/src/generator/converter/type_resolver.rs) - Central OpenAPI to Rust type conversion
- [converter/inline_resolver.rs](crates/oas3-gen/src/generator/converter/inline_resolver.rs) - Cache-aware inline type creation
- [converter/cache.rs](crates/oas3-gen/src/generator/converter/cache.rs) - Type deduplication with focused registries
- [converter/unions.rs](crates/oas3-gen/src/generator/converter/unions.rs) - oneOf/anyOf conversion
- [converter/variants.rs](crates/oas3-gen/src/generator/converter/variants.rs) - Union variant building
- [naming/inference.rs](crates/oas3-gen/src/generator/naming/inference.rs) - Variant prefix extraction helpers
- [naming/identifiers.rs](crates/oas3-gen/src/generator/naming/identifiers.rs) - Identifier sanitization
- [analyzer/mod.rs](crates/oas3-gen/src/generator/analyzer/mod.rs) - TypeAnalyzer and serde mode computation
- [codegen/mod.rs](crates/oas3-gen/src/generator/codegen/mod.rs) - Code generation entry point
- [codegen/client.rs](crates/oas3-gen/src/generator/codegen/client.rs) - HTTP client generation
- [codegen/server.rs](crates/oas3-gen/src/generator/codegen/server.rs) - HTTP server trait generation
- [ast/mod.rs](crates/oas3-gen/src/generator/ast/mod.rs) - AST type definitions
- [ast/server.rs](crates/oas3-gen/src/generator/ast/server.rs) - Server AST definitions
- [operation_registry.rs](crates/oas3-gen/src/generator/operation_registry.rs) - HTTP operations and webhooks

## Collection Types (Critical)

- **IndexMap** - Operations/endpoints (preserves spec order)
- **BTreeMap** - Types/schemas (alphabetical, deterministic)
- **HashMap** - NEVER for code generation order

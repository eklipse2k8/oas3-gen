# Architecture

Cargo workspace with two crates following a three-stage pipeline: **Parse OpenAPI -> Convert to AST -> Generate Rust Code**

## Directory Structure

```text
book/                             # User documentation (mdBook)
│   ├── src/
│   │   ├── SUMMARY.md            # Table of contents
│   │   ├── introduction.md       # Getting started guide
│   │   └── code-generation.md    # CLI flag reference
│   └── book.toml                 # mdBook configuration
docs/                             # Internal developer documentation
│   ├── architecture.md           # This file
│   ├── coding-standards.md       # Style and patterns
│   ├── commands.md               # CLI reference
│   ├── testing.md                # Test requirements
│   └── code-fragments.md         # Codegen fragment reference
crates/
├── oas3-gen/                      # CLI tool (binary)
│   ├── fixtures/                  # Test fixtures (JSON and YAML)
│   │   ├── basic_api.json         # Basic API test fixture
│   │   ├── content_types.json     # Content type handling tests
│   │   ├── enum_deduplication.json # Enum deduplication tests
│   │   ├── event_stream.json      # Server-sent events streaming tests
│   │   ├── implicit_union.json    # Implicit union tests
│   │   ├── integer_path_param.json # Integer path parameter tests
│   │   ├── intersection_union.json # Intersection/union type tests
│   │   ├── Lizard.json            # Discriminator/inheritance tests
│   │   ├── oas_3_1_2_pet_benchmark.json # Benchmark specification
│   │   ├── operation_filtering.json # Operation filtering tests
│   │   ├── petstore.json          # Petstore API specification
│   │   ├── relaxed_enum_deduplication.json # Relaxed enum deduplication tests
│   │   ├── schema.yaml            # YAML format test fixture
│   │   ├── undeclared_path_params.json # Undeclared path parameter tests
│   │   ├── union_serde.json       # Union serialization tests
│   │   ├── untyped_parameter.json # Untyped parameter tests
│   │   ├── event_stream/          # Event stream generated output fixtures
│   │   │   ├── mod.rs
│   │   │   ├── client.rs
│   │   │   └── types.rs
│   │   ├── intersection_union/    # Intersection union generated output fixtures
│   │   │   ├── mod.rs
│   │   │   ├── client.rs
│   │   │   └── types.rs
│   │   ├── petstore/              # Petstore client generated output fixtures
│   │   │   ├── mod.rs
│   │   │   ├── client.rs
│   │   │   └── types.rs
│   │   ├── petstore_server/       # Petstore server generated output fixtures
│   │   │   ├── mod.rs
│   │   │   ├── server.rs
│   │   │   └── types.rs
│   │   └── union_serde/           # Union serde generated output fixtures
│   │       ├── mod.rs
│   │       ├── client.rs
│   │       └── types.rs
│   └── src/
│       ├── main.rs                # Entry point
│       ├── ui/                    # CLI interface
│       │   ├── mod.rs
│       │   ├── cli.rs             # Argument definitions
│       │   ├── colors.rs          # Terminal theming
│       │   └── commands/          # Command handlers
│       │       ├── mod.rs
│       │       ├── generate.rs
│       │       └── list.rs
│       ├── utils/                 # Cross-cutting utilities
│       │   ├── mod.rs
│       │   ├── schema_ext.rs      # SchemaExt trait for schema queries and inference
│       │   ├── spec.rs            # Spec loading utilities
│       │   └── text.rs            # Text processing utilities
│       ├── tests/                 # Integration test utilities
│       │   ├── mod.rs
│       │   ├── common.rs          # Common test helpers
│       │   ├── petstore.rs        # Petstore integration tests
│       │   └── union_serde.rs     # Union serialization tests
│       └── generator/             # Core generation pipeline
│           ├── mod.rs
│           ├── orchestrator.rs    # Main pipeline coordinator
│           ├── operation_registry.rs # Operation and webhook collection management
│           ├── schema_registry.rs # Dependency tracking and cycle detection
│           ├── tests/             # Generator tests
│           │   ├── mod.rs
│           │   ├── orchestrator.rs
│           │   ├── operation_registry.rs
│           │   ├── schema_graph.rs
│           │   ├── undeclared_path_params.rs
│           │   └── untyped_params.rs
│           ├── postprocess/       # Type postprocessing and refinement
│           │   ├── mod.rs         # PostprocessOutput, postprocess() orchestrator
│           │   ├── response_enum.rs  # ResponseEnumDeduplicator for deduplicating response enums
│           │   ├── serde_usage.rs    # SerdeUsage for serde mode propagation
│           │   ├── uses.rs           # RustTypeDeduplication, HeaderRefCollection, ModuleImports
│           │   ├── validation.rs     # NestedValidationProcessor for #[validate(nested)]
│           │   └── tests/         # Postprocess tests
│           │       └── mod.rs
│           ├── naming/            # Identifier naming and conversion
│           │   ├── mod.rs
│           │   ├── constants.rs   # Naming constants
│           │   ├── identifiers.rs # Rust identifier generation
│           │   ├── inference.rs   # Variant prefix extraction and deduplication
│           │   ├── name_index.rs  # Name indexing for conflict resolution
│           │   ├── operations.rs  # Operation naming
│           │   ├── responses.rs   # Response naming
│           │   └── tests/         # Naming tests
│           │       ├── mod.rs
│           │       ├── identifiers.rs
│           │       ├── inference.rs
│           │       ├── operations.rs
│           │       └── responses.rs
│           ├── ast/               # AST type definitions
│           │   ├── mod.rs
│           │   ├── types.rs       # Core AST types (RustType, StructDef, EnumDef, etc.)
│           │   ├── client.rs      # Client AST definitions
│           │   ├── constants.rs   # Constant node definitions (HttpHeaderRef)
│           │   ├── derives.rs     # Derive macro selection
│           │   ├── documentation.rs # Doc comment generation
│           │   ├── fields.rs      # Field-related AST types (FieldMeta, etc.)
│           │   ├── lints.rs       # Clippy lint attributes
│           │   ├── outer_attrs.rs # Type-safe outer attributes (skip_serializing_none, non_exhaustive)
│           │   ├── parsed_path.rs # URL path template parsing
│           │   ├── serde_attrs.rs # Serde attribute builders with ToTokens
│           │   ├── server.rs      # Server AST definitions
│           │   ├── status_codes.rs # HTTP status code handling (full RFC coverage)
│           │   ├── tokens.rs      # Token stream utilities
│           │   ├── validation_attrs.rs # Validation attribute builders with ToTokens
│           │   └── tests/         # AST tests
│           │       ├── mod.rs
│           │       ├── content_category.rs
│           │       ├── documentation.rs
│           │       ├── outer_attrs.rs
│           │       ├── parsed_path.rs
│           │       ├── status_codes.rs
│           │       ├── types.rs
│           │       └── validation_attrs.rs
│           ├── converter/         # OpenAPI -> AST conversion
│           │   ├── mod.rs         # SchemaConverter, ConverterContext, CodegenConfig
│           │   ├── cache.rs       # Type deduplication with focused registries
│           │   ├── common.rs      # ConversionOutput<T> wrapper for inline type tracking
│           │   ├── discriminator.rs # Discriminator handling for oneOf
│           │   ├── fields.rs      # Struct field conversion
│           │   ├── hashing.rs     # Schema fingerprinting for deduplication
│           │   ├── inline_resolver.rs # InlineTypeResolver for cache-aware inline type creation
│           │   ├── methods.rs     # Helper constructor methods for enum variants
│           │   ├── operations.rs  # Request/response type generation
│           │   ├── parameters.rs  # Parameter conversion
│           │   ├── relaxed_enum.rs # anyOf enums with known values + freeform
│           │   ├── requests.rs    # Request body handling
│           │   ├── responses.rs   # Response enum generation
│           │   ├── structs.rs     # Object schema conversion (includes field optionality)
│           │   ├── type_resolver.rs # Central type mapping and conversion logic
│           │   ├── type_usage_recorder.rs # Tracks request/response type usage
│           │   ├── unions.rs      # UnionConverter for oneOf/anyOf handling
│           │   ├── union_types.rs # Shared union types (UnionKind, CollisionStrategy, etc.)
│           │   ├── value_enums.rs # Builds value enums with collision handling
│           │   ├── variants.rs    # Builds union variant definitions
│           │   └── tests/         # Converter tests
│           │       ├── mod.rs
│           │       ├── cache.rs
│           │       ├── common_tests.rs
│           │       ├── enums.rs
│           │       ├── fields.rs
│           │       ├── helper_tests.rs
│           │       ├── implicit_dependencies.rs
│           │       ├── inline_objects.rs
│           │       ├── intersection_union.rs
│           │       ├── metadata_tests.rs
│           │       ├── operations.rs
│           │       ├── structs.rs
│           │       ├── type_aliases.rs
│           │       └── type_resolution.rs
│           └── codegen/           # AST -> Rust source generation
│               ├── mod.rs         # SchemaCodeGenerator, Visibility, GeneratedResult
│               ├── attributes.rs  # Attribute generation
│               ├── client.rs      # HTTP client generation (ClientFragment)
│               ├── coercion.rs    # Type coercion logic
│               ├── constants.rs   # Regex and header constant generation
│               ├── enums.rs       # Enum, DiscriminatedEnum, ResponseEnum generation
│               ├── headers.rs     # Header code generation
│               ├── http.rs        # HTTP status code fragments
│               ├── methods.rs     # Helper method fragments
│               ├── mod_file.rs    # Module file generation (mod.rs)
│               ├── server.rs      # HTTP server trait generation (ServerGenerator)
│               ├── structs.rs     # Struct code generation (StructFragment)
│               ├── type_aliases.rs # Type alias generation
│               ├── types.rs       # TypeFragment, TypesFragment for type file generation
│               └── tests/         # Codegen tests
│                   ├── mod.rs
│                   ├── client.rs
│                   ├── coercion_tests.rs
│                   ├── constants_tests.rs
│                   ├── enum_tests.rs
│                   ├── module_uses_tests.rs
│                   ├── struct_tests.rs
│                   └── type_alias_tests.rs
└── oas3-gen-support/              # Runtime library (rlib + cdylib)
    └── src/
        └── lib.rs                 # Runtime utilities for generated code
```

## Generation Pipeline (One-Way Data Flow)

The generator follows a strict one-way data flow where each stage produces immutable outputs consumed by the next:

1. **Parse**: Load OpenAPI spec via `oas3` crate (JSON or YAML, auto-detected)
2. **Registry Init**: Build `SchemaRegistry` (dependency graph, cycles, merged schemas, discriminators)
3. **Schema Introspection**: `SchemaExt` trait provides type predicates, union queries, and name inference
4. **Convert Schemas**: `SchemaConverter` with `ConverterContext` transforms schemas to `Vec<RustType>`
5. **Convert Operations**: `OperationConverter` produces `Vec<OperationInfo>` + types + usage data
6. **Postprocess**: `TypePostprocessor` propagates usage, updates serde modes, deduplicates response enums
7. **Generate**: `codegen::generate()` produces formatted Rust source code

**Key Principle:** Data flows forward only. Each stage consumes outputs from previous stages without back-references. The `SharedSchemaCache` enables deduplication within the conversion stage but doesn't feed back to earlier stages.

## Key Files

- [orchestrator.rs](../crates/oas3-gen/src/generator/orchestrator.rs): Pipeline coordinator, combines all stages
- [schema_registry.rs](../crates/oas3-gen/src/generator/schema_registry.rs): Dependency graph, cycle detection, merged schemas
- [utils/schema_ext.rs](../crates/oas3-gen/src/utils/schema_ext.rs): SchemaExt trait for schema queries and inference
- [converter/mod.rs](../crates/oas3-gen/src/generator/converter/mod.rs): SchemaConverter, ConverterContext, CodegenConfig
- [converter/type_resolver.rs](../crates/oas3-gen/src/generator/converter/type_resolver.rs): Central OpenAPI to Rust type conversion
- [converter/inline_resolver.rs](../crates/oas3-gen/src/generator/converter/inline_resolver.rs): Cache-aware inline type creation coordinator
- [converter/cache.rs](../crates/oas3-gen/src/generator/converter/cache.rs): Type deduplication with focused registries
- [converter/unions.rs](../crates/oas3-gen/src/generator/converter/unions.rs): oneOf/anyOf to discriminated enums
- [converter/variants.rs](../crates/oas3-gen/src/generator/converter/variants.rs): Union variant building (ref, inline, const)
- [naming/inference.rs](../crates/oas3-gen/src/generator/naming/inference.rs): Variant prefix extraction helpers
- [naming/identifiers.rs](../crates/oas3-gen/src/generator/naming/identifiers.rs): Identifier sanitization
- [postprocess/mod.rs](../crates/oas3-gen/src/generator/postprocess/mod.rs): Postprocess orchestrator, composes all processors
- [postprocess/serde_usage.rs](../crates/oas3-gen/src/generator/postprocess/serde_usage.rs): SerdeUsage for serde mode propagation
- [postprocess/response_enum.rs](../crates/oas3-gen/src/generator/postprocess/response_enum.rs): ResponseEnumDeduplicator
- [postprocess/uses.rs](../crates/oas3-gen/src/generator/postprocess/uses.rs): RustTypeDeduplication, ModuleImports, HeaderRefCollection
- [postprocess/validation.rs](../crates/oas3-gen/src/generator/postprocess/validation.rs): NestedValidationProcessor
- [codegen/mod.rs](../crates/oas3-gen/src/generator/codegen/mod.rs): SchemaCodeGenerator entry point
- [codegen/types.rs](../crates/oas3-gen/src/generator/codegen/types.rs): TypeFragment, TypesFragment for type file generation
- [codegen/client.rs](../crates/oas3-gen/src/generator/codegen/client.rs): HTTP client generation (ClientFragment)
- [codegen/server.rs](../crates/oas3-gen/src/generator/codegen/server.rs): HTTP server trait generation (ServerGenerator)
- [ast/mod.rs](../crates/oas3-gen/src/generator/ast/mod.rs): AST type definitions
- [ast/server.rs](../crates/oas3-gen/src/generator/ast/server.rs): Server AST definitions (ServerRequestTraitDef, ServerTraitMethod)
- [operation_registry.rs](../crates/oas3-gen/src/generator/operation_registry.rs): HTTP operations and webhooks

## Code Generation Fragments

See [code-fragments.md](./code-fragments.md) for a complete reference of all code generation fragments in the `codegen/` module, including:
- Fragment hierarchy and composition patterns
- Struct, enum, and response enum fragments
- Client and server generation fragments
- Attribute and method generation helpers

## Key Dependencies

All dependencies are managed at the workspace level in the root `Cargo.toml` and inherited by crates.

### Code Generation

- **oas3** (0.20): OpenAPI 3.1 spec parser with JSON and YAML support
- **quote** (1.0): Token stream generation
- **proc-macro2** (1.0): Token manipulation
- **syn** (2.0): Rust syntax parser with full parsing support
- **prettyplease** (0.2): Code formatter
- **bon** (3.8): Builder pattern derive macros

### CLI & Terminal

- **clap** (4.5): Argument parsing with derives and color support
- **tokio** (1.49): Async runtime for multi-threaded I/O
- **anyhow** (1.0): Error handling with context
- **thiserror** (2.0): Custom error type derivation
- **crossterm** (0.29): Terminal interaction
- **comfy-table** (7.2): Table formatting for CLI output
- **num-format** (0.4): Number formatting for statistics
- **cfg-if** (1.0): Conditional compilation

### Serialization & Data

- **serde** (1.0): Serialization framework
- **serde_json** (1.0): JSON with order preservation
- **serde_with** (3.15): Enhanced serde utilities and chrono support
- **serde_path_to_error** (0.1): Detailed deserialization error paths
- **json-canon** (0.1): Canonical JSON representation
- **quick-xml** (>=0.38): XML parsing for content negotiation
- **mediatype** (0.21): MIME type handling with serde support

### Validation & Patterns

- **validator** (0.20): Validation attributes and derive macros
- **regex** (1.12): Pattern matching and validation

### Type System Support

- **better_default** (1.0): Enhanced `#[default(value)]` attribute
- **chrono** (>=0.4.42): Date/time types with serde support
- **uuid** (1.19): UUID type support with serde
- **indexmap** (2.13): Insertion-ordered maps with serde
- **http** (1.4): HTTP primitives and status codes

### String & Identifier Processing

- **inflections** (1.1): Case conversions (snake_case, camelCase, etc.)
- **cruet** (0.15): Advanced string inflection and pluralization
- **any_ascii** (0.3): ASCII transliteration for identifiers
- **percent-encoding** (2.3): URL encoding for path templates
- **string_cache** (0.9): Interned strings for performance
- **strum** (0.27): String enum derivations

### Iterator & Collection Processing

- **itertools** (0.14): Deterministic iteration (sorted, unique, dedup, merge)
- **petgraph** (0.8): Graph data structures for dependency analysis

### Performance & Caching

- **blake3** (1.8): Fast cryptographic hashing with NEON support
- **fmmap** (0.4): Memory-mapped file I/O with tokio support

### HTTP Client & Server

- **reqwest** (0.13): HTTP client for remote specs
- **axum** (0.8): Web framework for generated server code
- **axum-core** (0.5): Core axum types and traits
- **futures** (0.3): Async primitives and combinators
- **futures-core** (0.3): Core future traits
- **eventsource-stream** (0.2): Server-sent events streaming

### Runtime Support

- **oas3-gen-support** (0.25.0): Workspace runtime library with macros and utilities

### Development & Testing

- **tempfile** (3.24): Temporary test files and directories

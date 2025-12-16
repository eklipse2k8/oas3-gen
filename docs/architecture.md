# Architecture

Cargo workspace with two crates following a three-stage pipeline: **Parse OpenAPI -> Convert to AST -> Generate Rust Code**

## Directory Structure

```text
crates/
├── oas3-gen/                      # CLI tool (binary)
│   ├── fixtures/                  # Test fixtures
│   │   ├── basic_api.json         # Basic API test fixture
│   │   ├── content_types.json     # Content type handling tests
│   │   ├── enum_deduplication.json # Enum deduplication tests
│   │   ├── implicit_union.json    # Implicit union tests
│   │   ├── integer_path_param.json # Integer path parameter tests
│   │   ├── operation_filtering.json # Operation filtering tests
│   │   ├── petstore.json          # Petstore API specification
│   │   ├── relaxed_enum_deduplication.json # Relaxed enum deduplication tests
│   │   └── petstore/              # Petstore generated output fixtures
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
│       │   └── text.rs            # Text processing utilities
│       ├── tests/                 # Integration test utilities
│       │   ├── mod.rs
│       │   ├── common.rs          # Common test helpers
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
│           │   └── schema_graph.rs
│           ├── analyzer/          # Schema analysis and validation
│           │   ├── mod.rs
│           │   ├── errors.rs      # Error type definitions
│           │   ├── stats.rs       # Schema statistics
│           │   ├── transforms.rs  # Schema transformations
│           │   ├── type_graph.rs  # Type dependency graph
│           │   ├── type_usage.rs  # Type usage tracking
│           │   └── tests/         # Analyzer tests
│           │       ├── mod.rs
│           │       ├── error_tests.rs
│           │       ├── transform_tests.rs
│           │       └── type_usage_tests.rs
│           ├── naming/            # Identifier naming and conversion
│           │   ├── mod.rs
│           │   ├── constants.rs   # Naming constants
│           │   ├── identifiers.rs # Rust identifier generation
│           │   ├── inference.rs   # Type name inference
│           │   ├── operations.rs  # Operation naming
│           │   ├── responses.rs   # Response naming
│           │   └── tests/         # Naming tests
│           │       ├── mod.rs
│           │       ├── identifiers.rs
│           │       ├── inference.rs
│           │       └── responses.rs
│           ├── ast/               # AST type definitions
│           │   ├── mod.rs
│           │   ├── types.rs       # Core AST types (RustType, StructDef, EnumDef, etc.)
│           │   ├── tokens.rs      # Token stream utilities
│           │   ├── derives.rs     # Derive macro selection
│           │   ├── lints.rs       # Clippy lint attributes
│           │   ├── outer_attrs.rs # Type-safe outer attributes (skip_serializing_none, non_exhaustive)
│           │   ├── serde_attrs.rs # Serde attribute builders with ToTokens
│           │   ├── status_codes.rs # HTTP status code handling (full RFC coverage)
│           │   ├── validation_attrs.rs # Validation attribute builders with ToTokens
│           │   └── tests/         # AST tests
│           │       ├── mod.rs
│           │       ├── status_codes.rs
│           │       ├── types.rs
│           │       └── validation_attrs.rs
│           ├── converter/         # OpenAPI -> AST conversion
│           │   ├── mod.rs         # CodegenConfig and policy enums
│           │   ├── cache.rs       # Schema conversion caching with StructSummary
│           │   ├── common.rs      # Common conversion utilities
│           │   ├── discriminator.rs # Discriminator handling
│           │   ├── enums.rs       # oneOf/anyOf/allOf conversion (includes string enum optimization)
│           │   ├── hashing.rs     # Schema fingerprinting
│           │   ├── metadata.rs    # Schema metadata extraction
│           │   ├── operations.rs  # Request/response type generation
│           │   ├── path_renderer.rs # URL path template rendering
│           │   ├── responses.rs   # Response type generation
│           │   ├── structs.rs     # Object schema conversion (includes field optionality)
│           │   ├── type_resolver.rs # Type mapping with TypeResolverBuilder
│           │   ├── type_usage_recorder.rs # Type usage recording
│           │   └── tests/         # Converter tests
│           │       ├── mod.rs
│           │       ├── cache.rs
│           │       ├── enums.rs
│           │       ├── implicit_dependencies.rs
│           │       ├── inline_objects.rs
│           │       ├── metadata_tests.rs
│           │       ├── operations.rs
│           │       ├── path_renderer.rs
│           │       ├── structs.rs
│           │       ├── type_aliases.rs
│           │       └── type_resolution.rs
│           └── codegen/           # AST -> Rust source generation
│               ├── mod.rs
│               ├── attributes.rs  # Attribute generation
│               ├── client.rs      # HTTP client generation
│               ├── coercion.rs    # Type coercion logic
│               ├── constants.rs   # Constant generation
│               ├── enums.rs       # Enum code generation
│               ├── error_impls.rs # Error trait implementations
│               ├── metadata.rs    # Metadata comment generation
│               ├── structs.rs     # Struct code generation
│               ├── type_aliases.rs # Type alias generation
│               └── tests/         # Codegen tests
│                   ├── mod.rs
│                   ├── client.rs
│                   ├── coercion_tests.rs
│                   ├── constants_tests.rs
│                   ├── enum_tests.rs
│                   ├── error_impl_tests.rs
│                   ├── struct_tests.rs
│                   └── type_alias_tests.rs
└── oas3-gen-support/              # Runtime library (rlib + cdylib)
    └── src/
        └── lib.rs                 # Runtime utilities for generated code
```

## Generation Pipeline

1. **Parse**: Load OpenAPI spec via `oas3` crate
2. **Analyze**: Build schema dependency graph, detect cycles
3. **Convert**: Transform schemas to AST (`converter/`)
4. **Generate**: Produce formatted Rust code (`codegen/`)

## Key Files

- [orchestrator.rs](../crates/oas3-gen/src/generator/orchestrator.rs): Pipeline coordinator
- [schema_registry.rs](../crates/oas3-gen/src/generator/schema_registry.rs): Dependency and cycle management
- [operation_registry.rs](../crates/oas3-gen/src/generator/operation_registry.rs): HTTP operations and webhooks management
- [type_resolver.rs](../crates/oas3-gen/src/generator/converter/type_resolver.rs): OpenAPI to Rust type mapping with TypeResolverBuilder
- [identifiers.rs](../crates/oas3-gen/src/generator/naming/identifiers.rs): Identifier sanitization and keyword handling
- [cache.rs](../crates/oas3-gen/src/generator/converter/cache.rs): Schema conversion caching with StructSummary
- [type_usage.rs](../crates/oas3-gen/src/generator/analyzer/type_usage.rs): Type usage tracking and analysis
- [converter/mod.rs](../crates/oas3-gen/src/generator/converter/mod.rs): CodegenConfig and typed policy enums

## Key Dependencies

All dependencies are managed at the workspace level in the root `Cargo.toml` and inherited by crates.

### Code Generation

- **oas3** (0.20): OpenAPI 3.1 spec parser
- **quote** (1.0): Token stream generation
- **proc-macro2** (1.0): Token manipulation
- **syn** (2.0): Rust syntax parser with full parsing support
- **prettyplease** (0.2): Code formatter

### CLI & Terminal

- **clap** (4.5): Argument parsing with derives and color support
- **tokio** (1.48): Async runtime for multi-threaded I/O
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
- **serde_plain** (1.0): Plain text serialization
- **serde_path_to_error** (0.1): Detailed deserialization error paths
- **json-canon** (0.1): Canonical JSON representation

### Validation & Patterns

- **validator** (0.20): Validation attributes and derive macros
- **regex** (1.12): Pattern matching and validation

### Type System Support

- **better_default** (1.0): Enhanced `#[default(value)]` attribute
- **chrono** (>=0.4.42): Date/time types with serde support
- **uuid** (1.18): UUID type support with serde
- **indexmap** (2.12): Insertion-ordered maps with serde
- **http** (1.3): HTTP primitives and status codes

### String & Identifier Processing

- **inflections** (1.1): Case conversions (snake_case, camelCase, etc.)
- **cruet** (0.15): Advanced string inflection and pluralization
- **any_ascii** (0.3): ASCII transliteration for identifiers
- **percent-encoding** (2.3): URL encoding for path templates
- **string_cache** (0.9): Interned strings for performance
- **strum** (0.27): String enum derivations

### Performance & Caching

- **blake3** (1.8): Fast cryptographic hashing with NEON support
- **fmmap** (0.4): Memory-mapped file I/O with tokio support

### HTTP Client

- **reqwest** (0.12): HTTP client for remote specs

### Runtime Support

- **oas3-gen-support** (0.21.0): Workspace runtime library with macros and utilities

### Development & Testing

- **tempfile** (3.23): Temporary test files and directories

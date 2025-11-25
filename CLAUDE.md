# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

This is a fully-functional OpenAPI-to-Rust code generator that parses OpenAPI 3.1 specifications and generates comprehensive Rust type definitions with validation. The tool reads OpenAPI JSON files and generates idiomatic Rust code including structs, enums, type aliases, validation attributes, and Default implementations.

The project is organized as a Cargo workspace with two crates:

- **oas3-gen**: The main CLI tool for code generation
- **oas3-gen-support**: Runtime support library providing macros and utilities for generated code

## Available Subagents

This project has specialized subagents that can be invoked using the Task tool for specific types of work:

### performance-engineer

**Purpose**: Optimize CLI performance, reduce memory usage, and improve build times

**When to use**:

- Profiling performance bottlenecks with flamegraphs
- Optimizing schema conversion and code generation speed
- Reducing binary size and startup time
- Implementing benchmarks with criterion

### code-reviewer

**Purpose**: Review code for Rust idioms, safety, and project standards

**When to use**:

- Reviewing code changes for correctness and performance
- Checking adherence to token conservation requirements
- Evaluating AST manipulation and type safety
- Ensuring generated code quality

### test-automator

**Purpose**: Create comprehensive test suites and CI/CD pipelines

**When to use**:

- Writing unit tests for converters and generators
- Setting up property-based testing with proptest
- Creating GitHub Actions workflows
- Testing generated code compilation

### cli-developer

**Purpose**: Enhance CLI interface and user experience

**When to use**:

- Adding new CLI arguments or features
- Improving error messages and progress reporting
- Setting up binary distribution and releases
- Implementing shell completions

### documentation-expert

**Purpose**: Create user-friendly documentation for all audiences

**When to use**:

- Writing installation guides for non-Rust users
- Creating usage examples and troubleshooting guides
- Documenting generated code patterns
- Maintaining README and CHANGELOG

### Subagent Collaboration

These subagents are designed to work together:

- **performance-engineer** → **code-reviewer** for optimization validation
- **code-reviewer** → **test-automator** for test coverage requirements
- **cli-developer** → **documentation-expert** for usage documentation
- **test-automator** → **performance-engineer** for benchmark creation

## Coding Standards

### CRITICAL: Token Conservation Requirements

- **NO inline comments**: Never add explanatory comments, session notes, or relative-to-session notes within code. Code must be self-documenting through clear naming and structure.
- **NO emojis**: Never use emojis in any context - code, comments, documentation, or messages. Emojis consume valuable tokens.
- **Doc comments only**: Only use proper Rust doc comments (`///` or `//!`) for public API documentation that will be part of generated rustdoc.

This project prioritizes token efficiency. Every inline comment and emoji wastes tokens that could be used for actual code or logic.

### Naming Conventions

Follow [Rust API Guidelines](https://rust-lang.github.io/api-guidelines/naming.html). Prioritize clarity over brevity (prefer `request` over `req`).

**Casing:**

| Identifier Type                       | Convention           | Examples                                 |
| ------------------------------------- | -------------------- | ---------------------------------------- |
| Crates                                | `kebab-case`         | `oas3-gen`, `oas3-gen-support`           |
| Modules                               | `snake_case`         | `schema_graph`, `operation_registry`     |
| Structs, Enums, Traits, Type Aliases  | `UpperCamelCase`     | `SchemaConverter`, `GenerateMode`        |
| Enum Variants                         | `UpperCamelCase`     | `RequestOnly`, `Bidirectional`           |
| Functions, Methods                    | `snake_case`         | `generate_code`, `convert_schema`        |
| Variables, Fields, Parameters         | `snake_case`         | `spec`, `visibility`, `only_operations`  |
| Constants, Statics                    | `UPPER_SNAKE_CASE`   | `REQUEST_SUFFIX`, `CLIPPY_ALLOWS`        |
| Generic Type Parameters               | `UpperCamelCase`     | `T`, `E`, `IntoSchema`                   |
| Macros                                | `snake_case!`        | `discriminated_enum!`                    |

**Type Suffixes:**

- `...Converter`: Type conversion | `...Analyzer`: Data inspection | `...Registry`: Collection storage
- `...Graph`: Graph structures | `...Config`: Configuration | `...Builder`: Builder pattern
- `...Def`: AST nodes (StructDef, EnumDef, TypeAliasDef)

**Function Patterns:**

- Constructors: `new()`, `with_<property>()`, `from_<source>()`
- Getters: `<property>()` (no `get_` prefix)
- Conversions: `to_<type>()` (non-consuming), `into_<type>()` (consuming)
- Predicates: `is_<condition>()`, `has_<property>()`

**Generated Code:**

- Distinguish OpenAPI (source) from Rust AST (target) concepts
- Operation types: `...Request`, `...RequestBody`, `...Response`
- Fields: `snake_case` with keyword escaping (`r#type`)

### Collection Types for Deterministic Generation

CRITICAL: Choose collection types carefully to ensure deterministic code generation.

**IndexMap/IndexSet** (insertion order):
- `OperationRegistry`: Preserves operation order from OpenAPI spec for logical client method ordering
- Use when spec author's ordering is meaningful and should be reflected in generated code
- Operations should appear in client in same order as spec

**BTreeMap/BTreeSet** (sorted order):
- Schema storage, type generation, dependency graphs
- Produces alphabetically sorted output independent of spec ordering
- More stable across spec changes (reordering schemas doesn't change generated output)
- Makes generated code easier to navigate and review
- Example: `deduplicate_and_order_types()` intentionally uses BTreeMap for sorting

**HashMap/HashSet** (non-deterministic):
- NEVER use for anything that affects code generation order
- Only acceptable for internal logic where order doesn't matter (e.g., temporary deduplication)

**Rule of thumb:**
- Operations/endpoints → IndexMap (spec order matters)
- Types/schemas/dependencies → BTreeMap (alphabetical is better)
- Internal bookkeeping → HashMap only if order truly doesn't matter

### Preferred Code Patterns

**Reference Counting and Cloning:**

- Use `Arc<T>` for shared ownership of expensive-to-clone types (e.g., `Arc<ObjectSchema>`)
- `Arc::clone()` is O(1) and only increments a reference count
- Prefer `Arc` over deep cloning when passing schemas or large data structures through the conversion pipeline
- This reduces memory usage and improves performance

**Vec Initialization:**

- Prefer `vec![]` over `Vec::new()` for consistency
- Both are idiomatic, but `vec![]` is more concise

**Builder Pattern:**

- Use builder pattern (via `derive_builder`) for structs with multiple optional fields or complex construction
- Direct struct initialization is acceptable for simple parameter objects with few required fields
- Builders improve readability when constructing objects with many fields
- Example: `FieldDefBuilder::default().name("foo").rust_type(ty).build()?`

**Avoid Tuples:**

- NEVER use tuples as function return types when returning multiple values
- Use named structs instead for clarity and maintainability
- Good: `fn convert() -> Generated<RustType>` with `struct Generated<T> { item: T, inline_types: Vec<RustType> }`
- Bad: `fn convert() -> (RustType, Vec<RustType>)`
- Tuples lack semantic meaning and make code harder to understand
- Exception: Standard library patterns like `Iterator::enumerate()` where tuple meaning is well-established

**String Enums:**

- Use `strum` (with `#[derive(EnumString, Display)]`) for simple known string enums
- Provides automatic string parsing and serialization without boilerplate
- Good for enums with fixed string representations like HTTP methods, status categories, etc.
- Example: `#[derive(EnumString, Display)] enum HttpMethod { Get, Post, Put, Delete }`

**String Interning:**

- Use `string_cache::DefaultAtom` when strings act as symbols (identifiers, type names, field names)
- `DefaultAtom` provides O(1) equality comparison and reduced memory usage through interning
- Wrap strings in `DefaultAtom` using `.into()`: `let name: DefaultAtom = "MyStruct".into()`
- Particularly effective for repeated identifiers in code generation where the same names appear frequently
- Example: Type names, field names, operation IDs, schema references
- Don't use for arbitrary user content or large strings that won't be reused

**Error Context with anyhow:**

- Use `with_context()` instead of `map_err()` when adding context to errors
- `with_context()` preserves the error chain and is more idiomatic with anyhow
- Bad: `.map_err(|e| anyhow::anyhow!("Failed for '{}': {e}", name))?`
- Good: `.with_context(|| format!("Failed for '{}'", name))?`
- The underlying error is automatically chained; don't manually interpolate it into the message
- Import `use anyhow::Context;` to access the `with_context()` method on `Result` types

### Design Principles and Code Quality

**SOLID Principles:**

- Single Responsibility: One concern per module/struct/function
- Open/Closed: Extend via composition, not modification
- Liskov Substitution: Subtypes fully replace base types
- Interface Segregation: Focused traits over monolithic ones
- Dependency Inversion: Depend on abstractions

**Avoid Duplication:**

- Never duplicate logic; extract to reusable functions/traits/generics
- Search for existing implementations before writing new code
- Refactor duplicated patterns immediately upon discovery

**Code Placement Strategy:**

1. Review pipeline architecture: Parse/Analyze → Convert (AST) → Generate (Rust source)
2. Identify stage:
   - analyzer/ for schema analysis, validation, and type usage tracking
   - naming/ for identifier generation and type name inference
   - converter/ for OpenAPI to AST transformation
   - codegen/ for AST to Rust source code generation
3. Locate module: enums, structs, operations, type_resolver, attributes, cache, etc.
4. Check utilities for cross-cutting concerns: utils/text.rs, naming/identifiers.rs

**Test Coverage:**

- All code changes require unit tests in `#[cfg(test)]` modules
- Cover: happy paths, edge cases (empty/boundary/special chars), error conditions
- Run `cargo test` before committing

## Build and Development Commands

### Build

```bash
cargo build
```

### Run

The application uses a CLI interface powered by `clap` with subcommands:

```bash
# Generate Rust types from OpenAPI spec
cargo run -- generate -i spec.json -o generated.rs

# With verbose output (shows cycles, operations count, etc.)
cargo run -- generate -i spec.json -o output.rs --verbose

# Quiet mode (errors only)
cargo run -- generate -i spec.json -o output.rs --quiet

# Generate all schemas (default: only operation-referenced schemas)
cargo run -- generate -i spec.json -o output.rs --all-schemas

# Output to nested directory (creates parent directories automatically)
cargo run -- generate -i spec.json -o output/types/generated.rs

# List all operations in the spec
cargo run -- list operations -i spec.json

# View help
cargo run -- --help
cargo run -- generate --help
cargo run -- list --help
```

**Subcommands:**

- `generate`: Generate Rust code from OpenAPI specification
  - `--input` / `-i`: (Required) Path to OpenAPI JSON specification file
  - `--output` / `-o`: (Required) Path where generated Rust code will be written
  - `--visibility`: Visibility level for generated types (public, crate, or file; default: public)
  - `--verbose` / `-v`: Enable verbose output with detailed progress information
  - `--quiet` / `-q`: Suppress non-essential output (errors only)
  - `--all-schemas`: Generate all schemas defined in spec (default: only schemas referenced by operations)

- `list`: List information from OpenAPI specification
  - `operations`: List all operations with their IDs, methods, and paths

**Global Options:**

- `--color`: Control color output (always, auto, never; default: auto)
- `--theme`: Terminal theme (dark, light, auto; default: auto)

### Testing

```bash
# Run all tests (tests both oas3-gen and oas3-gen-support crates)
cargo test
```

Note: Use `cargo test` without `--lib` to test the entire workspace. Using `cargo test --lib` only tests the oas3-gen-support library crate.

### Linting (non-destructive)

```bash
cargo clippy --all -- -W clippy::pedantic

# Format code
cargo +nightly fmt --all --check
```

### Linting (updates files)

```bash
# Automatically fix most warnings, if possible
cargo clippy --fix --allow-dirty --all -- -W clippy::pedantic

# Cleanup any unformatted code
cargo +nightly fmt --all 
```

Note: This project uses custom rustfmt settings (see rustfmt.toml):

- 2 spaces for indentation
- 120 character max width
- Merged imports with crate granularity
- Normalized doc attributes

### Dependency Management

```bash
# Check for security advisories and licensing issues
cargo deny check

# Update dependencies
cargo update
```

### Performance Profiling

```bash
# Get available options for flamegraph
cargo flamegraph -h

# Default flamegraph execution of oas3-gen
cargo flamegraph -o flamegraph.svg -- generate -i spec.json -o output.rs
```

### Debugging

When debugging issues in this project, follow these principles:

- Use logging (tracing, log) or macros like `dbg!()` to inspect state
- Make code changes only if you have high confidence they can solve the problem
- When debugging, try to determine the root cause rather than addressing symptoms
- Debug for as long as needed to identify the root cause and identify a fix
- Use print statements, logs, or temporary code to inspect program state, including descriptive statements or error messages to understand what's happening
- To test hypotheses, you can also add test statements or functions
- Revisit your assumptions if unexpected behavior occurs
- Use `RUST_BACKTRACE=1` to get stack traces, and `cargo-expand` to debug macros and derive logic
- Read terminal output

### Workspace-Specific Commands

The project uses a Cargo workspace, so you can work with individual crates:

```bash
# Build only the CLI tool
cargo build -p oas3-gen

# Build only the support library
cargo build -p oas3-gen-support

# Run tests for a specific crate
cargo test -p oas3-gen
cargo test -p oas3-gen-support

# Check a specific crate
cargo check -p oas3-gen-support
```

## Architecture

Cargo workspace with two crates following a three-stage pipeline: **Parse OpenAPI → Convert to AST → Generate Rust Code**

```text
crates/
├── oas3-gen/                      # CLI tool (binary)
│   └── src/
│       ├── main.rs                # Entry point
│       ├── ui/                    # CLI interface
│       │   ├── cli.rs             # Argument definitions
│       │   ├── colors.rs          # Terminal theming
│       │   └── commands/          # Command handlers (generate, list)
│       ├── utils/                 # Cross-cutting utilities
│       │   └── text.rs            # Text processing utilities
│       └── generator/             # Core generation pipeline
│           ├── orchestrator.rs    # Main pipeline coordinator
│           ├── operation_registry.rs # Operation collection management
│           ├── schema_graph.rs    # Dependency tracking and cycle detection
│           ├── analyzer/          # Schema analysis and validation
│           │   ├── errors.rs      # Error type definitions
│           │   ├── stats.rs       # Schema statistics
│           │   ├── transforms.rs  # Schema transformations
│           │   ├── type_graph.rs  # Type dependency graph
│           │   └── type_usage.rs  # Type usage tracking
│           ├── naming/            # Identifier naming and conversion
│           │   ├── identifiers.rs # Rust identifier generation
│           │   └── inference.rs   # Type name inference
│           ├── ast/               # AST type definitions
│           │   ├── types.rs       # Core AST types (RustType, StructDef, EnumDef, etc.)
│           │   ├── derives.rs     # Derive macro selection
│           │   ├── lints.rs       # Clippy lint attributes
│           │   ├── serde_attrs.rs # Serde attribute builders
│           │   └── validation_attrs.rs # Validation attribute builders
│           ├── converter/         # OpenAPI → AST conversion
│           │   ├── cache.rs       # Schema conversion caching
│           │   ├── constants.rs   # Conversion constants
│           │   ├── enums.rs       # oneOf/anyOf/allOf conversion
│           │   ├── field_optionality.rs # Field requirement logic
│           │   ├── hashing.rs     # Schema fingerprinting
│           │   ├── metadata.rs    # Schema metadata extraction
│           │   ├── operations.rs  # Request/response type generation
│           │   ├── path_renderer.rs # URL path template rendering
│           │   ├── responses.rs   # Response type generation
│           │   ├── status_codes.rs # HTTP status code handling
│           │   ├── string_enum_optimizer.rs # String enum optimization
│           │   ├── structs.rs     # Object schema conversion
│           │   ├── type_resolver.rs # Type mapping and nullable patterns
│           │   └── type_usage_recorder.rs # Type usage recording
│           └── codegen/           # AST → Rust source generation
│               ├── attributes.rs  # Attribute generation
│               ├── coercion.rs    # Type coercion logic
│               ├── constants.rs   # Constant generation
│               ├── enums.rs       # Enum code generation
│               ├── error_impls.rs # Error trait implementations
│               ├── metadata.rs    # Metadata comment generation
│               ├── structs.rs     # Struct code generation
│               ├── type_aliases.rs # Type alias generation
│               └── client/        # HTTP client generation
│                   └── methods.rs # Client method generation
└── oas3-gen-support/              # Runtime library (rlib + cdylib)
    └── src/
        └── lib.rs                 # discriminated_enum! macro and utilities
```

**Generation Pipeline:**

1. **Parse**: Load OpenAPI spec via `oas3` crate
2. **Analyze**: Build schema dependency graph, detect cycles
3. **Convert**: Transform schemas to AST (`converter/`)
4. **Generate**: Produce formatted Rust code (`codegen/`)

**Key Files:**

- [orchestrator.rs](crates/oas3-gen/src/generator/orchestrator.rs): Pipeline coordinator
- [schema_graph.rs](crates/oas3-gen/src/generator/schema_graph.rs): Dependency and cycle management
- [type_resolver.rs](crates/oas3-gen/src/generator/converter/type_resolver.rs): OpenAPI to Rust type mapping
- [identifiers.rs](crates/oas3-gen/src/generator/naming/identifiers.rs): Identifier sanitization and keyword handling
- [cache.rs](crates/oas3-gen/src/generator/converter/cache.rs): Schema conversion caching for performance
- [type_usage.rs](crates/oas3-gen/src/generator/analyzer/type_usage.rs): Type usage tracking and analysis

### Key Dependencies

All dependencies are managed at the workspace level in the root `Cargo.toml` and inherited by crates.

**Code Generation:**

- **oas3** (0.20): OpenAPI 3.1 spec parser
- **quote** (1.0): Token stream generation
- **proc-macro2** (1.0): Token manipulation
- **syn** (2.0): Rust syntax parser with full parsing support
- **prettyplease** (0.2): Code formatter

**CLI & Terminal:**

- **clap** (4.5): Argument parsing with derives and color support
- **tokio** (1.48): Async runtime for multi-threaded I/O
- **anyhow** (1.0): Error handling with context
- **thiserror** (2.0): Custom error type derivation
- **crossterm** (0.29): Terminal interaction
- **comfy-table** (7.2): Table formatting for CLI output
- **num-format** (0.4): Number formatting for statistics
- **cfg-if** (1.0): Conditional compilation

**Serialization & Data:**

- **serde** (1.0): Serialization framework
- **serde_json** (1.0): JSON with order preservation
- **serde_with** (3.15): Enhanced serde utilities and chrono support
- **serde_plain** (1.0): Plain text serialization
- **serde_path_to_error** (0.1): Detailed deserialization error paths
- **json-canon** (0.1): Canonical JSON representation

**Validation & Patterns:**

- **validator** (0.20): Validation attributes and derive macros
- **regex** (1.12): Pattern matching and validation

**Type System Support:**

- **better_default** (1.0): Enhanced `#[default(value)]` attribute
- **chrono** (>=0.4.42): Date/time types with serde support
- **uuid** (1.18): UUID type support with serde
- **indexmap** (2.12): Insertion-ordered maps with serde
- **http** (1.3): HTTP primitives and status codes

**String & Identifier Processing:**

- **inflections** (1.1): Case conversions (snake_case, camelCase, etc.)
- **cruet** (0.15): Advanced string inflection and pluralization
- **any_ascii** (0.3): ASCII transliteration for identifiers
- **percent-encoding** (2.3): URL encoding for path templates
- **string_cache** (0.9): Interned strings for performance
- **strum** (0.27): String enum derivations

**Performance & Caching:**

- **blake3** (1.8): Fast cryptographic hashing with NEON support
- **fmmap** (0.4): Memory-mapped file I/O with tokio support

**HTTP Client:**

- **reqwest** (0.12): HTTP client for remote specs

**Runtime Support:**

- **oas3-gen-support** (0.20.0): Workspace runtime library with macros and utilities

**Development & Testing:**

- **tempfile** (3.23): Temporary test files and directories

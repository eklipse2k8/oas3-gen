# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

This is a fully-functional OpenAPI-to-Rust code generator that parses OpenAPI 3.x specifications and generates comprehensive Rust type definitions with validation. The tool reads OpenAPI JSON files and generates idiomatic Rust code including structs, enums, type aliases, validation attributes, and Default implementations.

The project is organized as a Cargo workspace with two crates:

- **oas3-gen**: The main CLI tool for code generation
- **oas3-gen-support**: Runtime support library providing macros and utilities for generated code

## Build and Development Commands

### Build

```bash
cargo build
```

### Run

The application uses a CLI interface powered by `clap`:

```bash
# Basic usage
cargo run -- --input spec.json --output generated.rs

# Short flags
cargo run -- -i spec.json -o output.rs

# With verbose output (shows cycles, operations count, etc.)
cargo run -- -i spec.json -o output.rs --verbose

# Quiet mode (errors only)
cargo run -- -i spec.json -o output.rs --quiet

# Output to nested directory (creates parent directories automatically)
cargo run -- -i spec.json -o output/types/generated.rs

# View help
cargo run -- --help
```

**CLI Arguments:**

- `--input` / `-i`: (Required) Path to OpenAPI JSON specification file
- `--output` / `-o`: (Required) Path where generated Rust code will be written
- `--verbose` / `-v`: Enable verbose output with detailed progress information
- `--quiet` / `-q`: Suppress non-essential output (errors only)

### Testing

```bash
cargo test
```

### Linting

```bash
cargo clippy
cargo fmt --check
```

### Format Code

```bash
cargo fmt
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

# Run the CLI (from workspace root)
cargo run -p oas3-gen -- -i spec.json -o output.rs

# Check a specific crate
cargo check -p oas3-gen-support
```

## Architecture

The codebase is organized as a Cargo workspace with two crates:

```text
crates/
├── oas3-gen/                      - Main CLI tool
│   ├── Cargo.toml                 - Binary crate dependencies
│   ├── src/
│   │   ├── main.rs                - Entry point and orchestration
│   │   ├── reserved.rs            - Rust keyword handling and naming utilities
│   │   └── generator/             - Core generation logic
│   │       ├── mod.rs             - Module definition and re-exports
│   │       ├── utils.rs           - Helper functions
│   │       ├── ast.rs             - AST type definitions
│   │       ├── schema_graph.rs    - Dependency tracking
│   │       ├── schema_converter.rs - Schema → AST conversion
│   │       ├── operation_converter.rs - Operation → request/response types
│   │       └── code_generator.rs  - AST → Rust code generation
│   ├── tests/                     - Integration tests
│   │   └── discriminator_deserialization.rs
│   └── examples/                  - Example generated code
│       └── generated_types.rs
└── oas3-gen-support/              - Runtime support library
    ├── Cargo.toml                 - Library crate (rlib + cdylib)
    └── src/
        └── lib.rs                 - Macros and runtime utilities
```

### Module Structure

#### oas3-gen Crate (CLI Tool)

**generator/mod.rs** (`crates/oas3-gen/src/generator/mod.rs`)

- Module orchestration and public API
- Re-exports: `SchemaGraph`, `SchemaConverter`, `OperationConverter`, `CodeGenerator`
- Declares all submodules with clear documentation

**generator/utils.rs** (`crates/oas3-gen/src/generator/utils.rs`)

- `doc_comment_lines()`: Converts strings to Rust doc comment lines
- `doc_comment_block()`: Creates full doc comment blocks

**generator/ast.rs** (`crates/oas3-gen/src/generator/ast.rs`)
Intermediate representation types:

- `RustType`: Enum containing Struct, Enum, or TypeAlias variants
- `StructDef`: Struct definition with fields, derives, and serde attributes
- `FieldDef`: Field definition with type, docs, validation, and default values
- `EnumDef`: Enum definition with variants and optional discriminator
- `VariantDef`: Enum variant (Unit, Tuple, or Struct content)
- `TypeRef`: Type reference with support for Box<T>, Option<T>, Vec<T> wrappers
- `OperationInfo`: Metadata about API operations

**generator/schema_graph.rs** (`crates/oas3-gen/src/generator/schema_graph.rs`)
Schema dependency management and cycle detection:

- `SchemaGraph` struct manages all schemas from OpenAPI spec
- Builds dependency graph tracking which schemas reference others
- DFS-based cycle detection algorithm
- Marks cyclic schemas for Box<T> wrapper injection
- Key methods:
  - `new()`: Extracts schemas from OpenAPI spec components
  - `build_dependencies()`: Analyzes schema references to build dependency graph
  - `detect_cycles()`: Identifies circular schema references using DFS
  - `is_cyclic()`: Checks if a schema is part of a cycle
  - `extract_ref_name()`: Parses $ref strings to extract schema names

**generator/schema_converter.rs** (`crates/oas3-gen/src/generator/schema_converter.rs`)
Converts OpenAPI schemas to Rust AST (largest module):

- Handles all schema types: objects, enums, oneOf, anyOf, allOf
- Detects and handles nullable patterns (anyOf with null → Option<T>)
- Generates inline enum types for nested unions
- Extracts validation rules from OpenAPI constraints
- Manages cyclic references with Box<T> wrappers
- Handles discriminated unions with serde tag attribute
- Key methods (16 total):
  - `convert_schema()`: Main entry point for schema conversion
  - `convert_one_of_enum()`: Handles oneOf with discriminator support
  - `convert_any_of_enum()`: Handles anyOf with untagged enum generation
  - `convert_string_enum_with_catchall()`: Forward-compatible enums
  - `convert_simple_enum()`: Simple string enums
  - `convert_struct()`: Converts object schemas to structs
  - `schema_to_type_ref()`: Maps schemas to Rust type references (public)
  - `extract_validation_attrs()`: Extracts validation rules (public)
  - `extract_validation_pattern()`: Extracts regex patterns (public)
  - `extract_default_value()`: Extracts default values (public)

**generator/operation_converter.rs** (`crates/oas3-gen/src/generator/operation_converter.rs`)
Generates request/response types for API operations:

- Creates request structs combining parameters and request body
- Orders parameters by location (path → query → header → cookie)
- Extracts response type references from operation definitions
- Generates OperationInfo metadata for tracking
- Handles inline request body schemas
- Key methods:
  - `convert_operation()`: Main operation conversion entry point
  - `create_request_struct()`: Builds request type from parameters and body
  - `convert_parameter()`: Converts individual parameter to field definition
  - `extract_response_schema_name()`: Extracts schema name from response

**generator/code_generator.rs** (`crates/oas3-gen/src/generator/code_generator.rs`)
Converts Rust AST to actual source code:

- `TypeUsage` enum: Tracks request/response usage for derive optimization
- `RegexKey` struct: Manages regex validation constant names
- `TypeKind` enum: Internal type categorization (Struct, Enum, Alias)
- Generates regex validation constants with LazyLock pattern
- Deduplicates types using BTreeMap ordering
- Generates impl Default blocks for structs and enums
- Handles serde attributes (rename, tag, untagged, skip_serializing_if)
- Key methods (21 total):
  - `build_type_usage_map()`: Builds type usage map from operations (public)
  - `generate()`: Main code generation entry point (public)
  - `generate_regex_constants()`: Creates static regex validators
  - `generate_default_impls()`: Generates Default trait implementations
  - `json_value_to_rust_expr()`: Converts JSON to Rust expressions
  - `generate_struct()`, `generate_enum()`, `generate_type_alias()`
  - `ordered_types()`: Deduplicates and orders types for output

**Main Orchestration** (`crates/oas3-gen/src/main.rs`)
Coordinates the generation pipeline with CLI argument handling:

1. Parses CLI arguments using clap (input file, output file, verbose/quiet flags)
2. Loads OpenAPI spec from specified JSON file
3. Builds SchemaGraph with dependency analysis
4. Detects and reports circular dependencies (with detailed output in verbose mode)
5. Converts all schemas to Rust AST using SchemaConverter
6. Converts all operations to request/response types using OperationConverter
7. Generates final Rust code using CodeGenerator
8. Formats code with prettyplease
9. Creates parent directories if needed
10. Writes output to user-specified path

The CLI provides structured logging with three levels:

- Normal: Key progress updates (default)
- Verbose (`--verbose`): Detailed cycle information, operation counts, etc.
- Quiet (`--quiet`): Errors only

#### oas3-gen-support Crate (Runtime Library)

**lib.rs** (`crates/oas3-gen-support/src/lib.rs`)
Provides runtime support for generated code:

- **`discriminated_enum!` macro**: Declarative macro for discriminated union deserialization
  - Supports discriminator field-based routing
  - Optional fallback variant for unknown discriminator values
  - Custom serialize/deserialize implementations
  - Used for oneOf/anyOf with discriminator in generated code
- **`better_default::Default` re-export**: Enables `#[default(value)]` attribute on struct fields
  - Allows inline default value specification
  - Generates Default trait implementations automatically

**Integration Tests** (`crates/oas3-gen/tests/discriminator_deserialization.rs`)

- Tests discriminated enum deserialization with real-world scenarios
- Validates fallback variant behavior
- Ensures proper handling of nested discriminated types

### Key Dependencies

All dependencies are managed at the workspace level in the root `Cargo.toml` and inherited by crates.

**Core Generation Dependencies (oas3-gen):**

- **oas3** (0.19): OpenAPI 3.x specification parsing library
- **oas3-gen-support**: Workspace crate providing runtime support utilities
- **quote** (1.0): Quasi-quoting for generating Rust token streams
- **proc-macro2** (1.0): Standalone proc-macro API for token manipulation
- **syn** (2.0, features: full, parsing): Rust syntax parsing for code formatting
- **prettyplease** (0.2): Pretty-printing generated Rust code with proper formatting

**CLI and I/O:**

- **clap** (4.5, features: derive): Command-line argument parsing with derive macros
- **tokio** (1.48, features: rt-multi-thread, fs, macros): Async runtime for file I/O
- **anyhow** (1.0): Flexible error handling

**Serialization:**

- **serde** (1.0, features: derive): Serialization framework
- **serde_json** (1.0, features: preserve_order): JSON serialization with field order preservation

**String Processing:**

- **inflections** (1.1): Case conversion utilities (PascalCase, snake_case, camelCase)
- **any_ascii** (0.3): ASCII transliteration for identifier sanitization

**Validation:**

- **validator** (0.20, features: derive): Runtime validation attributes and traits
- **regex** (1.11): Regular expression validation support

**Runtime Support Dependencies (oas3-gen-support):**

- **better_default** (1.0): Provides `#[default(value)]` attribute for struct fields
- **serde/serde_json**: For custom discriminated enum serialization
- **regex**: Shared validation support
- **validator**: Shared validation framework
- **chrono** (0.4, features: std, clock, serde): Date/time types for OpenAPI date-time format
- **indexmap** (2.12, features: serde): Ordered map for unique array items
- **uuid** (1.11, features: serde): UUID types for OpenAPI uuid format

**Dev Dependencies (oas3-gen):**

- **chrono**: For testing generated date-time types
- **indexmap**: For testing generated unique array types
- **uuid**: For testing generated UUID types
- **tempfile** (3.14): For creating temporary test files

### Type Mapping System

The `schema_to_type_ref()` method in `SchemaConverter` (`crates/oas3-gen/src/generator/schema_converter.rs`) maps OpenAPI types to Rust:

**Primitive Types:**

- String → String
- Number → f64
- Integer → i64
- Boolean → bool
- Null → Option<()>
- Object (without schema) → serde_json::Value

**Complex Types:**

- Array → Vec<T> (where T is derived from items schema)
- Object (with schema) → Named struct type
- oneOf → Tagged or untagged enum
- anyOf → Untagged enum (or nullable pattern detection)
- Enums → String enums with serde rename attributes

**Special Patterns:**

- `anyOf: [T, null]` → Option<T> (nullable pattern)
- Forward-compatible enums → Enum with catch-all variant
- Cyclic references → Box<T> wrapper
- Discriminated unions → Tagged enum with serde(tag = "field")

### Validation Features

The generator extracts OpenAPI validation constraints and converts them to Rust validator attributes:

**String Validation:**

- `minLength`/`maxLength` → `#[validate(length(min = X, max = Y))]`
- `pattern` → Generates regex constant + `#[validate(regex(path = "CONST_NAME"))]`
- `format: email` → `#[validate(email)]`
- `format: uri` → `#[validate(url)]`

**Numeric Validation:**

- `minimum`/`maximum` → `#[validate(range(min = X, max = Y))]`
- `exclusiveMinimum`/`exclusiveMaximum` → `#[validate(range(exclusive_min = X, ...))]`

**Array Validation:**

- `minItems`/`maxItems` → `#[validate(length(min = X, max = Y))]`

**Default Values:**

- Generates `impl Default` for structs/enums with default values
- Converts JSON defaults to Rust expressions

### Naming and Formatting

**Identifier Handling** (`crates/oas3-gen/src/reserved.rs`):

- Converts OpenAPI names to valid Rust identifiers
- Replaces invalid characters (-, ., spaces) with underscores
- Handles Rust keyword conflicts with r# prefix
- ASCII transliteration for international characters using `any_ascii`
- Key functions:
  - `to_rust_type_name()`: Converts names to PascalCase for types
  - `to_rust_field_name()`: Converts names to snake_case for fields
  - `regex_const_name()`: Generates unique constant names for regex validators

**Type Names:**

- Converts schema names to PascalCase
- Handles keyword conflicts (Self, Type, etc.) with r# prefix
- Ensures uniqueness in enum variant names

**Field Names:**

- Converts property names to snake_case
- Adds `serde(rename = "...")` when Rust name differs from OpenAPI name
- Automatically handles: keywords (type → r#type), special chars (user-id → user_id), case changes (userId → user_id)

### Documentation Generation

**Doc Comments** (`crates/oas3-gen/src/generator/utils.rs`):

- `doc_comment_lines()`: Converts OpenAPI descriptions to Rust doc comments (`///`)
- Handles literal `\n` escape sequences by normalizing to actual newlines
- Preserves multi-line documentation with proper formatting
- Empty lines converted to `/// ` (maintains doc comment continuity)
- Used throughout generated code for structs, enums, fields, and variants

**Location Hints** (`crates/oas3-gen/src/generator/operation_converter.rs`):

- Adds parameter location hints to generated request structs
- Format: `/// Path parameter`, `/// Query parameter`, etc.
- Helps developers understand parameter usage in API requests

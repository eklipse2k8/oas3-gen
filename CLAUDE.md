# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

This is a fully-functional OpenAPI-to-Rust code generator that parses OpenAPI 3.x specifications and generates comprehensive Rust type definitions with validation. The tool reads OpenAPI JSON files and generates idiomatic Rust code including structs, enums, type aliases, validation attributes, and Default implementations.

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

## Architecture

The codebase is organized into two main modules:
- **src/main.rs**: Entry point and orchestration
- **src/generator.rs**: Core generation logic (2368 lines)

### Core Components

**SchemaGraph** (`src/generator.rs:98-276`)
- Manages all schemas from the OpenAPI spec components
- Builds and tracks dependency relationships between schemas
- Detects circular dependencies using depth-first search
- Provides schema resolution and lookup functionality
- Key methods:
  - `build_dependencies()`: Analyzes schema references to build dependency graph
  - `detect_cycles()`: Identifies circular schema references
  - `is_cyclic()`: Checks if a schema is part of a cycle (used to add Box<T> wrappers)

**RustType AST** (`src/generator.rs:282-419`)
Represents the intermediate representation of generated Rust code:
- `RustType`: Enum containing Struct, Enum, or TypeAlias variants
- `StructDef`: Struct definition with fields, derives, and serde attributes
- `FieldDef`: Field definition with type, docs, validation, and default values
- `EnumDef`: Enum definition with variants and optional discriminator
- `VariantDef`: Enum variant (Unit, Tuple, or Struct content)
- `TypeRef`: Type reference with support for Box<T>, Option<T>, Vec<T> wrappers

**SchemaConverter** (`src/generator.rs:425-1456`)
Converts OpenAPI schemas to Rust AST structures:
- Handles all schema types: objects, enums, oneOf, anyOf, allOf
- Detects and handles nullable patterns (anyOf with null)
- Generates inline enum types for nested unions
- Extracts validation rules from OpenAPI constraints:
  - String length (min/max)
  - Number ranges (min/max, exclusive min/max)
  - Regex patterns
  - Format-based validation (email, URL)
- Manages cyclic references with Box<T> wrappers
- Handles discriminated unions with serde tag attribute
- Key methods:
  - `convert_schema()`: Main entry point for schema conversion
  - `convert_one_of_enum()`: Handles oneOf with discriminator support
  - `convert_any_of_enum()`: Handles anyOf with untagged enum generation
  - `convert_struct()`: Converts object schemas to structs
  - `schema_to_type_ref()`: Maps schemas to Rust type references

**OperationConverter** (`src/generator.rs:1462-1752`)
Generates request and response types for API operations:
- Creates request structs combining parameters and request body
- Orders parameters by location (path → query → header → cookie)
- Extracts response type references from operation definitions
- Generates OperationInfo metadata for tracking
- Properly handles optional vs required parameters
- Key methods:
  - `convert_operation()`: Main operation conversion entry point
  - `create_request_struct()`: Builds request type from parameters and body
  - `convert_parameter()`: Converts individual parameter to field definition

**CodeGenerator** (`src/generator.rs:1758-2367`)
Converts Rust AST to actual Rust source code using quote! macro:
- Generates regex validation constants with LazyLock pattern
- Deduplicates types using BTreeMap ordering
- Generates impl Default blocks for structs and enums
- Handles serde attributes (rename, rename_all, skip_serializing_if, tag, untagged)
- Generates validator attributes for runtime validation
- Converts JSON default values to Rust expressions
- Key methods:
  - `generate()`: Main code generation entry point
  - `generate_regex_constants()`: Creates static regex validators
  - `generate_default_impls()`: Generates Default trait implementations
  - `ordered_types()`: Deduplicates and orders types for output

**Main Orchestration & CLI** (`src/main.rs:7-153`)
The main function coordinates the generation pipeline with CLI argument handling:
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

### Key Dependencies

- **clap**: Command-line argument parsing with derive macros
- **oas3**: Core OpenAPI 3.x parsing library
- **serde/serde_json**: JSON serialization with preserved field order
- **quote/proc-macro2**: For generating Rust code as token streams
- **prettyplease**: Pretty-printing generated Rust code
- **syn**: Parsing Rust syntax for code formatting
- **validator**: Runtime validation attributes and traits
- **regex**: Regular expression validation support
- **inflections**: Case conversion utilities (PascalCase, snake_case, etc.)
- **any_ascii**: ASCII transliteration for identifier sanitization
- **tokio**: Async runtime for file I/O

### Type Mapping System

The `schema_to_type_ref()` method (`src/generator.rs:1246-1455`) maps OpenAPI types to Rust:

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

**Identifier Handling** (`src/generator.rs:41-64`):
- Converts OpenAPI names to valid Rust identifiers
- Replaces invalid characters (-, ., spaces) with underscores
- Handles Rust keyword conflicts with r# prefix
- ASCII transliteration for international characters

**Type Names** (`src/generator.rs:56-64`):
- Converts schema names to PascalCase
- Handles keyword conflicts (Self, Type)

**Field Names**:
- Converts property names to snake_case
- Adds serde(rename = "...") for kebab-case or special characters
- Uses rename_all when all fields follow consistent pattern

### Documentation Generation

**Doc Comments** (`src/generator.rs:21-38`):
- Converts OpenAPI descriptions to Rust doc comments (`///`)
- Handles literal `\n` escape sequences
- Preserves multi-line documentation
- Adds location hints for operation parameters (Path/Query/Header/Cookie)

## Features

### Fully Implemented
✅ CLI interface with clap (input/output paths, verbose/quiet modes)
✅ Automatic directory creation for output paths
✅ Schema parsing and conversion (objects, arrays, primitives, enums)
✅ oneOf/anyOf/allOf composition handling
✅ Discriminated and untagged union types
✅ Nullable type detection and Option<T> generation
✅ Cyclic dependency detection and Box<T> wrapper injection
✅ Operation to request/response type generation
✅ Parameter handling (path, query, header, cookie)
✅ Validation attribute generation (length, range, regex, email, URL)
✅ Default value generation with impl Default
✅ Regex pattern validation with static constants
✅ Doc comment generation from OpenAPI descriptions
✅ Serde attribute generation (rename, tag, untagged, skip_serializing_if)
✅ Inline enum type generation for nested unions
✅ Forward-compatible enum patterns with catch-all variants
✅ Type deduplication and ordering
✅ Pretty-printed output with rustfmt
✅ Configurable logging levels (normal, verbose, quiet)

### Not Yet Implemented
❌ Callback schema handling
❌ Link object processing
❌ Security scheme type generation
❌ Multi-file output (currently single file)
❌ Custom type mapping configuration

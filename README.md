# Rust OpenAPI 3.1 Type Generator

<!-- prettier-ignore-start -->
[![crates.io](https://img.shields.io/crates/v/oas3-gen?label=latest)](https://crates.io/crates/oas3-gen)
[![dependency status](https://deps.rs/crate/oas3-gen/0.12.1/status.svg)](https://deps.rs/crate/oas3-gen/0.12.1)
[![MIT licensed](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE.md)
[![openapi](https://badgen.net/badge/OAS/v3.1.1?list=1&color=purple)](https://github.com/OAI/OpenAPI-Specification)
<!-- prettier-ignore-end -->

`oas3-gen` is a command-line interface (CLI) for generating idiomatic Rust type definitions from an OpenAPI v3.1.x specification. The tool produces clean, production-ready code designed for seamless integration into any Rust project. Its primary function is to provide a robust and reliable method for type generation, ensuring the resulting code is correct, efficient, and well-documented.

## Quick Start

### 1. Installation

Install the tool directly from crates.io using `cargo`.

```sh
cargo install oas3-gen
```

### 2. Generation

Provide a path to an OpenAPI specification and specify an output file for the generated Rust code.

```sh
oas3-gen --input <path/to/openapi.json> --output <path/to/generated_types.rs>
```

#### Example

Consider the following OpenAPI schema definition in `schemas/pet.json`:

```json
{
  "Pet": {
    "type": "object",
    "description": "Represents a pet in the store.",
    "required": ["id", "name"],
    "properties": {
      "id": {
        "type": "integer",
        "format": "int64",
        "description": "The unique identifier for the pet."
      },
      "name": {
        "type": "string",
        "description": "The name of the pet."
      },
      "tag": {
        "type": "string",
        "description": "An optional tag for the pet."
      }
    }
  }
}
```

Executing `oas3-gen` produces the corresponding Rust types.

```rust
// src/generated_types.rs

use serde::{Deserialize, Serialize};

/// Represents a pet in the store.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Pet {
    /// The unique identifier for the pet.
    pub id: i64,
    /// The name of the pet.
    pub name: String,
    /// An optional tag for the pet.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tag: Option<String>,
}
```

## Key Features

- **Comprehensive OpenAPI 3.1 Support:** Parses schemas, parameters, request bodies, and responses from the latest OpenAPI specification.
- **Idiomatic Code Generation:** Creates Rust structs and enums that follow common language conventions.
- **Serde Integration:** Automatically derives `serde::Serialize` and `serde::Deserialize` for immediate use with JSON and other data formats.
- **Documentation Generation:** Converts OpenAPI schema descriptions directly into Rust documentation comments.
- **Complex Schema Resolution:** Correctly handles `allOf`, `oneOf`, and `anyOf` compositions to generate accurate and complex type definitions.
- **Cycle Detection:** Intelligently detects and manages cyclical dependencies between schemas, preventing infinite recursion in type definitions.
- **Convention-Aware Naming:** Detects `camelCase` and `snake_case` in the source schema and applies the appropriate `#[serde(rename_all = "...")]` attribute.
- **Operation Scaffolding:** Generates types for API operation parameters, request bodies, and responses.
- **Validation Support:** Translates OpenAPI constraints (e.g., `minLength`, `maxLength`, `pattern`, `minimum`, `maximum`) into validation attributes.
- **Enhanced CLI Experience:** Provides colored, timestamped output with automatic theme detection for improved readability in various terminal environments.

### Command-Line Options

```text
A rust type generator for OpenAPI v3.1.x specification.

Usage: oas3-gen [OPTIONS] --input <FILE> --output <FILE>

Options:
  -i, --input <FILE>             Path to the OpenAPI JSON specification file
  -o, --output <FILE>            Path where the generated Rust code will be written
      --visibility <VISIBILITY>  Visibility level for generated types [default: public]
                                 [possible values: public, crate, file]
  -v, --verbose                  Enable verbose output with detailed progress information
  -q, --quiet                    Suppress non-essential output (errors only)
      --color <COLOR>            Control color output [default: auto]
                                 [possible values: always, auto, never]
      --theme <THEME>            Terminal theme (dark or light background) [default: auto]
                                 [possible values: dark, light, auto]
  -h, --help                     Print help
  -V, --version                  Print version
```

### Examples

```sh
# Basic usage with automatic color and theme detection
oas3-gen -i openapi.json -o generated.rs

# Verbose output showing detailed statistics
oas3-gen -i openapi.json -o generated.rs --verbose

# Force dark theme with always-on colors
oas3-gen -i openapi.json -o generated.rs --theme dark --color always

# Generate with crate-level visibility
oas3-gen -i openapi.json -o generated.rs --visibility crate

# Quiet mode (errors only)
oas3-gen -i openapi.json -o generated.rs --quiet
```

## License

This project is licensed under the MIT License. See the [LICENSE.md](LICENSE.md) file for details.

### Contribution

Contributions are welcome! Please see [CONTRIBUTING.md](CONTRIBUTING.md) for more details on how to get started.

Unless you explicitly state otherwise, any contribution intentionally submitted for inclusion in the work by you shall be licensed as MIT, without any additional terms or conditions.

See [OpenAPI v3.1.x]: <https://spec.openapis.org/oas/v3.1.1>

# Rust OpenAPI 3.1 Type Generator

<!-- prettier-ignore-start -->
[![crates.io](https://img.shields.io/crates/v/oas3-gen?label=latest)](https://crates.io/crates/oas3-gen)
[![dependency status](https://deps.rs/crate/oas3-gen/0.7.1/status.svg)](https://deps.rs/crate/oas3-gen/0.7.1)
[![MIT licensed](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE.md)
[![openapi](https://badgen.net/badge/OAS/v3.1.1?list=1&color=purple)](https://github.com/OAI/OpenAPI-Specification)
<!-- prettier-ignore-end -->

`oas3-gen` is a command-line tool that generates idiomatic Rust type definitions from an OpenAPI v3.1.x specification. It is designed to create production-ready code that is easy to use and integrate into any Rust project.

The primary goal is to provide a robust and reliable way to generate Rust types from an OpenAPI specification, ensuring that the generated code is correct, efficient, and well-documented.

## Key Features

- **OpenAPI 3.1 Support:** Full support for the latest OpenAPI specification.
- **Idiomatic Rust Code:** Generates clean, readable, and idiomatic Rust structs and enums.
- **Serde Integration:** Automatically derives `serde::Serialize` and `serde::Deserialize` for seamless JSON and other format integration.
- **Documentation Generation:** Converts schema descriptions into Rust doc comments.
- **Complex Schema Support:** Handles `allOf`, `oneOf`, and `anyOf` for complex type compositions.
- **Cycle Detection:** Intelligently detects and handles cyclical dependencies between schemas.
- **Naming Conventions:** Automatically detects `camelCase` and `snake_case` naming conventions and applies `#[serde(rename_all = "...")]`.
- **Operation Generation:** Generates types for API operation parameters, request bodies, and responses.

## Installation

You can install `oas3-gen` directly from crates.io using `cargo`:

```sh
cargo install oas3-gen
```

## Usage

To generate Rust types, provide the path to your OpenAPI specification file and the desired output file.

```sh
oas3-gen --input <path/to/openapi.json> --output <path/to/generated_types.rs>
```

### Example

Given the following OpenAPI `schemas/pet.json`:

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

Running `oas3-gen` will produce the following Rust code in `src/generated_types.rs`:

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

### Command-Line Options

```
A rust type generator for OpenAPI v3.1.x specification.

Usage: oas3-gen [OPTIONS] --input <FILE> --output <FILE>

Options:
  -i, --input <FILE>   Path to the OpenAPI JSON specification file
  -o, --output <FILE>  Path where the generated Rust code will be written
  -v, --verbose        Enable verbose output with detailed progress information
  -q, --quiet          Suppress non-essential output (errors only)
  -h, --help           Print help
  -V, --version        Print version
```

## License

This project is licensed under the MIT License. See the [LICENSE.md](LICENSE.md) file for details.

### Contribution

Contributions are welcome! Please see [CONTRIBUTING.md](CONTRIBUTING.md) for more details on how to get started.

Unless you explicitly state otherwise, any contribution intentionally submitted for inclusion in the work by you shall be licensed as MIT, without any additional terms or conditions.

See [OpenAPI v3.1.x]: <https://spec.openapis.org/oas/v3.1.1>

# Rust OpenAPI 3.1 Type Generator

<!-- prettier-ignore-start -->
[![crates.io](https://img.shields.io/crates/v/oas3-gen?label=latest)](https://crates.io/crates/oas3-gen)
[![dependency status](https://deps.rs/crate/oas3-gen/0.17.1/status.svg)](https://deps.rs/crate/oas3-gen/0.17.1)
[![MIT licensed](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE.md)
[![openapi](https://badgen.net/badge/OAS/v3.1.1?list=1&color=purple)](https://github.com/OAI/OpenAPI-Specification)
<!-- prettier-ignore-end -->

`oas3-gen` is a command-line interface (CLI) for generating idiomatic Rust type definitions from an OpenAPI v3.1.x specification. The tool produces clean, production-ready code designed for seamless integration into any Rust project. Its primary function is to provide a robust and reliable method for type generation, ensuring the resulting code is correct, efficient, and well-documented.

## Quick Start

### 1. Installation

Install the tool directly from crates.io using `cargo`.

```zsh
cargo install oas3-gen
```

### 2. Generation

Provide a path to an OpenAPI specification and specify an output file for the generated Rust code.

```zsh
# generate types (structs and enums)
oas3-gen generate -i path/to/openapi.json -o path/to/types.rs

# generate client operations
oas3-gen generate client -i path/to/openapi.json -o path/to/client.rs
```

#### Example

Consider the following Pet OpenAPI schema definition in `fixtures/petstore.json`:

```json
{
  "Pet": {
    "type": "object",
    "required": ["id", "name"],
    "properties": {
     "id": {
      "type": "integer",
      "format": "int64"
     },
     "name": {
      "type": "string"
     },
     "tag": {
      "type": "string"
     }
    }
  }
}
```

Executing `oas3-gen` produces the corresponding Rust types.

```rust
// src/types.rs

use serde::Deserialize;

#[derive(Debug, Clone, PartialEq, oas3_gen_support::Default, Deserialize)]
pub struct Pet {
  pub id: i64,
  pub name: String,
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
- **Operation Filtering:** Selectively generate code for specific operations using `--only` or exclude operations with `--exclude` for fine-grained control.

### Command-Line Options

```text
OpenAPI to Rust code generator

Usage: oas3-gen [OPTIONS] <COMMAND>

Commands:
  list      List information from OpenAPI specification
  generate  Generates idiomatic, type-safe Rust code from an OpenAPI v3.1 (OAS31) specification
  help      Print this message or the help of the given subcommand(s)

Options:
  -h, --help     Print help
  -V, --version  Print version

Terminal Output:
      --color <WHEN>   Coloring [default: auto] [possible values: always, auto, never]
      --theme <THEME>  Theme [default: auto] [possible values: dark, light, auto]
```

#### Generate Command

```text
Generates idiomatic, type-safe Rust code from an OpenAPI v3.1 (OAS31) specification

Usage: oas3-gen generate [OPTIONS] --input <FILE> --output <FILE> [MODE]

Arguments:
  [MODE]  Sets the generation mode [default: types] [possible values: types, client]

Required:
  -i, --input <FILE>   Path to the OpenAPI specification file
  -o, --output <FILE>  Path for the generated rust output file

Code Generation:
  -C, --visibility <PUB>  Module visibility for generated items [default: public] [possible values: public, crate, file]

Operation Filtering:
      --only <id_1,id_2,...>     Include only the specified comma-separated operation IDs
      --exclude <id_1,id_2,...>  Exclude the specified comma-separated operation IDs
      --all-schemas              Generate all schemas, even those unreferenced by selected operations
```

#### List Command

```text
List information from OpenAPI specification

Usage: oas3-gen list <COMMAND>

Commands:
  operations  List all operations defined in the OpenAPI specification
  help        Print this message or the help of the given subcommand(s)

Options:
  -h, --help  Print help
```

### Examples

```zsh
# Basic usage to generate types
oas3-gen generate types -i openapi.json -o types.rs

# Basic usage to generate companion client
oas3-gen generate client -i openapi.json -o client.rs

# Generate types with crate-level visibility
oas3-gen generate -i openapi.json -o types.rs -C crate

# Generate all schemas types including unused ones
oas3-gen generate -i openapi.json -o types.rs --all-schemas

# Generate code for specific operation types only
oas3-gen generate -i openapi.json -o types.rs --only create_user,get_user,update_user

# Generate code excluding certain operation types
oas3-gen generate -i openapi.json -o types.rs --exclude delete_user,list_users

# Generate all schemas but only specific operation types (includes unreferenced schemas)
oas3-gen generate -i openapi.json -o types.rs --all-schemas --only create_user

# List all operations in the specification
oas3-gen list operations -i openapi.json
```

## License

This project is licensed under the MIT License. See the [LICENSE.md](LICENSE.md) file for details.

### Contribution

Contributions are welcome! Please see [CONTRIBUTING.md](CONTRIBUTING.md) for more details on how to get started.

Unless you explicitly state otherwise, any contribution intentionally submitted for inclusion in the work by you shall be licensed as MIT, without any additional terms or conditions.

See [OpenAPI v3.1.x]: <https://spec.openapis.org/oas/v3.1.1>

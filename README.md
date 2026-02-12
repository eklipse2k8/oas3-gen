# (OAS3-GEN) OpenAPI 3.1+ Rust Generator

```rust
//     ██████╗  █████╗ ███████╗██████╗        ██████╗ ███████╗███╗   ██╗
//    ██╔═══██╗██╔══██╗██╔════╝╚════██╗      ██╔════╝ ██╔════╝████╗  ██║
//    ██║   ██║███████║███████╗ █████╔╝█████╗██║  ███╗█████╗  ██╔██╗ ██║
//    ██║   ██║██╔══██║╚════██║ ╚═══██╗╚════╝██║   ██║██╔══╝  ██║╚██╗██║
//    ╚██████╔╝██║  ██║███████║██████╔╝      ╚██████╔╝███████╗██║ ╚████║
//     ╚═════╝ ╚═╝  ╚═╝╚══════╝╚═════╝        ╚═════╝ ╚══════╝╚═╝  ╚═══╝
```

<!-- prettier-ignore-start -->
[![crates.io](https://img.shields.io/crates/v/oas3-gen?label=latest)](https://crates.io/crates/oas3-gen)
[![dependency status](https://deps.rs/crate/oas3-gen/0.25.2/status.svg)](https://deps.rs/crate/oas3-gen/0.25.2)
[![MIT licensed](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE.md)
[![openapi](https://badgen.net/badge/OAS/v3.1.2?list=1&color=purple)](https://github.com/OAI/OpenAPI-Specification/blob/main/versions/3.1.2.md)
<!-- prettier-ignore-end -->

`oas3-gen` is a command-line interface (CLI) for generating idiomatic Rust type definitions from an OpenAPI v3.1.x specification. The tool produces clean, production-ready code designed for seamless integration into any Rust project. Its primary function is to provide a robust and reliable method for type generation, ensuring the resulting code is correct, efficient, and well-documented.

## Quick Start

### 1. Installation

Install the tool directly from crates.io using `cargo`.

```zsh
cargo install oas3-gen
```

#### Alternative build with [Nix](https://nixos.org/)

 -  Make sure you have `nix` installed or install it with:
    ```shell
    sh <(curl --proto '=https' --tlsv1.2 -L https://nixos.org/nix/install) --daemon
    ```
 -  Now simply run it with:
    - `nix run github:eklipse2k8/oas3-gen` will run the binary, fetching the git repo automatically.
    - Or locally in the repo with `nix run`
    - To globally install use `nix profile install`

This takes cares of build dependencies (`openssl`, `pkg-config`) and they are packaged reproducibly and defined in `flake.nix`.
A development shell is included and can be accessed by running `nix develop` or use `direnv allow` if available.

### 2. Generation

Provide a path to an OpenAPI specification (JSON or YAML) and specify an output file for the generated Rust code. The format is auto-detected based on file extension.

```zsh
# generate types (structs and enums)
oas3-gen generate -i path/to/openapi.json -o path/to/types.rs
oas3-gen generate -i path/to/openapi.yaml -o path/to/types.rs

# generate client operations
oas3-gen generate client -i path/to/openapi.json -o path/to/client.rs

# generate server module (types.rs, server.rs, mod.rs)
oas3-gen generate server-mod -i path/to/openapi.json -o path/to/output/
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

#[derive(Debug, Clone, PartialEq, Deserialize, oas3_gen_support::Default)]
pub struct Pet {
  pub id: i64,
  pub name: String,
  pub tag: Option<String>,
}
```

### Server Generation

The `server-mod` command generates an axum-based server trait with handler functions and router configuration.

```zsh
oas3-gen generate server-mod -i path/to/openapi.json -o path/to/output/
```

This generates three files:
- `types.rs` - All request/response types
- `server.rs` - Server trait, handlers, and router
- `mod.rs` - Module exports

#### Generated Server Trait

```rust
// server.rs

pub trait ApiServer: Send + Sync {
    fn list_pets(&self, request: ListPetsRequest)
        -> impl std::future::Future<Output = anyhow::Result<ListPetsResponse>> + Send;

    fn create_pet(&self, request: CreatePetRequest)
        -> impl std::future::Future<Output = anyhow::Result<CreatePetResponse>> + Send;

    fn get_pet_by_id(&self, request: GetPetByIdRequest)
        -> impl std::future::Future<Output = anyhow::Result<GetPetByIdResponse>> + Send;
}
```

#### Implementing the Server

```rust
use generated::{ApiServer, ListPetsRequest, ListPetsResponse, /* ... */};

#[derive(Clone)]
struct MyServer {
    db: DatabasePool,
}

impl ApiServer for MyServer {
    fn list_pets(&self, request: ListPetsRequest)
        -> impl std::future::Future<Output = anyhow::Result<ListPetsResponse>> + Send
    {
        async move {
            let pets = self.db.query_pets(request.query.limit).await?;
            Ok(ListPetsResponse::Ok(pets))
        }
    }
    // ... implement other methods
}

#[tokio::main]
async fn main() {
    let server = MyServer { db: create_pool().await };
    let app = generated::router(server);
    let listener = tokio::net::TcpListener::bind("0.0.0.0:3000").await.unwrap();
    axum::serve(listener, app).await.unwrap();
}
```

The generated router automatically:
- Extracts path, query, and header parameters
- Deserializes request bodies (JSON, form, multipart)
- Routes requests to the correct handler based on path and HTTP method
- Converts response enums to proper HTTP responses with status codes

## Key Features

| Feature | Description |
|---------|-------------|
| Custom Formats | Use `serde_as` to parse formats |
| Cycle Detection | Prevents infinite type recursion |
| Doc Comments | Schema descriptions become rustdoc |
| Enum Helpers | Ergonomic is_/as_ methods |
| Enum Modes | Merge, preserve, or relaxed |
| Event stream | Simple event stream capture if media-type is specified |
| JSON/YAML Support | Auto-detects format from file extension |
| OData Support | Optional @odata.* field handling |
| OpenAPI 3.1 | Most common spec parsing support |
| Operation Filtering | Include/exclude specific operations |
| Operation Types | Request/response type generation |
| Schema Composition | Handles allOf/oneOf/anyOf correctly |
| Server Generation | Axum server trait scaffolding |
| Serde Integration | Automatic derive for serialization |
| Smart Naming | Auto-detects camelCase/snake_case conventions |
| Validation | Constraint attributes from spec |
| Builder Pattern | Optional `bon` integration for ergonomic struct construction |
| Webhooks | Generates structs from Webhook components |

### Missing features

* OAS 3.1 Links and `$dynamic-ref` (oas3 doesn't support this yet)
* OAS 3.2 Event-stream support
* External schema references
* HTTP schema references and fetching

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
  [MODE]  Sets the generation mode [default: types] [possible values: types, client, client-mod, server-mod]

Required:
  -i, --input <FILE>   Path to the OpenAPI specification file
  -o, --output <PATH>  Path for generated output (file for types/client, directory for client-mod/server-mod)

Code Generation:
  -C, --visibility <PUB>       Module visibility for generated items [default: public] [possible values: public, crate, file]
      --odata-support          Enable OData-specific field optionality rules (makes @odata.* fields optional on concrete types)
      --enum-mode <ENUM_MODE>  Specifies how to handle enum case sensitivity and duplicates [default: merge] [possible values: merge, preserve, relaxed]
      --no-helpers             Disable generation of ergonomic helper methods for enum variants
  -c, --customize <TYPE=PATH>  Custom serde_as type overrides (format: type_name=custom::Path)
      --enable-builders        Enable bon builder derives on schema structs and builder methods on request structs
      --doc-format             Format documentation comments using mdformat (requires mdformat installed)
      --all-headers            Emit header constants for all component-level header parameters

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
# Basic usage to generate a client module from a json schema file
oas3-gen generate client-mod -i openapi.json -o generated

# Generate a server module with axum trait scaffolding
oas3-gen generate server-mod -i openapi.json -o generated

# Or generate types and client code individually ...

# Generate types with crate-level visibility
oas3-gen generate types -i openapi.json -o types.rs -C crate
oas3-gen generate client -i openapi.json -o client.rs -C crate

# Generate all schemas types including unused ones
oas3-gen generate -i openapi.json -o types.rs --all-schemas

# Generate code for specific operation types only
oas3-gen generate -i openapi.json -o types.rs --only create_user,get_user,update_user

# Generate code excluding certain operation types
oas3-gen generate -i openapi.json -o types.rs --exclude delete_user,list_users

# Generate all schemas but only specific operation types (includes unreferenced schemas)
oas3-gen generate -i openapi.json -o types.rs --all-schemas --only create_user

# Enable OData support for Microsoft Graph
oas3-gen generate -i graph-api.json -o types.rs --odata-support

# Enable relaxed (case-insensitive) enum deserialization
oas3-gen generate -i openapi.json -o types.rs --enum-mode relaxed

# Enable custom parsing through serde_as traits
oas3-gen generate client-mod -i openapi.json -o generated --customize datetime=MyCustomDateTime

# Enable bon builder derives for ergonomic struct construction
oas3-gen generate client-mod -i openapi.json -o generated --enable-builders

# Format documentation comments with mdformat
oas3-gen generate client-mod -i openapi.json -o generated --doc-format

# List all operations in the specification
oas3-gen list operations -i openapi.json
```

## Documentation Formatting with `mdformat`

The `--doc-format` flag pipes generated documentation comments through
[mdformat](https://github.com/executablebooks/mdformat), a CommonMark-compliant
Markdown formatter. This reformats long descriptions from OpenAPI `summary` and
`description` fields into consistently wrapped, readable rustdoc comments. Lines are wrapped at 100 characters and markdown structure (headings, blockquotes, lists) is normalized.

The `mdformat` binary must be available on your `PATH` when using `--doc-format`.
If it is not installed, the command will fail with an error. The flag is entirely
optional and has no effect on the structure or correctness of generated code.

## License

This project is licensed under the MIT License. See the [LICENSE.md](LICENSE.md) file for details.

```md
Copyright (c) 2026 Individual contributors
```

### Contribution

Contributions are welcome! Please see [CONTRIBUTING.md](CONTRIBUTING.md) for more details on how to get started.

Unless you explicitly state otherwise, any contribution intentionally submitted for inclusion in the work by you shall be licensed as MIT, without any additional terms or conditions.

See [OpenAPI v3.1.2]: <https://spec.openapis.org/oas/v3.1.2>

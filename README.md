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
[![dependency status](https://deps.rs/crate/oas3-gen/0.29.0/status.svg)](https://deps.rs/crate/oas3-gen/0.29.0)
[![MIT licensed](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE.md)
[![openapi](https://badgen.net/badge/OAS/v3.1.2?list=1&color=purple)](https://github.com/OAI/OpenAPI-Specification/blob/main/versions/3.1.2.md)
<!-- prettier-ignore-end -->

`oas3-gen` is a command-line interface (CLI) for generating idiomatic Rust type definitions from an OpenAPI v3.1.x specification. The tool produces clean, production-ready code designed for seamless integration into any Rust project. Its primary function is to provide a robust and reliable method for type generation, ensuring the resulting code is correct, efficient, and well-documented.

Generated output preserves the declaration order written in the OpenAPI document for schemas, fields, enum variants, union variants, operations, and header constants. Map-like generated types use `indexmap::IndexMap`, and arrays marked with `uniqueItems: true` use `indexmap::IndexSet` so runtime insertion order is preserved too. An `IndexSet` drops duplicate items rather than rejecting them. Pass `--no-ordered-collections` to opt out of the `indexmap` runtime types and emit `std::collections::HashMap<String, T>` plus `Vec<T>` instead.

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

Consider the following `Pet` OpenAPI schema definition in `fixtures/petstore.json`:

```json
{
  "Pet": {
    "type": "object",
    "description": "A pet in the store's care.",
    "required": ["id", "name", "species"],
    "properties": {
      "id": { "type": "string", "format": "uuid", "readOnly": true, "description": "Server-assigned identifier" },
      "name": { "type": "string", "minLength": 1, "maxLength": 64 },
      "species": { "$ref": "#/components/schemas/Species" },
      "status": { "$ref": "#/components/schemas/PetStatus" },
      "nickname": { "type": ["string", "null"], "description": "An optional nickname" },
      "birthDate": { "type": "string", "format": "date" },
      "weightKg": { "type": "number", "format": "double", "exclusiveMinimum": 0, "maximum": 500 },
      "microchip": { "type": "string", "pattern": "^[0-9]{15}$", "description": "ISO 11784 microchip number" },
      "tags": { "type": "array", "items": { "type": "string" }, "uniqueItems": true, "maxItems": 10 },
      "photoUrls": { "type": "array", "items": { "type": "string", "format": "uri" } },
      "allergies": { "$ref": "#/components/schemas/Health" },
      "vaccinations": { "$ref": "#/components/schemas/PetVaccinations" },
      "createdAt": { "type": "string", "format": "date-time", "readOnly": true },
      "tag": { "type": "string", "deprecated": true, "description": "Superseded by `tags`" }
    }
  }
}
```

Executing `oas3-gen generate -i fixtures/petstore.json -o types.rs` produces the corresponding Rust type, along with the types it refers to, such as `Species`, `PetStatus`, and `Health`.

```rust
// types.rs

use serde::{Deserialize, Serialize};

/// A pet in the store's care.
#[derive(Debug, Clone, PartialEq, Deserialize, oas3_gen_support::Default)]
pub struct Pet {
    /// Server-assigned identifier
    pub id: uuid::Uuid,
    pub name: String,
    /// Kinds of pets the store cares for
    pub species: Species,
    /// Where the pet is in the adoption process
    pub status: Option<PetStatus>,
    /// An optional nickname
    pub nickname: Option<String>,
    #[serde(rename = "birthDate")]
    pub birth_date: Option<chrono::NaiveDate>,
    #[serde(rename = "weightKg")]
    pub weight_kg: Option<f64>,
    /// ISO 11784 microchip number
    pub microchip: Option<String>,
    pub tags: Option<indexmap::IndexSet<String>>,
    #[serde(rename = "photoUrls")]
    pub photo_urls: Option<Vec<String>>,
    /// A health note, tagged by `type` without an explicit mapping.
    pub allergies: Option<Box<Health>>,
    /// Vaccination records keyed by vaccine family
    pub vaccinations: Option<indexmap::IndexMap<String, Vec<VaccineRecord>>>,
    #[serde(rename = "createdAt")]
    pub created_at: Option<chrono::DateTime<chrono::Utc>>,
    /// Superseded by `tags`
    #[deprecated]
    pub tag: Option<String>,
}
```

`Pet` only appears in responses, so it derives `Deserialize` and carries no validation. `NewPet`, the schema a caller sends to create a pet, derives `Serialize` and `validator::Validate` instead, with its `minLength`, `maxLength`, `pattern`, and other constraints as validation attributes.

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
    fn list_pets(
        &self,
        request: ListPetsRequest,
    ) -> impl std::future::Future<
        Output = anyhow::Result<
            ApiResponse<
                WithHeaders<XNextAndXTotalCountAndXRequestIdHeaders, Pets>,
                Error,
            >,
        >,
    > + Send;

    fn create_pet(
        &self,
        request: CreatePetRequest,
    ) -> impl std::future::Future<
        Output = anyhow::Result<ApiResponse<WithHeaders<LocationHeaders, Pet>, Error>>,
    > + Send;

    fn show_pet_by_id(
        &self,
        request: ShowPetByIdRequest,
    ) -> impl std::future::Future<
        Output = anyhow::Result<
            ApiResponse<WithHeaders<ETagAndXCacheHeaders, Pet>, ErrorOrProblem>,
        >,
    > + Send;

    // ... one method per operation and webhook
}
```

#### Implementing the Server

```rust
use generated::{
    ApiResponse, ApiServer, Error, ListPetsRequest, Pets, WithHeaders,
    XNextAndXTotalCountAndXRequestIdHeaders, /* ... */
};

#[derive(Clone)]
struct MyServer {
    db: DatabasePool,
}

impl ApiServer for MyServer {
    async fn list_pets(
        &self,
        request: ListPetsRequest,
    ) -> anyhow::Result<
        ApiResponse<WithHeaders<XNextAndXTotalCountAndXRequestIdHeaders, Pets>, Error>,
    > {
        let pets = self.db.query_pets(request.query.limit).await?;
        Ok(ApiResponse::Ok(WithHeaders {
            headers: XNextAndXTotalCountAndXRequestIdHeaders::default(),
            body: pets,
        }))
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
- Extracts path, query, and header parameters, including arrays sent as repeated query parameters
- Extracts API keys and bearer tokens declared by the operation's security requirements, answering a request that lacks required credentials with `401`
- Deserializes request bodies (JSON, form, multipart, text, and binary), answering malformed ones with `400` before your method runs
- Routes requests to the correct handler based on path and HTTP method
- Converts `ApiResponse` values into HTTP responses with the status, headers, and media type each response declares
- Streams Server-Sent Events for `text/event-stream` responses through Axum's `Sse`, from the `BoxStream` your method returns

## Key Features

| Feature | Description |
|---------|-------------|
| Custom Formats | Use `serde_as` to parse formats |
| Cycle Detection | Prevents infinite type recursion |
| Doc Comments | Schema descriptions become rustdoc |
| Enum Helpers | Ergonomic is_/as_ methods |
| Enum Modes | Merge, preserve, or relaxed |
| JSON/YAML Support | Auto-detects format from file extension |
| OData Support | Optional @odata.* field handling |
| OpenAPI 3.1 | Most common spec parsing support |
| Operation Filtering | Include/exclude specific operations |
| Operation Types | Request/response type generation |
| Order Preserving | Mirrors OpenAPI declaration order in generated code |
| Schema Composition | Handles allOf/oneOf/anyOf correctly |
| Server Generation | Axum server trait scaffolding |
| Server-Sent Events | `text/event-stream` responses stream on the client and the server |
| Serde Integration | Automatic derive for serialization |
| Smart Naming | Auto-detects camelCase/snake_case conventions |
| Validation | Constraint attributes from spec, including `multipleOf` |
| Builder Pattern | Optional `bon` integration for ergonomic struct construction |
| Webhooks | Generates structs from Webhook components |

### Missing features

* OAS 3.1 Links and `$dynamic-ref` (oas3 doesn't support this yet)
* OAS 3.2 Event-stream support
* External schema references
* HTTP schema references and fetching
* Cookie parameters (`in: cookie`); the generator skips each one with a warning

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

Usage: oas3-gen generate [OPTIONS] --input <FILE> [MODE]

Arguments:
  [MODE]  Sets the generation mode [default: types] [possible values: types, client, client-mod, server-mod]

Required:
  -i, --input <FILE>   Path to the OpenAPI specification file
  -o, --output <PATH>  Path for generated output (file for types/client, directory for client-mod/server-mod)

Code Generation:
  -C, --visibility <PUB>           Module visibility for generated items [default: public] [possible values: public, crate, file]
      --odata-support              Enable OData-specific field optionality rules (makes @odata.* fields optional on concrete types)
      --enum-layout <ENUM_LAYOUT>  Controls how enum variants are ordered in generated code [default: spec] [possible values: spec, sorted]
      --enum-mode <ENUM_MODE>      Specifies how to handle enum case sensitivity and duplicates [default: merge] [possible values: merge, preserve, relaxed]
      --no-helpers                 Disable generation of ergonomic helper methods for enum variants
  -c, --customize <TYPE=PATH>      Custom serde_as type overrides (format: type_name=custom::Path)
      --all-headers                Emit header constants for all parameters defined in components, not just those used in operations
      --enable-builders            Enable bon builder derives on schema structs and builder methods on request structs
      --doc-format                 Format documentation comments using mdformat (requires mdformat installed)
      --no-ordered-collections     Emit std::collections::HashMap and Vec instead of indexmap::IndexMap/IndexSet for map fields and uniqueItems arrays
      --module-version <VERSION>   Version written to the [package] table of the generated Cargo.toml (requires --workspace) [default: 0.0.0]
  -w, --workspace                  Emit a workspace-compatible Cargo.toml and place generated sources in an inner src/ directory (requires client-mod or server-mod)

Operation Filtering:
      --only <id_1,id_2,...>     Include only the specified comma-separated operation IDs
      --exclude <id_1,id_2,...>  Exclude the specified comma-separated operation IDs
      --all-schemas              Generate all schemas, even those unreferenced by selected operations
```

#### List Command

```text
List information from OpenAPI specification

Usage: oas3-gen list [OPTIONS] <COMMAND>

Commands:
  operations  List all operations defined in the OpenAPI specification
  help        Print this message or the help of the given subcommand(s)

Options:
  -h, --help     Print help
  -V, --version  Print version

Terminal Output:
      --color <WHEN>   Coloring [default: auto] [possible values: always, auto, never]
      --theme <THEME>  Theme [default: auto] [possible values: dark, light, auto]
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

# Sort enum variants alphabetically instead of preserving spec order
oas3-gen generate -i openapi.json -o types.rs --enum-layout sorted

# Enable custom parsing through serde_as traits
oas3-gen generate client-mod -i openapi.json -o generated --customize datetime=MyCustomDateTime

# Enable bon builder derives for ergonomic struct construction
oas3-gen generate client-mod -i openapi.json -o generated --enable-builders

# Opt out of indexmap runtime types (emit std::collections::HashMap and Vec)
oas3-gen generate client-mod -i openapi.json -o generated --no-ordered-collections

# Format documentation comments with mdformat
oas3-gen generate client-mod -i openapi.json -o generated --doc-format

# Generate a standalone crate (Cargo.toml + src/) with a pinned package version
oas3-gen generate client-mod -i openapi.json -o my-api --workspace --module-version 1.0.0

# Show detailed progress information during generation
oas3-gen generate client-mod -i openapi.json -o generated --verbose

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

All contributions submitted for inclusion in the work must be code-signed by an active _HUMAN_ GitHub account and shall be licensed as MIT, without any additional terms or conditions. No bot or agent generated contributions will be accepted and will be immediately rejected.

See [OpenAPI v3.1.2]: <https://spec.openapis.org/oas/v3.1.2>

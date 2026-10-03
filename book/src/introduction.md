# oas3-gen

Welcome to the `oas3-gen` book. `oas3-gen` reads an OpenAPI 3.1 specification
and generates Rust code for the API it describes. You can generate types to use
in your own code, an HTTP client built on [`reqwest::Client`][rustdoc-reqwest-client], or a server trait
with an [`axum::Router`][rustdoc-axum-router].

This book assumes you're familiar with Rust structs, enums, and Cargo. We'll
look at the generated code and work through the options that change it. You
don't need to know how the generator itself is implemented.

## Overview

An OpenAPI document describes an API's operations and the data they accept and
return. `oas3-gen` uses those descriptions to create Rust structs, enums, and
type aliases. It also generates [`serde::Serialize`][rustdoc-serde-serialize] and [`serde::Deserialize`][rustdoc-serde-deserialize] implementations
where the types need them, along with validation for supported schema
constraints.

You can start with type generation and add a client or server as your project
needs it. [Client Generation](./client-generation.md) walks through calling an
API, reading responses, and sending API keys and bearer tokens. [Server Generation](./server-generation.md)
shows how to implement a service, connect its router, and check incoming
credentials.

[Shared Generation Options](./code-generation.md) explains type generation,
response shapes, security requirements, and flags that apply to both targets.
The [Builder Pattern](./builders.md) chapter shows how to construct generated
values with the optional `bon` integration. You can follow the client or server
chapter first and refer to the shared options as you need them.

## Features

The generator supports several parts of working with an OpenAPI description:

- Generate structs, enums, and type aliases from schemas, including unions
  described with `oneOf`, `anyOf`, and discriminators.
- Call API operations through an asynchronous `reqwest` client.
- Implement a generated server trait and connect it to an Axum router.
- Send and receive API keys and bearer tokens declared by `apiKey` and `http`
  bearer security schemes.
- Check supported field constraints with [`validator::Validate`][rustdoc-validate].
- Construct schema values and requests with builders by passing
  `--enable-builders`.

We'll introduce each option alongside examples of the code it produces.

## Requirements

You'll need Rust 1.89 or later and an OpenAPI 3.1.x specification. The generator
accepts both JSON and YAML documents.

## Installation

To install the command-line tool, run:

```bash
cargo install oas3-gen
```

Cargo builds the tool and installs the `oas3-gen` executable. If you'd like to
build from this repository instead, run:

```bash
git clone https://github.com/eklipse2k8/oas3-gen
cd oas3-gen
cargo build --release
```

The source build places the executable in `target/release/`. You can also run
it from the repository with `cargo run --`, followed by the same arguments used
with `oas3-gen` below.

## Quick Start

Let's start by generating types. The following commands assume you have an
OpenAPI document named `api.json` in your current directory:

```bash
oas3-gen generate types -i api.json -o types.rs
```

The `-i` argument names the input document, and `-o` names the output file. This
command writes the types used by your API's operations to `types.rs`.

To generate an HTTP client together with those types, change the mode to
`client`:

```bash
oas3-gen generate client -i api.json -o client.rs
```

Both the types and the client go into `client.rs`. If you'd prefer separate
files, use `client-mod` and give `-o` a directory:

```bash
oas3-gen generate client-mod -i api.json -o src/api/
```

This creates `types.rs`, `client.rs`, and `mod.rs` in `src/api/`. For a server,
use `server-mod`:

```bash
oas3-gen generate server-mod -i api.json -o src/server/
```

The server output contains `types.rs`, `server.rs`, and `mod.rs`. You'll
implement the generated trait to provide the behavior of each API operation.
Continue with [Client Generation](./client-generation.md) to call an API or
[Server Generation](./server-generation.md) to implement one.

These commands generate source files to include in a Rust project. To generate
an accompanying `Cargo.toml`, add `--workspace` to either module mode. See
[Workspace Crate Output](./code-generation.md#workspace-crate-output) for an
example and instructions on using the generated crate.

## Supported Formats

You can use a JSON specification with a `.json` extension or a YAML
specification with a `.yaml` or `.yml` extension. The examples in this book use
JSON; the same generation options apply to YAML input.

## License

`oas3-gen` is available under the MIT license.

[rustdoc-axum-router]: https://docs.rs/axum/0.8.9/axum/struct.Router.html
[rustdoc-reqwest-client]: https://docs.rs/reqwest/0.13.5/reqwest/struct.Client.html
[rustdoc-serde-deserialize]: https://docs.rs/serde/1.0.229/serde/trait.Deserialize.html
[rustdoc-serde-serialize]: https://docs.rs/serde/1.0.229/serde/trait.Serialize.html
[rustdoc-validate]: https://docs.rs/validator/0.21.0/validator/trait.Validate.html

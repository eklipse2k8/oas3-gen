# oas3-gen

A Rust code generator for OpenAPI v3.1.x specifications.

## Overview

`oas3-gen` parses OpenAPI 3.1 specifications and generates comprehensive Rust type
definitions with validation. It produces idiomatic Rust code with full serde
support, making it easy to integrate with HTTP clients and servers.

## Features

- **Type Generation**: Structs, enums, and type aliases from OpenAPI schemas
- **HTTP Client**: Generated async client using `reqwest`
- **Axum Server**: Generated server trait and router for `axum`
- **Validation**: Field validation using the `validator` crate
- **Serde Support**: Full serialization/deserialization with `serde`
- **Discriminated Unions**: Proper handling of `oneOf`/`anyOf` with discriminators
- **Builder Pattern**: Optional [`bon`](https://docs.rs/bon/latest/bon/) integration for ergonomic struct construction via `--enable-builders`

## Installation

```bash
cargo install oas3-gen
```

Or build from source:

```bash
git clone https://github.com/eklipse2k8/oas3-gen
cd oas3-gen
cargo build --release
```

## Quick Start

Generate types from an OpenAPI specification:

```bash
oas3-gen generate types -i api.json -o types.rs
```

Generate a complete HTTP client:

```bash
oas3-gen generate client -i api.json -o client.rs
```

Generate a modular client library:

```bash
oas3-gen generate client-mod -i api.json -o src/api/
```

Generate an Axum server trait:

```bash
oas3-gen generate server-mod -i api.json -o src/server/
```

## Supported Formats

- JSON specifications (`.json`)
- YAML specifications (`.yaml`, `.yml`)

## Requirements

- Rust 1.89 or later
- OpenAPI 3.1.x specification

## License

MIT

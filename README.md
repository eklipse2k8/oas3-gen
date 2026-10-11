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

Generate Rust types, a `reqwest` HTTP client, or an `axum` server trait and router
from an OpenAPI 3.1 document. The generator accepts JSON and YAML input.

## Quick Start

You'll need Rust 1.89 or later. Install the generator:

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

Generate a client crate from your API document:

```zsh
oas3-gen generate client-mod -i api.json -o my-api --workspace
cargo check --manifest-path my-api/Cargo.toml
```

Replace `api.json` with the path to your document. To try the repository's
example, use [fixtures/petstore.json](fixtures/petstore.json). The output
contains a `Cargo.toml` with the required dependencies and a `src/` directory
with the client and its types.

To generate a server crate, use `server-mod`:

```zsh
oas3-gen generate server-mod -i api.json -o my-server --workspace
```

You'll implement the generated trait to handle requests. If you only need
data types, generate a single file:

```zsh
oas3-gen generate types -i api.json -o types.rs
```

## Generated Client

The petstore client includes this method for its health endpoint. This excerpt
omits imports and the other methods:

```rust
impl PetstoreClient {
    pub async fn get_health(
        &self,
        request: GetHealthRequest,
    ) -> anyhow::Result<ApiResponse<(), ()>> {
        request.validate().context("parameter validation")?;
        let mut url = self.base_url.clone();
        url.path_segments_mut()
            .map_err(|()| anyhow::anyhow!("URL cannot be a base"))?
            .push("health");
        let response = self.client.get(url).send().await?;
        GetHealthRequest::parse_response(response).await
    }
}
```

The method sends the request through [`reqwest::Client`][reqwest-client] and
passes the response to the generated decoder.

## Generated Server

Server output declares a trait method for the same operation. This excerpt
omits the other operations:

```rust
pub trait ApiServer: Send + Sync {
    fn get_health(
        &self,
        request: GetHealthRequest,
    ) -> impl std::future::Future<Output = anyhow::Result<ApiResponse<(), ()>>> + Send;
}
```

Implement `ApiServer`, then pass your service to the generated `router()`
function. It returns an [`axum::Router`][axum-router] that extracts requests,
calls your methods, and writes HTTP responses.

## Features

| Feature | Generated behavior |
|---|---|
| Types | Structs, enums, type aliases, and unions with Serde serialization where needed |
| HTTP clients | Async methods for API operations, with typed requests and responses |
| Axum servers | A service trait, request extractors, handlers, and routes |
| Body formats | JSON, text, and binary data, plus form and multipart requests |
| Authentication | API keys in headers, query parameters, and cookies, plus bearer tokens |
| Event streams | Server-Sent Events on both clients and servers |
| Validation | Checks for supported schema constraints, including lengths, ranges, patterns, and `multipleOf` |
| Builders | Optional `bon` builders for schema values and requests |
| Generation options | Operation filtering, name overrides, visibility, enum handling, and type adapters |

## Learn More

Read the [book](https://eklipse2k8.github.io/oas3-gen/) for client and server
walkthroughs, generation options, schema mappings, and known limitations.
Run `oas3-gen generate --help` for command-line usage.

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md) for contribution requirements and the
pull request process. Report bugs or suggest changes in
[GitHub issues](https://github.com/eklipse2k8/oas3-gen/issues).

## License

Licensed under the [MIT license](LICENSE.md).

Copyright (c) 2026 Individual contributors

[reqwest-client]: https://docs.rs/reqwest/0.13.5/reqwest/struct.Client.html
[axum-router]: https://docs.rs/axum/0.8.9/axum/struct.Router.html

See [OpenAPI v3.1.2]: <https://spec.openapis.org/oas/v3.1.2>

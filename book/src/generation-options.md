# Generation Options

Use generation options to choose the output layout, select operations, and control the
generated API's names and visibility. The [Flag Summary](#flag-summary) lists the
defaults and accepted values.

Commands run from a checkout of this repository. If you installed the tool, replace
`cargo run --` with `oas3-gen` . Use the path to your own document in place of
`spec.json` or `petstore.json` . Unless stated otherwise, examples refer to
`fixtures/petstore.json` ; Rust excerpts may omit imports, attributes, and unrelated
definitions.

- [Generation Modes](#generation-modes)
- [Workspace Crate Output](#workspace-crate-output)
- [Operation Filtering](#operation-filtering)
- [Schema Filtering](#schema-filtering)
- [API Name Override](#api-name-override)
- [Function Name Overrides](#function-name-overrides)
- [Visibility](#visibility)
- [Header Emission](#header-emission)
- [Builder Generation](#builder-generation)
- [Documentation Formatting](#documentation-formatting)
- [Known Limitations](#known-limitations)
- [Flag Summary](#flag-summary)

## Generation Modes

The positional `mode` argument selects the kind of output. Choose `types` when you need
data types, `client` or `client-mod` when you want to call an API, and `server-mod` when
you want to implement one:

```text
cargo run -- generate <MODE> -i spec.json -o <OUTPUT>
```

### `types`

The `types` mode writes type definitions to a single file. Let's generate the petstore's
types:

```bash
cargo run -- generate types -i petstore.json -o types.rs
```

Among others, `types.rs` now contains the `PetStatus` enum and the `Pet` struct:

```rust
/// Where the pet is in the adoption process
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, oas3_gen_support::Default)]
pub enum PetStatus {
    #[serde(rename = "available")]
    #[default]
    Available,
    #[serde(rename = "pending")]
    Pending,
    #[serde(rename = "adopted")]
    Adopted,
}

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
    /* ... */
}
```

The required properties `id` , `name` , and `species` become plain fields, and an
optional property such as `status` becomes an [`Option`][rustdoc-option] . The derives
follow how the API uses each type. `Pet` appears only in responses, so it derives
[`Deserialize`][rustdoc-serde-deserialize] but not
[`Serialize`][rustdoc-serde-serialize] . `NewPet` , which the API only accepts, derives
`Serialize` and [`Validate`][rustdoc-validate] instead. Server output reverses those
directions.

### `client`

The `client` mode writes types and a `reqwest` client to one file. See
[Choosing the Output Layout](./client-generation.md#choosing-the-output-layout) for an
example.

### `client-mod`

The `client-mod` mode writes `types.rs` , `client.rs` , and `mod.rs` to a directory. See
[Client Generation](./client-generation.md) for how to generate and use it.

### `server-mod`

The `server-mod` mode writes `types.rs` , `server.rs` , and `mod.rs` to a directory. See
[Server Generation](./server-generation.md) for how to implement the trait and use the
generated Axum router.

## Workspace Crate Output

Use `--workspace` when you want the output to have its own `Cargo.toml` and `src/`
directory. You can build that output as a crate or include it in a Cargo workspace. The
flag is available with `client-mod` and `server-mod` ; using it with `types` or `client`
produces a generation error.

```text
-w, --workspace
```

For example, let's generate a client crate in `petstore-api` :

```bash
cargo run -- generate client-mod -i petstore.json -o petstore-api --workspace
```

The generated directory has this structure:

```text
petstore-api/
├── Cargo.toml
└── src/
    ├── lib.rs
    ├── types.rs
    └── client.rs
```

The `src/lib.rs` file is the library root, so you can build `petstore-api` as a crate.
The type and client or server files contain the same code as the corresponding module
output.

### Crate Name

The package takes its name from the final component of the output directory, so
`-o petstore-api` produces `name = "petstore-api"` and a `petstore_api` library target.
Names are transliterated to ASCII and converted to kebab-case: `-o "output/Pet Store"`
yields `pet-store` , `-o café-api` yields `cafe-api` , and `-o petstore_v2` yields
`petstore-v2` . The library target of `petstore-v2` is still `petstore_v2` , because
Cargo maps dashes to underscores.

Generation fails when no valid name can be derived, for example when the directory name
is empty after sanitization or starts with a digit.

### Generated Manifest

The manifest declares the package and the dependencies its generated code uses. The
command above writes this manifest for the petstore client, shown here without its
banner; the dependency list for your API may differ:

```toml
[package]
name = "petstore-api"
version = "0.0.0"
edition = "2024"
rust-version = "1.89"
description = "Rust client generated from the Petstore OpenAPI document"

[dependencies]
anyhow = "1.0"
chrono = { version = "0.4.42", default-features = false, features = ["std", "clock", "serde"] }
http = "1.5"
indexmap = { version = "2.14", features = ["serde"] }
oas3-gen-support = "0.29.0"
regex = "1.13"
reqwest = { version = "0.13", default-features = false, features = ["form", "json", "multipart", "http2", "native-tls", "query", "stream"] }
reqwest_cookie_store = "0.10"
secrecy = "0.10"
serde = { version = "1.0", features = ["derive"] }
serde_html_form = { version = "0.4", default-features = false, features = ["ser", "std"] }
serde_json = { version = "1.0", features = ["preserve_order"] }
serde_with = { version = "3.24", features = ["base64", "chrono"] }
uuid = { version = "1.26", features = ["serde"] }
validator = { version = "0.21", features = ["derive", "indexmap"] }
```

The description comes from the document's `info.title` , `Petstore` . Adding
`--enable-builders` adds a `bon` entry. Generating the petstore server with `server-mod`
instead leaves out `reqwest` , `reqwest_cookie_store` , and `serde_html_form` , and adds
the Axum crates:

```toml
axum = { version = "0.8.5", features = ["multipart"] }
axum-extra = { version = "0.12", features = ["cookie", "query"] }
```

The generator rewrites `Cargo.toml` on every run and adds a banner identifying it as
generated output. Put code that needs its own dependencies in a separate crate so that
regenerating the API doesn't overwrite your changes.

The package version starts at `0.0.0` . Pass `--module-version` to set a version for the
generated crate:

```bash
cargo run -- generate client-mod -i petstore.json -o petstore-api --workspace --module-version 1.0.0
```

The `--module-version` flag requires `--workspace` , because that's the mode that
creates the manifest.

### Dependencies

Each generator release contains the dependency version requirements it writes to the
manifest. It doesn't read them from your project's environment. The requirement for
`oas3-gen-support` uses the generator's version. Cargo resolves these requirements when
you build the generated crate.

The manifest declares crates referenced by the generated source. For example, a
specification without date or time types doesn't need a `chrono` entry, and server
output doesn't need the client's `reqwest` dependency. The following table describes the
dependencies you may see:

| Crate | Declared when the generated code |
|-------|----------------------------------|
| `anyhow` | returns [`anyhow::Result`][rustdoc-anyhow-result] from client or server methods |
| `axum` | defines a `server-mod` router and extractors, with the `multipart` feature |
| `axum-extra` | reads query parameters or cookie API keys on the server, with the `cookie` and `query` features |
| `bon` | derives builders (`--enable-builders`) |
| `chrono` | maps `date`, `date-time`, or `time` formats |
| `futures` | returns or sends a server's `text/event-stream` as a [`BoxStream`][rustdoc-futures-boxstream] |
| `http` | emits header constants or [`HeaderMap`][rustdoc-http-header-map] conversions |
| `indexmap` | emits [`IndexMap`][rustdoc-indexmap-map]/[`IndexSet`][rustdoc-indexmap-set] collection types |
| `oas3-gen-support` | uses runtime derives, diagnostics, `multipleOf` checks, or reads event streams on the client |
| `regex` | emits `pattern` validation constants |
| `reqwest` | performs `client-mod` HTTP calls; the `form` feature sends `application/x-www-form-urlencoded` bodies |
| `reqwest_cookie_store` | sends cookie API keys or encodes cookie parameters from the client |
| `secrecy` | stores API keys and bearer tokens |
| `serde` | derives [`Serialize`][rustdoc-serde-serialize]/[`Deserialize`][rustdoc-serde-deserialize] |
| `serde_html_form` | serializes query parameters on the client, with only its `ser` and `std` features |
| `serde_json` | handles freeform [`serde_json::Value`][rustdoc-serde-json-value] payloads |
| `serde_with` | applies `serde_as` conversions |
| `uuid` | maps the `uuid` format |
| `validator` | derives [`Validate`][rustdoc-validate] |

`serde_json` gains the `preserve_order` feature under the default collection policy so
that freeform JSON objects keep their key order. With
[`--no-ordered-collections`](./schema-types.md#ordering-and-collections) the feature is
omitted. Likewise, `validator` gains its `indexmap` feature when the generated code also
uses `indexmap` . The feature checks the length of an `IndexSet` or `IndexMap` field,
such as the `maxItems` limit on the `tags` of `NewPet` .

### Using the Crate

The manifest declares a plain `[package]` with no `[workspace]` table, so the crate
builds standalone and can also be added to an existing workspace:

```toml
[workspace]
members = ["crates/*", "petstore-api"]
```

To use the generated crate from another crate, add a path dependency to that crate's
`Cargo.toml` :

```toml
[dependencies]
petstore-api = { path = "../petstore-api" }
```

Keep the default [`--visibility public`](#visibility) for a crate other code depends on.
`crate` and `file` visibility restrict the re-exports in `lib.rs` , so other crates
can't access those re-exported items.

## Operation Filtering

You may only need a few operations from a large API. Use `--only` to select those
operations, or `--exclude` to leave specific operations out. You can use one of these
flags at a time.

```text
--only <id1,id2,...>
--exclude <id1,id2,...>
```

### `--only`

Pass the normalized `snake_case` operation IDs, separated by commas. Filtering uses
these IDs before any `--fn-name` overrides. For example, the petstore's `listPets` and
`createPet` operations use `list_pets` and `create_pet` in the filter:

```bash
cargo run -- generate client-mod -i petstore.json -o output/ \
  --only list_pets,create_pet
```

The generated client keeps `list_pets()` and `create_pet()` and omits the other
operations. With `server-mod` , the same selection keeps those two trait methods and
their routes. Types needed by the selected methods are still included, as we'll see in
[Schema Dependency Resolution](#schema-dependency-resolution) .

### `--exclude`

Use `--exclude` when you want most of an API but don't need a particular operation. For
example, to leave out the image upload operation, run:

```bash
cargo run -- generate client-mod -i petstore.json -o output/ \
  --exclude upload_pet_image
```

The client keeps the petstore's other operations, including `list_pets()` and
`create_pet()` , and omits `upload_pet_image()` . In `server-mod` mode, the flag omits
that operation's trait method and route.

A callback follows the operation that declares it. The petstore's `adopt_pet` operation
declares the `statusChanged` callback, which `server-mod` generates as
`adopt_pet_status_changed` . In `server-mod` , `--only adopt_pet` keeps that callback's
route and trait method, and `--exclude adopt_pet` removes them.

You can also name a callback by its own normalized `snake_case` ID.
`--only adopt_pet_status_changed` keeps only the callback, and
`--exclude adopt_pet_status_changed` removes only the callback while `adopt_pet` stays.

### Schema Dependency Resolution

Filtering an operation also filters the types needed to use it. The generator follows
dependencies through the schemas so that a selected operation has the types it needs:

1. It collects schemas used by the selected operations' parameters, request
   bodies, and responses.
2. It follows references from those schemas to other schemas.
3. It generates the resulting set of types.

For example, the petstore's `listPets` returns `Pets` , an array of `Pet` values, and an
`Error` for its `default` response. Selecting only `list_pets` also generates the
schemas `Pet` refers to: `Species` , `PetStatus` , the `Health` union with its
`Allergies` and `Diet` branches, `PetVaccinations` , and `VaccineRecord` . The operation
doesn't refer to any of them directly.

## Schema Filtering

By default, the generator includes schemas used by the selected operations and the
schemas they depend on. Pass `--all-schemas` when you also need types that no operation
references.

```text
--all-schemas
```

### Default (Reachability Filtering)

This example uses a different document. Suppose a specification defines `Pet` ,
`Category` , `Store` , and `Inventory` , but its operations only use `Pet` and
`Category` . Generate types with the default options:

```bash
cargo run -- generate types -i spec.json -o types.rs
```

The output contains `Pet` and `Category` . The generator skips `Store` and `Inventory`
because no selected operation needs them. With `--verbose` , it reports how many of
these orphaned schemas it skipped.

### With `--all-schemas`

To include those unused schemas too, add the flag:

```bash
cargo run -- generate types -i spec.json -o types.rs --all-schemas
```

The output now contains all four types: `Pet` , `Category` , `Store` , and `Inventory` .

### Combining with Operation Filtering

You can't combine `--all-schemas` with `--only` or `--exclude` in one command. If you
need both a complete set of types and a filtered client, generate two outputs:

```bash
cargo run -- generate types -i spec.json -o types.rs --all-schemas
cargo run -- generate client -i spec.json -o client.rs --only list_pets
```

The first file contains all schemas. The second contains the filtered client and its own
required types; these commands don't make the client import types from the first file.

## API Name Override

Use `--api-name` to name the client struct in `client` or `client-mod` mode, or the
server trait in `server-mod` mode. The generator converts the supplied name to
`PascalCase` .

```text
--api-name <NAME>
```

By default, the client name comes from the specification's `info.title` . For example,
the petstore's title, `"Petstore"` , becomes `PetstoreClient` ; an empty title produces
`ApiClient` . The default server trait name is `ApiServer` . To choose `PetStoreClient`
for the client, run:

```bash
cargo run -- generate client-mod -i petstore.json -o output/ --api-name PetStoreClient
```

The client struct and its implementation now use `PetStoreClient` . In server mode,
choose a trait name with the same flag:

```bash
cargo run -- generate server-mod -i petstore.json -o output/ --api-name PetStoreApi
```

The server trait is named `PetStoreApi` , and the generated router uses that name in its
trait bound. Changing the API name doesn't rename the operations; use
[`--fn-name`](#function-name-overrides) for those.

## Function Name Overrides

Use `--fn-name` to choose the generated client or server method name for an operation.
The key is its `operationId` from the specification; the generator also accepts the
normalized operation ID shown by `oas3-gen list operations` .

```text
--fn-name <ID=NAME>
```

The generator converts your method name to `snake_case` and uses it to derive the
request type's name. For example, renaming `listPets` to `fetch_all_pets` also produces
`FetchAllPetsRequest` . Repeat the flag to rename more than one operation:

```bash
cargo run -- generate client-mod -i petstore.json -o output/ \
  --fn-name listPets=fetch_all_pets \
  --fn-name showPetById=get_pet
```

The override changes both names together:

| Operation ID | Method Name | Request Type |
|---|---|---|
| `listPets` | `fetch_all_pets` | `FetchAllPetsRequest` |
| `showPetById` | `get_pet` | `GetPetRequest` |

Use the new names when calling the client or implementing the server trait.

## Visibility

Use `--visibility` to choose where generated items can be accessed. The default,
`public` , makes them available to other crates. Choose `crate` when the generated API
is an internal part of your application.

```text
-C, --visibility <LEVEL>
```

| Value | Modifier | Use Case |
|-------|----------|----------|
| `public` (default) | `pub` | Library distribution |
| `crate` | `pub(crate)` | Internal crate types |
| `file` | `pub(super)` | Items used only inside the generated module |

Let's compare the three values on two petstore types: the `Pet` struct and the
`ChatModelChoice` union, which has [helper methods](./enum-types.md#helper-methods) .

### Example: `--visibility public`

With public visibility, the types, fields, and methods use `pub` :

```rust
pub struct Pet {
    pub id: uuid::Uuid,
    pub name: String,
    /* ... */
}

pub enum ChatModelChoice {
    ChatModel(ChatModel),
    String(String),
}

impl ChatModelChoice {
    pub fn chat10() -> Self {
        Self::ChatModel(ChatModel::Chat10)
    }
    /* ... */
}
```

### Example: `--visibility crate`

With crate visibility, those items use `pub(crate)` :

```rust
pub(crate) struct Pet {
    pub(crate) id: uuid::Uuid,
    pub(crate) name: String,
    /* ... */
}

pub(crate) enum ChatModelChoice {
    ChatModel(ChatModel),
    String(String),
}

impl ChatModelChoice {
    pub(crate) fn chat10() -> Self {
        Self::ChatModel(ChatModel::Chat10)
    }
    /* ... */
}
```

### Example: `--visibility file`

With file visibility, those items use `pub(super)` :

```rust
pub(super) struct Pet {
    pub(super) id: uuid::Uuid,
    pub(super) name: String,
    /* ... */
}

pub(super) enum ChatModelChoice {
    ChatModel(ChatModel),
    String(String),
}

impl ChatModelChoice {
    pub(super) fn chat10() -> Self {
        Self::ChatModel(ChatModel::Chat10)
    }
    /* ... */
}
```

In module output, `pub(super)` lets the generated files use each other's items:
`client.rs` or `server.rs` can reach the types in `types.rs` . The module's `mod.rs` or
`lib.rs` doesn't re-export anything, so the items stay inside the generated module.

## Header Emission

Header name constants use
[`http::HeaderName`][rustdoc-http-header-name], so you can refer to headers without repeating their string names. By default, the generator creates constants for request parameters and [response headers](./http.md#response-headers)
used by the selected operations. Use `--all-headers` to include header parameters from
`components/parameters` even when no operation references them.

Headers that the `http` crate already names are the exception. The
[`http::header`][rustdoc-http-header-constants] module declares constants for standard
headers such as `Content-Type` , `Location` , and `Authorization` . The generator
doesn't declare its own constant for any of them; generated code refers to `http` 's
constant, such as `http::header::LOCATION` , instead.

OpenAPI says to ignore header parameters named `Accept` , `Content-Type` , or
`Authorization` , so the generator leaves them out of request types. The specification
describes those headers elsewhere: media types cover `Accept` and `Content-Type` , and
security schemes cover `Authorization` . For example, the petstore's `listAuditEvents`
declares a required `Authorization` header parameter, yet its request struct has no
header field. To send credentials in `Authorization` , declare a
[security scheme](./http.md#security-schemes) , as that operation also does.

```text
--all-headers
```

### Default (Operation-Referenced Headers Only)

The petstore's `showPetById` operation uses an `x-api-version` header. The document also
declares an `x-debug-trace` header parameter in `components/parameters` that no
operation uses. Start with the default options:

```bash
cargo run -- generate types -i petstore.json -o types.rs
```

Among the other header constants, the output contains one for the header the operation
uses:

```rust
pub const X_API_VERSION: http::HeaderName = http::HeaderName::from_static(
    "x-api-version",
);
```

`x-debug-trace` isn't emitted because no operation uses it.

### With `--all-headers`

Now include the unused component header:

```bash
cargo run -- generate types -i petstore.json -o types.rs --all-headers
```

The output gains a constant for it:

```rust
pub const X_DEBUG_TRACE: http::HeaderName = http::HeaderName::from_static(
    "x-debug-trace",
);
```

## Builder Generation

Use `--enable-builders` when you want to construct generated values through setter
methods. The generator adds [`bon::Builder`][rustdoc-bon-builder] derives to schema
structs and `#[builder]` constructors to request structs.

```text
--enable-builders
```

Builders are disabled by default. The [Builder Pattern](./builders.md) chapter walks
through using them and explains how request builders run validation.

### Default (Builders Disabled)

Without the flag, generated structs have no `bon` attributes. This excerpt from the
petstore client shows a schema type and a request type:

```rust
/// A pet in the store's care.
#[derive(Debug, Clone, PartialEq, Deserialize, oas3_gen_support::Default)]
pub struct Pet {
    /// Server-assigned identifier
    pub id: uuid::Uuid,
    pub name: String,
    /* ... */
}

/// Get a pet
#[derive(Debug, Clone, validator::Validate, oas3_gen_support::Default)]
pub struct ShowPetByIdRequest {
    pub path: ShowPetByIdRequestPath,
    #[validate(nested)]
    pub header: ShowPetByIdRequestHeader,
}
```

### With `--enable-builders`

With the flag, the schema gets a derive and the request gets a constructor annotated for
`bon` :

```rust
/// A pet in the store's care.
#[derive(Debug, Clone, PartialEq, Deserialize, oas3_gen_support::Default, bon::Builder)]
pub struct Pet {
    /// Server-assigned identifier
    pub id: uuid::Uuid,
    pub name: String,
    /* ... */
}

/// Get a pet
#[derive(Debug, Clone, validator::Validate, oas3_gen_support::Default)]
pub struct ShowPetByIdRequest {
    pub path: ShowPetByIdRequestPath,
    #[validate(nested)]
    pub header: ShowPetByIdRequestHeader,
}

#[bon::bon]
impl ShowPetByIdRequest {
    /// Create a new request with the given parameters.
    #[builder]
    pub fn new(
        pet_id: uuid::Uuid,
        if_none_match: Option<String>,
        x_api_version: String,
    ) -> anyhow::Result<Self> {
        /* ... */
    }
}
```

The constructor accepts each parameter separately, so the builder has a setter for
`pet_id` , `if_none_match` , and `x_api_version` . It assembles the nested `path` and
`header` structs and validates the request before returning it.

## Raw Response Bodies

Use `--raw-body` when a client downloads bodies you don't want to hold in memory all at
once, such as large files. By default, the client reads each binary response body into a
[`Vec<u8>`][rustdoc-vec] before it returns. With the flag, the client returns the body
unread as a [`reqwest::Body`][rustdoc-reqwest-body], and your code reads it as it arrives.

```text
--raw-body
```

The flag applies to every response body the client would otherwise read into a
`Vec<u8>`: binary media types such as `image/png`, and text media types whose schema has
`format: binary`. It doesn't change JSON, text, or event-stream responses, request
bodies, or `server-mod` output.

### Default (Bytes)

Without the flag, `get_pet_photo()` in the petstore client returns the `image/png` photo
as bytes:

```rust
pub async fn get_pet_photo(
    &self,
    request: GetPetPhotoRequest,
) -> anyhow::Result<ApiResponse<Vec<u8>, Problem>> {
    /* ... */
}
```

### With `--raw-body`

With the flag, the same method returns the body unread:

```rust
pub async fn get_pet_photo(
    &self,
    request: GetPetPhotoRequest,
) -> anyhow::Result<ApiResponse<reqwest::Body, Problem>> {
    /* ... */
}
```

`reqwest::Body` implements [`http_body::Body`][rustdoc-http-body], so its `size_hint()`
reports the body's length when the response states it up front.
[`BodyDataStream`][rustdoc-http-body-util-body-data-stream] turns the body into a stream
of chunks. This example writes the photo to a file one chunk at a time, using the
`futures`, `http-body`, `http-body-util`, and `tokio` crates from your own
`Cargo.toml`:

```rust
use futures::StreamExt as _;
use http_body::Body as _;
use tokio::io::AsyncWriteExt as _;

if let ApiResponse::Ok(body) = response {
    println!("Photo size: {:?}", body.size_hint().exact());
    let mut file = tokio::fs::File::create("photo.png").await?;
    let mut chunks = http_body_util::BodyDataStream::new(body);
    while let Some(chunk) = chunks.next().await {
        file.write_all(&chunk?).await?;
    }
}
```

The size is `None` when the length isn't known before the body arrives, as with a
chunked response. `BodyDataStream` skips trailers, the header fields a server can send
after the body; [`BodyStream`][rustdoc-http-body-util-body-stream] yields them as frames
alongside the data.

A payload union that holds a raw body, such as `PetOrReqwestBody`, derives `Debug` but
not `Clone`, because a `reqwest::Body` can't be cloned.

## Documentation Formatting

Descriptions and summaries in the OpenAPI document become documentation comments in the
generated code. Use `--doc-format` to format longer comments with the external
[mdformat](https://github.com/executablebooks/mdformat) tool. The generator sends text
longer than 100 bytes to `mdformat` with a line-wrap setting of 100 characters.

```text
--doc-format
```

Install `mdformat` and make sure it's available on your `PATH` before using the flag:

```bash
pip install mdformat
```

### Default (Formatting Disabled)

Without the flag, the generator preserves documentation text while converting escaped
newlines to line breaks. A long description can therefore remain on one line, as the
petstore's `CollarTelemetry` description does:

```rust
/// A batch of smart-collar readings, written with JSON Schema 2020-12 keywords. Keywords without a Rust equivalent are accepted and ignored.
pub struct CollarTelemetry {
    /* ... */
}
```

### With `--doc-format`

```bash
cargo run -- generate types -i petstore.json -o types.rs --doc-format
```

The same description is wrapped into multiple lines:

```rust
/// A batch of smart-collar readings, written with JSON Schema 2020-12 keywords. Keywords without a Rust
/// equivalent are accepted and ignored.
pub struct CollarTelemetry {
    /* ... */
}
```

## Known Limitations

The following constraints and serialization options aren't fully represented in the
generated code. The petstore examples show where each limitation applies:

- With `--no-ordered-collections`, an array with `uniqueItems: true`, such as
  the `tags` of `Pet`, is a `Vec` and accepts repeated items. See
  [Ordering and Collections](./schema-types.md#ordering-and-collections).
- An optional field that also allows `null` is an `Option<T>`, so JSON `null`
  and a missing property both decode as `None`, and `None` is left out when
  serializing. A client therefore can't send `null` to clear a field with a
  JSON merge patch, such as the `nickname` in the `PetPatch` body of
  `updatePet`. Likewise, a field without a type, such as `anyValue` in
  `CollarTelemetry`, decodes `null` as `None`.
- The generator ignores `allowReserved` on query parameters. The client
  percent-encodes reserved characters, such as the `/` in a `q` value for
  `searchPets`.
- A query parameter's value must be a scalar or an array of scalars. A JSON
  object or an array of arrays in an untyped parameter, such as `filter` on
  `searchPets`, makes the client method return an error before it sends the
  request.
- An `anyOf` whose branches each require a different property isn't enforced.
  The `PetCarrier` schema requires either `wheels` or `straps`, but both become
  optional fields, so a carrier with neither decodes successfully.
- A multipart part name can't contain `"`, `\`, a carriage return, a line
  feed, or another control character. The generator warns about quotes and line
  breaks, as described in [Generation Warnings](./http.md#generation-warnings).
- A `multipleOf` that the generator can't check exactly only appears in the
  field's documentation. See [Multiples](./schema-types.md#multiples).

## Flag Summary

This table collects the options used across the generation chapters. Defaults apply when
you omit the corresponding option. Run `oas3-gen generate --help` to see the
command-line help, including input, output, and terminal options.

| Flag | Default | Description |
|------|---------|-------------|
| `mode` | `types` | Generation mode: `types`, `client`, `client-mod`, `server-mod` |
| `-w, --workspace` | `false` | Emit a workspace-compatible `Cargo.toml` with sources in `src/`; requires `client-mod` or `server-mod` |
| `--module-version` | `0.0.0` | Version written to the generated `Cargo.toml` `[package]` table; requires `--workspace` |
| `-C, --visibility` | `public` | Item visibility: `public`, `crate`, `file` |
| `--enum-mode` | `merge` | Enum duplicate handling: `merge`, `preserve`, `relaxed` |
| `--enum-layout` | `spec` | Variant ordering: `spec`, `sorted` |
| `--no-helpers` | `false` | Disable enum constructor helpers |
| `--odata-support` | `false` | Make `@odata.*` fields optional |
| `-c, --customize` | *(none)* | Custom `serde_with` adapter; repeatable |
| `--fn-name` | *(none)* | Custom function name per operation (`ID=NAME`); repeatable |
| `--api-name` | *(none)* | Name for the generated client struct or server trait |
| `--all-headers` | `false` | Emit header constants for all component-level headers |
| `--enable-builders` | `false` | Enable `bon` builder derives and methods |
| `--raw-body` | `false` | Return client binary response bodies as an unread `reqwest::Body` instead of `Vec<u8>` |
| `--no-ordered-collections` | `false` | Emit [`HashMap`][rustdoc-hashmap]/[`Vec`][rustdoc-vec] instead of `indexmap` collection types |
| `--doc-format` | `false` | Format documentation comments with `mdformat` |
| `--only` | *(none)* | Include only specified operations |
| `--exclude` | *(none)* | Exclude specified operations |
| `--all-schemas` | `false` | Generate all schemas regardless of usage |

[rustdoc-anyhow-result]: https://docs.rs/anyhow/1.0.104/anyhow/type.Result.html
[rustdoc-bon-builder]: https://docs.rs/bon/3.10.1/bon/derive.Builder.html
[rustdoc-futures-boxstream]: https://docs.rs/futures/0.3.34/futures/stream/type.BoxStream.html
[rustdoc-hashmap]: https://doc.rust-lang.org/std/collections/struct.HashMap.html
[rustdoc-http-header-constants]: https://docs.rs/http/1.5.0/http/header/index.html#constants
[rustdoc-http-header-map]: https://docs.rs/http/1.5.0/http/header/struct.HeaderMap.html
[rustdoc-http-body]: https://docs.rs/http-body/1.1.0/http_body/trait.Body.html
[rustdoc-http-body-util-body-data-stream]: https://docs.rs/http-body-util/0.1.5/http_body_util/struct.BodyDataStream.html
[rustdoc-http-body-util-body-stream]: https://docs.rs/http-body-util/0.1.5/http_body_util/struct.BodyStream.html
[rustdoc-http-header-name]: https://docs.rs/http/1.5.0/http/header/struct.HeaderName.html
[rustdoc-indexmap-map]: https://docs.rs/indexmap/2.14.2/indexmap/map/struct.IndexMap.html
[rustdoc-indexmap-set]: https://docs.rs/indexmap/2.14.2/indexmap/set/struct.IndexSet.html
[rustdoc-option]: https://doc.rust-lang.org/std/option/enum.Option.html
[rustdoc-reqwest-body]: https://docs.rs/reqwest/0.13.5/reqwest/struct.Body.html
[rustdoc-serde-deserialize]: https://docs.rs/serde/1.0.229/serde/trait.Deserialize.html
[rustdoc-serde-json-value]: https://docs.rs/serde_json/1.0.151/serde_json/enum.Value.html
[rustdoc-serde-serialize]: https://docs.rs/serde/1.0.229/serde/trait.Serialize.html
[rustdoc-validate]: https://docs.rs/validator/0.21.0/validator/trait.Validate.html
[rustdoc-vec]: https://doc.rust-lang.org/std/vec/struct.Vec.html

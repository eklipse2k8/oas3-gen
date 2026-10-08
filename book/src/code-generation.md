# Shared Generation Options

<a id="code-generation"></a>

Client and server generation use the same schemas and many of the same options.
This chapter explains those shared rules: choosing output modes, representing
responses, interpreting security requirements, and changing the generated types.
Use the [Flag Summary](#flag-summary) when you need a reminder of an option.

For a walkthrough of calling an API, start with
[Client Generation](./client-generation.md). To implement an API with Axum,
start with [Server Generation](./server-generation.md). Both chapters link back
here when an option applies to both targets.

The commands use `cargo run --` from a checkout of this repository. If you
installed the tool with Cargo, replace `cargo run --` with `oas3-gen`. Input
names such as `spec.json` and `petstore.json` refer to OpenAPI documents in your
current directory; substitute the path to your own document. You can find the
petstore example in `crates/oas3-gen/fixtures/petstore.json`.

The Rust examples are excerpts that focus on the option being discussed. They
omit imports, unrelated attributes, and some definitions. An ellipsis marks
omitted code, so those excerpts aren't complete programs.

## Table of Contents

- [Generation Modes](#generation-modes)
- [Responses](#responses)
- [Multipart Form Bodies](#multipart-form-bodies)
- [Security Schemes](#security-schemes)
- [Workspace Crate Output](#workspace-crate-output)
- [Generated Names](#generated-names)
- [Visibility](#visibility)
- [Enum Mode](#enum-mode)
- [Enum Layout](#enum-layout)
- [Numeric-Backed Enums](#numeric-backed-enums)
- [Helper Methods](#helper-methods)
- [OData Support](#odata-support)
- [Type Customization](#type-customization)
- [Operation Filtering](#operation-filtering)
- [Function Name Overrides](#function-name-overrides)
- [API Name Override](#api-name-override)
- [Schema Filtering](#schema-filtering)
- [Header Emission](#header-emission)
- [Builder Generation](#builder-generation)
- [Ordering and Collections](#ordering-and-collections)
- [Documentation Formatting](#documentation-formatting)
- [Flag Summary](#flag-summary)

## Generation Modes

The positional `mode` argument selects the kind of output. Choose `types`
when you need data types, `client` or `client-mod` when you want to call an API,
and `server-mod` when you want to implement one:

```text
cargo run -- generate <MODE> -i spec.json -o <OUTPUT>
```

### `types`

The `types` mode writes type definitions to a single file. For example, a
schema describing pets and their status might produce these types in `types.rs`:

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Pet {
    pub id: i64,
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Status {
    #[serde(rename = "available")]
    Available,
    #[serde(rename = "pending")]
    Pending,
}
```

### `client`

The `client` mode writes types and a `reqwest` client to one file. See
[Choosing the Output Layout](./client-generation.md#choosing-the-output-layout)
for an example.

### `client-mod`

The `client-mod` mode writes `types.rs`, `client.rs`, and `mod.rs` to a directory.
See [Client Generation](./client-generation.md) for how to generate and use it.

### `server-mod`

The `server-mod` mode writes `types.rs`, `server.rs`, and `mod.rs` to a directory.
See [Server Generation](./server-generation.md) for how to implement the trait
and use the generated Axum router.

## Responses

Generated operations share a response enum named `ApiResponse`. Its variants
represent the status codes declared by the selected operations. When both
success and failure payloads are needed, the enum has the form
`ApiResponse<Value, Failure>`. In module output, you'll find it in `types.rs`.

The generator also adds an `Other` variant when an operation has no `default`
response. That variant lets the client return an undeclared status and its raw
body. The following table shows the possible variant shapes:

| Variant shape | When |
|---|---|
| `Ok(Value)`, `Created(Value)`, ... | A 2xx status that carries a body in at least one operation |
| `NotFound(Failure)`, `Conflict(Failure)`, ... | Any other declared status that carries a body |
| `NoContent`, `NotModified`, ... | A status that has no body, and no [headers](#response-headers) in its class, in any operation |
| `ClientError(http::StatusCode, Failure)` | A range such as `4XX`; the concrete status travels with the body |
| `Unknown(http::StatusCode, Failure)` | The spec's `default` response, when it declares a body |
| `Other(http::StatusCode, Vec<u8>)` | A status the operation does not declare when it has no `default` response |

The variants that carry a status use [`http::StatusCode`][rustdoc-http-status]. `Other` holds its raw
body in a [`Vec<u8>`][rustdoc-vec].

Each operation supplies its own types for `Value` and `Failure`. We'll call
these the two *response classes*: `Value` covers 2xx statuses, and `Failure`
covers the remaining statuses. Before accounting for headers, the body types
determine each parameter as follows:

| Bodies declared in the class | Parameter |
|---|---|
| None | [`()`][rustdoc-unit] |
| One type | That type |
| One type plus a status that carries a payload but no body | [`Option<Type>`][rustdoc-option] |
| Several types | An untagged payload union such as `BasicErrorOrValidationError` |
| Several types plus a status that carries a payload but no body | `Option<Union>` |

A payload union is an enum that can hold one of several body types. Operations
that declare the same set of body types share that union definition. If no
status uses one of the response enum's type parameters, the generator omits
that parameter. If a schema already uses the name `ApiResponse`, the generator
names the response enum `ApiResponseType`.

### Response Headers

Some operations return headers as well as a body. When any status in a
response class declares headers, the class uses `WithHeaders<Headers, Body>`
as its parameter. `Headers` is a generated struct with one field for each
header declared across that class. `Body` is the type chosen using the table
above.

The wrapper keeps the headers and body together. The generator adds it beside
the response enum only when at least one response declares headers. If a
schema already uses the name `WithHeaders`, the generated name gets a numeric
suffix:

```rust
/// A response body together with the headers its response declares.
#[derive(Debug, Clone)]
pub struct WithHeaders<Headers, Body> {
    pub headers: Headers,
    pub body: Body,
}
```

A header field is optional unless every status in its class marks it as
required. If an optional header is missing or can't be parsed, its value is
`None`. For a list of values, one invalid element makes the whole list `None`.
A missing or invalid required response header causes `parse_response` to
return an error.

Operations with the same header fields share a header struct. The generator
names that struct after its headers, as in `XNextHeaders` or
`LinkAndLocationHeaders`, and adds a numeric suffix if the name is already in
use. A status without a body can still carry headers: its variant then holds
a `WithHeaders` value whose body is [`()`][rustdoc-unit] or an [`Option`][rustdoc-option].

The generator ignores `Content-Type` entries in response header definitions.
A header without a schema uses [`String`][rustdoc-string]. Enums used in headers implement
[`FromStr`][rustdoc-from-str] for both client and server output, and response headers receive name
constants in the same way as request headers.

For example, the petstore response enum and pagination headers include these
definitions:

```rust
/// Response shared by every operation.
#[derive(Debug, Clone)]
pub enum ApiResponse<Value, Failure> {
    /// 200
    Ok(Value),
    /// 201
    Created(Value),
    /// default
    Unknown(http::StatusCode, Failure),
}

/// Response headers that share one status class in an operation.
#[derive(Debug, Clone, PartialEq, oas3_gen_support::Default)]
pub struct XNextHeaders {
    /// A link to the next page of responses.
    pub x_next: Option<String>,
}
```

An operation returning pets with pagination headers uses
`ApiResponse<WithHeaders<XNextHeaders, Pets>, Error>`. The client decodes that
value from an HTTP response, and the server constructs it to send a response.
See [Reading Responses](./client-generation.md#reading-responses) and
[Returning Responses](./server-generation.md#returning-responses) for examples.

## Multipart Form Bodies

When an operation's request body uses `multipart/form-data`, the body schema
still becomes an ordinary struct in `types.rs`. The difference is on the wire:
the generated client sends each property as its own part, named after the
property, and the generated server reads those parts back into the struct.
This section explains which part each property becomes, so you can predict
what a server receives.

Let's start with a body that uploads a file next to a few text fields. This is
the `UploadForm` schema from `crates/oas3-gen/fixtures/multipart.json`, trimmed
to four of its properties:

```json
"UploadForm": {
  "type": "object",
  "required": ["name", "attachments"],
  "properties": {
    "name": { "type": "string" },
    "file": { "anyOf": [{ "type": "string", "format": "binary" }, { "type": "null" }] },
    "attachments": { "type": "array", "items": { "type": "string", "format": "binary" } },
    "quality": { "$ref": "#/components/schemas/Quality" }
  }
}
```

The generator produces a struct with `name: String`, `file: Option<Vec<u8>>`,
`attachments: Vec<Vec<u8>>`, and `quality: Option<Quality>`. The rest of this
section describes the parts those fields become. See
[Sending Multipart Bodies](./client-generation.md#sending-multipart-bodies) and
[Receiving Multipart Bodies](./server-generation.md#receiving-multipart-bodies)
for the code that writes and reads them.

### Choosing the Part for Each Property

The following table shows how the generator encodes each kind of property when
the media type has no `encoding` entry for it:

| Property schema | Part sent |
|---|---|
| `type: string` with `format: binary` | A file part labelled `application/octet-stream`, carrying the raw bytes |
| No `type` at all, such as `{}` | A file part, because OpenAPI 3.1 reads an untyped multipart property as raw binary |
| `type: string`, or a string `enum` | A text part holding the string |
| `type: string` with `format: byte` | A text part holding the base64 text |
| `type: integer`, `number`, `boolean`, or an integer `enum` | A text part holding the value, such as `3` or `true` |
| `format: date`, `date-time`, `time`, or `uuid` | A text part holding the serialized value, such as `2026-10-07T16:42:52Z` |
| `type: object`, a `$ref` to one, or a discriminated `oneOf` | An `application/json` part |
| An untagged `anyOf` or `oneOf`, or a schema such as `additionalProperties: true` | A text part when the value is a string, and an `application/json` part otherwise |
| `type: array` | One part per item, each encoded by the rules above |
| `additionalProperties` | One part per entry, named by its key |

An untyped property is generated as [`Vec<u8>`][rustdoc-vec] in a multipart body,
and `items: {}` produces `Vec<Vec<u8>>`. That's how you upload any number of
files under one name, as in the specification's own example. The generator
makes this change only when the schema is used just as a multipart body. If
the same schema is also a JSON response or another schema's property, the field
stays [`serde_json::Value`][rustdoc-serde-json-value], so its JSON form doesn't
change, and the generator warns.

A value whose type depends on the data, such as an untagged union, goes as
JSON unless it's a string. That way a server can tell the string `"5"` from the
number `5`.

### Encoding Objects

A media type's `encoding` map adjusts individual properties. Its
`contentType` field labels a property's parts:

- When the field lists several media types, such as `image/png, image/jpeg`,
  the generator uses the first one, because a part can carry only one. It warns
  when it labels every file with the first type of a list.
- A wildcard such as `image/*` describes a range rather than a type a client
  can send, so the generator skips it, falls back to the default, and warns.
- A JSON media type serializes the value as JSON. A string property with
  `contentType: application/json` is therefore sent as `"hello"`, quotes
  included.
- Objects are always serialized as JSON. When `contentType` names a non-JSON
  type for an object, such as `application/xml`, the part keeps
  `application/json`, and the generator warns.
- Rust strings are UTF-8, so a `charset` other than UTF-8 on a text part
  becomes `charset=utf-8`, with a warning.

The `style`, `explode`, and `allowReserved` fields switch a property to the
serialization used for query parameters. When any of them is present,
`contentType` is ignored for text values, and values aren't percent-encoded.
File properties keep their file parts. The following table applies to struct
and map properties:

| Property | Encoding | Parts sent |
|---|---|---|
| Array | `explode: true` (the default for `form`) | One part per item |
| Array | `explode: false` | One part, with items joined by `,`, a space, or `\|` for `form`, `spaceDelimited`, or `pipeDelimited` |
| Object | `style: deepObject` | One part per member, named `property[member]` |
| Object | `style: form`, `explode: true` | One part per member, named by the member |
| Object | `style: form`, `explode: false` | One part of alternating names and values, such as `min,1,max,5` |

This serialization has no way to escape its delimiters. An item that contains
the delimiter, such as `a,b` in a comma-joined array, arrives as two items. A
`style` value that `multipart/form-data` doesn't support, such as `matrix`,
falls back to `form` with a warning.

An Encoding Object can also declare `headers` for a part. The generated client
doesn't send part headers, and the generated server doesn't expose them. Part
headers are optional unless a header sets `required: true`, so the generator
warns about each required one.

### Generation Warnings

The generator also warns about specifications it can't follow exactly:

- An `encoding` entry names a property the body schema doesn't have.
- The body schema isn't an object with properties, such as a top-level
  `oneOf`. Its parts come from its JSON form, so binary values aren't sent as
  files.
- The media type is another `multipart` subtype, such as `multipart/mixed`.
  The client sends `multipart/form-data`.
- A property name contains a double quote or a line break, which a part header
  can't carry.
- An array holds nested arrays, whose inner arrays are sent as JSON text.

The parser the generator uses doesn't keep a schema's `contentEncoding`, so a
`type: string` property with `contentEncoding: base64` is sent as a plain text
part rather than labelled `application/octet-stream`.

## Security Schemes

OpenAPI describes authentication in two places. A *security scheme* under
`components.securitySchemes` describes a credential and how it travels in a
request. A *security requirement* in `security` names the schemes an operation
accepts. The generator supports `apiKey` schemes in headers, query parameters,
and cookies, and `http` schemes that use bearer tokens.

Let's start with a header key. This excerpt from an OpenAPI document declares
a scheme named `ApiKeyAuth` and uses it as the default requirement:

```json
{
  "security": [{ "ApiKeyAuth": [] }],
  "components": {
    "securitySchemes": {
      "ApiKeyAuth": { "type": "apiKey", "in": "header", "name": "X-Api-Key" }
    }
  }
}
```

The scheme name, `ApiKeyAuth`, identifies the credential in generated Rust
names. The `name` value, `X-Api-Key`, identifies the HTTP header. Operations
inherit the top-level `security` unless they declare their own requirements.

A bearer scheme is an `http` scheme whose `scheme` value is `bearer`. It has no
`name`, because the token always travels in the `Authorization` header as
`Bearer <token>`:

```json
{
  "components": {
    "securitySchemes": {
      "BearerAuth": { "type": "http", "scheme": "bearer" }
    }
  }
}
```

The generator matches `bearer` in any case, as HTTP does. A `bearerFormat`
value, such as `JWT`, only documents the token, so it doesn't change the
generated code. A header parameter named `Authorization` doesn't produce
credentials; OpenAPI says to ignore it, as [Header Emission](#header-emission)
explains.

### Requirements

The entries in the `security` array are alternatives: a request needs to
satisfy one entry. Within an entry, all the named schemes are required. These
examples show how the grouping changes the requirement:

| Operation's `security` | Requirement |
|---|---|
| `[{"ApiKeyAuth": []}]` | Supply the header key. |
| `[{"QueryKey": []}, {"SessionCookie": []}]` | Supply a query key or a session cookie. |
| `[{"ApiKeyAuth": [], "SessionCookie": []}]` | Supply both the header key and the session cookie. |
| `[{"BearerAuth": []}, {"ApiKeyAuth": []}]` | Supply a bearer token or the header key. |
| `[]` or `[{}]` | Allow anonymous access, replacing any top-level requirement. |

The middle examples assume `QueryKey` and `SessionCookie` are also declared as
`apiKey` schemes, and that `BearerAuth` is the bearer scheme shown earlier. The
complete example is in `crates/oas3-gen/fixtures/api_key_security.json`.

The server represents a credential as required when every alternative needs
it. Otherwise, the field is optional, and the extractor checks that the
supplied credentials complete at least one alternative. These checks establish
that credentials are present. Your service still needs to decide whether their
values authorize the request.

Other schemes, such as HTTP basic authentication and OAuth 2.0, don't produce
generated credentials. The generator reports a warning for each referenced
scheme it can't handle. If an operation has an alternative containing one of
these schemes, all its credential fields become optional, including those in
that same alternative. The generated extractor can't enforce the full
requirement, so your application must handle authorization for that operation.

### Keeping Keys Out of Logs

The server stores API keys and bearer tokens as
[`secrecy::SecretString`][rustdoc-secret-string].
The client uses the same type for header keys, query keys, and bearer tokens. Its debug representation
shows `[REDACTED]` in place of the value, and it clears its own stored value when
dropped. It doesn't implement [`Display`][rustdoc-display], [`Serialize`][rustdoc-serde-serialize], or [`PartialEq`][rustdoc-partial-eq].

Client cookie keys live in a cookie store instead. The generated client's
[`Debug`][rustdoc-debug] implementation leaves that store out. To read a `SecretString`, call
`expose_secret()` with [`secrecy::ExposeSecret`][rustdoc-expose-secret] in scope; the
[server example](./server-generation.md#reading-and-checking-keys) shows how.
Once exposed, the returned string is ordinary text, so keep it out of log output.

### Client

The client provides methods to set header, query, and cookie keys and bearer
tokens. See [Sending Credentials](./client-generation.md#sending-credentials) for examples and
[Cookie Keys](./client-generation.md#cookie-keys) for the cookie store's behavior.

### Server

Server request structs carry credentials extracted from the incoming request.
See [Receiving Credentials](./server-generation.md#receiving-credentials) for the
generated types, rejection behavior, and checks to perform in your service.

## Workspace Crate Output

Use `--workspace` when you want the output to have its own `Cargo.toml` and
`src/` directory. You can build that output as a crate or include it in a Cargo
workspace. The flag is available with `client-mod` and `server-mod`; using it
with `types` or `client` produces a generation error.

```text
-w, --workspace
```

For example, let's generate a client crate in `petstore-api`:

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

Notice that the source files are inside `src/`, and the module root is
`src/lib.rs`. This makes `petstore-api` a crate root. The type and client or
server files contain the same code as the corresponding module output.

### Crate Name

The package takes its name from the final component of the output directory, so
`-o petstore-api` produces `name = "petstore-api"` and a `petstore_api` library
target. Names are transliterated to ASCII and converted to kebab-case:
`-o "output/Pet Store"` yields `pet-store`, `-o café-api` yields `cafe-api`, and
`-o event_stream` yields `event-stream`. The library target of `event-stream` is
still `event_stream`, because Cargo maps dashes to underscores.

Generation fails when no valid name can be derived, for example when the
directory name is empty after sanitization or starts with a digit.

### Generated Manifest

The manifest declares the package and the dependencies its generated code
uses. This example shows a client with builders enabled; the dependency list
for your API may differ:

```toml
[package]
name = "petstore-api"
version = "0.0.0"
edition = "2024"
rust-version = "1.89"
description = "Rust client generated from the Swagger Petstore OpenAPI document"

[dependencies]
anyhow = "1.0"
bon = { version = "3.10", features = ["implied-bounds"] }
chrono = { version = "0.4.42", default-features = false, features = ["std", "clock", "serde"] }
http = "1.5"
indexmap = { version = "2.14", features = ["serde"] }
oas3-gen-support = "0.29.0"
reqwest = { version = "0.13", default-features = false, features = ["json", "multipart", "http2", "native-tls", "query", "stream"] }
serde = { version = "1.0", features = ["derive"] }
serde_json = { version = "1.0", features = ["preserve_order"] }
serde_with = { version = "3.24", features = ["base64", "chrono"] }
validator = { version = "0.21", features = ["derive"] }
```

The generator rewrites `Cargo.toml` on every run and adds a banner identifying
it as generated output. Put code that needs its own dependencies in a separate
crate so that regenerating the API doesn't overwrite your changes.

The package version starts at `0.0.0`. Pass `--module-version` to set a version
for the generated crate:

```bash
cargo run -- generate client-mod -i petstore.json -o petstore-api --workspace --module-version 1.0.0
```

The `--module-version` flag requires `--workspace`, because that's the mode
that creates the manifest.

### Dependencies

Each generator release contains the dependency version requirements it writes
to the manifest. It doesn't read them from your project's environment. The
requirement for `oas3-gen-support` uses the generator's version. Cargo resolves
these requirements when you build the generated crate.

The manifest declares crates referenced by the generated source. For example,
a specification without date or time types doesn't need a `chrono` entry, and
server output doesn't need the client's `reqwest` dependency. The following
table describes the dependencies you may see:

| Crate | Declared when the generated code |
|-------|----------------------------------|
| `anyhow` | returns [`anyhow::Result`][rustdoc-anyhow-result] from client or server methods |
| `axum` | defines a `server-mod` router and extractors, with the `multipart` feature |
| `axum-extra` | reads cookie API keys on the server, with the `cookie` feature |
| `bon` | derives builders (`--enable-builders`) |
| `chrono` | maps `date`, `date-time`, or `time` formats |
| `http` | emits header constants or [`HeaderMap`][rustdoc-http-header-map] conversions |
| `indexmap` | emits [`IndexMap`][rustdoc-indexmap-map]/[`IndexSet`][rustdoc-indexmap-set] collection types |
| `oas3-gen-support` | uses runtime derives, diagnostics, or event streams |
| `regex` | emits `pattern` validation constants |
| `reqwest` | performs `client-mod` HTTP calls |
| `reqwest_cookie_store` | sends cookie API keys from the client |
| `secrecy` | stores API keys and bearer tokens |
| `serde` | derives [`Serialize`][rustdoc-serde-serialize]/[`Deserialize`][rustdoc-serde-deserialize] |
| `serde_json` | handles freeform [`serde_json::Value`][rustdoc-serde-json-value] payloads |
| `serde_with` | applies `serde_as` conversions |
| `uuid` | maps the `uuid` format |
| `validator` | derives [`Validate`][rustdoc-validate] |

`serde_json` gains the `preserve_order` feature under the default collection
policy so that freeform JSON objects keep their key order. With
[`--no-ordered-collections`](#ordering-and-collections) the feature is omitted.

### Using the Crate

The manifest declares a plain `[package]` with no `[workspace]` table, so the
crate builds standalone and can also be added to an existing workspace:

```toml
[workspace]
members = ["crates/*", "petstore-api"]
```

To use the generated crate from another crate, add a path dependency to that
crate's `Cargo.toml`:

```toml
[dependencies]
petstore-api = { path = "../petstore-api" }
```

Keep the default [`--visibility public`](#visibility) for a crate other code
depends on. `crate` and `file` visibility restrict the re-exports in `lib.rs`, so
other crates can't access those re-exported items.

## Generated Names

Schema and property names become Rust identifiers. When a name isn't a valid
identifier, or would collide with something Rust or the generated code already
uses, the generator changes it as shown in the following table:

| Name in the specification | Generated name | Reason |
|---|---|---|
| A property named after a keyword, such as `type` or `match` | `r#type` | Keywords need the raw identifier prefix |
| A property named `crate`, `self`, or `super` | `crate_`, `self_`, `super_` | These keywords can't be raw identifiers |
| A property that starts with a digit, such as `2fa` | `_2fa` | Identifiers can't start with a digit |
| A schema named after a prelude item, such as `Option` or `Vec` | `OptionType` | The name would shadow the prelude |
| A schema named `Self` | `SelfType` | `Self` can't be a raw identifier |
| A schema named `S` | `SType` | Generated server handlers use `S` for your service type |
| A schema that starts with a digit, such as `123Response` | `T123Response` | Identifiers can't start with a digit |

A renamed property keeps its original name on the wire through
`#[serde(rename)]`, so the JSON stays the same. Enum variants follow the same
rules as schema names. For example, the enum value `self` becomes the variant
`SelfType`.

## Visibility

Use `--visibility` to choose where generated items can be accessed. The
default, `public`, makes them available to other crates. Choose `crate` when
the generated API is an internal part of your application.

```text
-C, --visibility <LEVEL>
```

| Value | Modifier | Use Case |
|-------|----------|----------|
| `public` (default) | `pub` | Library distribution |
| `crate` | `pub(crate)` | Internal crate types |
| `file` | `pub(super)` | Items used only inside the generated module |

### Example: `--visibility public`

With public visibility, the types, fields, and methods use `pub`:

```rust
pub struct Pet {
    pub id: i64,
    pub name: String,
}

pub enum Status {
    Available,
    Pending,
}

impl Status {
    pub fn available() -> Self { Self::Available }
}
```

### Example: `--visibility crate`

With crate visibility, those items use `pub(crate)`:

```rust
pub(crate) struct Pet {
    pub(crate) id: i64,
    pub(crate) name: String,
}

pub(crate) enum Status {
    Available,
    Pending,
}

impl Status {
    pub(crate) fn available() -> Self { Self::Available }
}
```

### Example: `--visibility file`

With file visibility, those items use `pub(super)`:

```rust
pub(super) struct Pet {
    pub(super) id: i64,
    pub(super) name: String,
}

pub(super) enum Status {
    Available,
    Pending,
}

impl Status {
    pub(super) fn available() -> Self { Self::Available }
}
```

In module output, `pub(super)` lets the generated files use each other's items:
`client.rs` or `server.rs` can reach the types in `types.rs`. The module's
`mod.rs` or `lib.rs` doesn't re-export anything, so the items stay inside the
generated module.

## Enum Mode

Different schema values can become the same Rust variant name. For example,
`"ACTIVE"` and `"active"` both become `Active`. Use `--enum-mode` to choose
whether to merge those values, keep them separate, or accept differences in
ASCII letter case during deserialization.

```text
--enum-mode <MODE>
```

| Value | Behavior |
|-------|----------|
| `merge` (default) | Merge duplicates; first occurrence is canonical, others become aliases |
| `preserve` | Keep all variants; append numeric suffix to collisions |
| `relaxed` | Merge duplicates; enable case-insensitive deserialization |

### Input Schema

Let's use the same schema to compare the three modes:

```json
{
  "type": "string",
  "enum": ["ACTIVE", "active", "Active", "PENDING"]
}
```

### Example: `--enum-mode merge`

The default mode merges values that produce the same Rust identifier. It
uses the first value as the serialization name and adds the others as Serde
aliases for deserialization:

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Status {
    #[serde(rename = "ACTIVE", alias = "active", alias = "Active")]
    Active,
    #[serde(rename = "PENDING")]
    Pending,
}
```

All three spellings of `"ACTIVE"` in the schema deserialize to
`Status::Active`. The lowercase string `"pending"` produces an error because
it isn't declared as a value or alias. Merging preserves the declared
spellings; it doesn't make every spelling case-insensitive.

### Example: `--enum-mode preserve`

Use `preserve` when your application needs to distinguish the declared
spellings. Each value gets its own variant, and colliding names get numeric
suffixes:

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Status {
    #[serde(rename = "ACTIVE")]
    Active,
    #[serde(rename = "active")]
    Active1,
    #[serde(rename = "Active")]
    Active2,
    #[serde(rename = "PENDING")]
    Pending,
}
```

Now `"ACTIVE"`, `"active"`, and `"Active"` deserialize to `Status::Active`,
`Status::Active1`, and `Status::Active2`, respectively. Your code can match
those variants separately.

### Example: `--enum-mode relaxed`

Use `relaxed` when you want to accept values regardless of ASCII letter
case. The generator merges the variants and writes a [`Deserialize`][rustdoc-serde-deserialize]
implementation that converts input to ASCII lowercase before matching:

```rust
#[derive(Debug, Clone, Serialize)]
pub enum Status {
    #[serde(rename = "ACTIVE")]
    Active,
    #[serde(rename = "PENDING")]
    Pending,
}

impl<'de> serde::Deserialize<'de> for Status {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        match s.to_ascii_lowercase().as_str() {
            "active" => Ok(Self::Active),
            "pending" => Ok(Self::Pending),
            _ => Err(serde::de::Error::unknown_variant(&s, &["active", "pending"])),
        }
    }
}
```

This version accepts both `"PENDING"` and `"pending"` as `Status::Pending`,
as well as the different spellings of `"ACTIVE"`. Serialization still uses
the declared name shown in the `#[serde(rename)]` attribute.

## Enum Layout

Once you've chosen how to handle enum values, you can choose their order in
the generated source. The `--enum-layout` flag preserves the specification's
order by default or sorts variants by their Rust names.

```text
--enum-layout <LAYOUT>
```

| Value | Behavior |
|-------|----------|
| `spec` (default) | Preserve variant order from the OpenAPI document |
| `sorted` | Sort variants alphabetically by Rust variant name |

`sorted` applies to value enums (string `enum`), `oneOf`/`anyOf` union variants,
and discriminated enum variants. HTTP response (status-code) enums are unaffected
and continue to use status-code order.

Use `sorted` when you want the declarations to stay in the same order after
the specification reorders enum values. This can make changes to the generated
source easier to review.

Ordering can also affect the default value. For value enums that derive
[`Default`][rustdoc-default], the generator marks the variant matching the schema's `default`.
If no variant matches, it uses the first declared variant. Switching to
`sorted` can therefore change the fallback default.

Untagged unions, described with `oneOf` or `anyOf`, handle defaults differently.
When a variant can represent the schema's default, the generator writes an
`impl Default` that constructs that value. For example, it might return
`Self::Preset(Preset::Auto2k)` for a wrapped enum or
`Self::String("cheesecake".to_string())` for a string.

A nullable union includes a `null` branch. If it has no usable default, it gets
no `Default` implementation. Properties with that union type use [`Option<T>`][rustdoc-option],
including properties listed as required.

### Input Schema

Consider a schema whose values aren't in alphabetical order:

```json
{
  "type": "string",
  "enum": ["zeta", "alpha", "gamma", "beta"]
}
```

### Example: `--enum-layout spec`

The default layout keeps `Zeta` first, followed by the remaining values in
schema order:

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Status {
    #[serde(rename = "zeta")]
    Zeta,
    #[serde(rename = "alpha")]
    Alpha,
    #[serde(rename = "gamma")]
    Gamma,
    #[serde(rename = "beta")]
    Beta,
}
```

### Example: `--enum-layout sorted`

The sorted layout puts `Alpha` first. The serialization names remain attached
to the same variants:

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Status {
    #[serde(rename = "alpha")]
    Alpha,
    #[serde(rename = "beta")]
    Beta,
    #[serde(rename = "gamma")]
    Gamma,
    #[serde(rename = "zeta")]
    Zeta,
}
```

## Numeric-Backed Enums

An enum can represent numbers as well as strings. When a schema combines an
`enum` array with `type: integer` or `type: number`, the generated type reads
and writes JSON numbers. The generator supplies [`Serialize`][rustdoc-serde-serialize] and [`Deserialize`][rustdoc-serde-deserialize]
implementations so that a value such as `8000` stays a number during
serialization.

Consider a schema that lists the sample rates an audio API accepts:

```json
{
  "type": "integer",
  "enum": [8000, 16000, 24000, 44100, 48000]
}
```

The generator emits one variant per value, a serializer that turns each variant
into its number, and a deserializer that maps numbers back to variants. These
implementations use the [`serde::Serializer`][rustdoc-serde-serializer] and [`serde::Deserializer`][rustdoc-serde-deserializer] traits,
with [`serde::de::Error`][rustdoc-serde-de-error] for decoding errors:

```rust
#[derive(Debug, Clone, PartialEq, Eq, Hash, oas3_gen_support::Default)]
pub enum SampleRate {
    #[default]
    Value8000,
    /* ... */
    Value48000,
}

impl serde::Serialize for SampleRate {
    fn serialize<S>(&self, serializer: S) -> core::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let value: i64 = match self {
            Self::Value8000 => 8000i64,
            /* ... */
            Self::Value48000 => 48000i64,
        };
        serializer.serialize_i64(value)
    }
}

impl<'de> serde::Deserialize<'de> for SampleRate {
    fn deserialize<D>(deserializer: D) -> core::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = i64::deserialize(deserializer)?;
        match value {
            8000i64 => Ok(Self::Value8000),
            /* ... */
            _ => Err(serde::de::Error::custom(/* ... */)),
        }
    }
}
```

`SampleRate::Value8000` now serializes to `8000`, and `8000` deserializes back
to `SampleRate::Value8000`. A number outside the listed values produces an
error that names the accepted values.

The schema's type and format determine which Rust number type the generated
implementation uses. Signed integers use [`i64`][rustdoc-i64], unsigned integers use [`u64`][rustdoc-u64],
and floating-point values use [`f64`][rustdoc-f64]. In each case, the serialized value is a
JSON number.

### Floating-Point Values

For a `number` enum, the generated deserializer reads an [`f64`][rustdoc-f64] and compares
its bit pattern with each allowed value. Here's an excerpt for a playback
rate that accepts `0.5`:

```rust
impl<'de> serde::Deserialize<'de> for PlaybackRate {
    fn deserialize<D>(deserializer: D) -> core::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = f64::deserialize(deserializer)?;
        match value.to_bits() {
            bits if bits == (0.5f64).to_bits() => Ok(Self::Value0_5),
            /* ... */
            _ => Err(serde::de::Error::custom(/* ... */)),
        }
    }
}
```

The Rust variants don't store floating-point values: each variant identifies
one allowed number. These enums can therefore derive [`Eq`][rustdoc-eq] and [`Hash`][rustdoc-hash], and you
can use them as map keys or set members.

## Helper Methods

By default, the generator adds constructor helpers for supported enum
variants. For a variant that wraps a struct with a [`Default`][rustdoc-default] implementation,
a helper can accept the required values and fill in the remaining fields.
Pass `--no-helpers` if you'd prefer to construct the variants yourself.

```text
--no-helpers
```

### Default (Helpers Enabled)

For this `ContentBlock` enum, the helpers construct the nested structs and
wrap them in the matching variants:

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ContentBlock {
    Text(TextBlock),
    Image(ImageBlock),
    Code(CodeBlock),
}

impl ContentBlock {
    pub fn text(text: String) -> Self {
        Self::Text(TextBlock {
            text,
            ..Default::default()
        })
    }

    pub fn image(source: Box<ImageSource>) -> Self {
        Self::Image(ImageBlock {
            source,
            ..Default::default()
        })
    }

    pub fn code(code: String) -> Self {
        Self::Code(CodeBlock {
            code,
            ..Default::default()
        })
    }
}
```

The `text()` helper lets us supply the text directly:

```rust
let block = ContentBlock::text("Hello, world!".to_string());
```

### With `--no-helpers`

The enum still has the same variants, but the constructor helpers are omitted:

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ContentBlock {
    Text(TextBlock),
    Image(ImageBlock),
    Code(CodeBlock),
}
```

We now construct the `TextBlock` and pass it to the `Text` variant ourselves:

```rust
let block = ContentBlock::Text(TextBlock {
    text: "Hello, world!".to_string(),
    ..Default::default()
});
```

## OData Support

An OData API may omit metadata fields that its schema lists as required. If
you encounter this with fields such as `@odata.type`, use `--odata-support` to
make eligible fields optional in the generated types.

```text
--odata-support
```

This rule applies to names starting with `@odata.` when the parent schema has
no discriminator and isn't an intersection using `allOf`.

### Input Schema

Let's use a schema that requires an ID and two metadata fields:

```json
{
  "type": "object",
  "properties": {
    "id": { "type": "string" },
    "@odata.type": { "type": "string" },
    "@odata.id": { "type": "string" }
  },
  "required": ["id", "@odata.type", "@odata.id"]
}
```

### Default (OData Support Disabled)

Without the flag, all three fields are required strings:

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Entity {
    pub id: String,
    #[serde(rename = "@odata.type")]
    pub odata_type: String,
    #[serde(rename = "@odata.id")]
    pub odata_id: String,
}
```

Deserialization fails if `@odata.type` or `@odata.id` are missing from the response.

### With `--odata-support`

With the flag, the metadata fields become optional:

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Entity {
    pub id: String,
    #[serde(rename = "@odata.type")]
    pub odata_type: Option<String>,
    #[serde(rename = "@odata.id")]
    pub odata_id: Option<String>,
}
```

Now deserialization succeeds when either metadata field is absent, and the
corresponding Rust field is `None`. The `id` field is still required because
its name doesn't start with `@odata.`.

## Type Customization

Use `--customize` to choose a `serde_with` adapter for a generated field. An
adapter controls how Serde reads or writes a value. The generated field keeps
its Rust type, and the adapter appears in its [`#[serde_as]`][rustdoc-serde-as] attribute.

```text
-c, --customize <TYPE=PATH>
```

The following keys select commonly customized formats:

| Key | OpenAPI Format | Rust Field Type |
|-----|----------------|-----------------|
| `date_time` | `date-time` | [`chrono::DateTime<chrono::Utc>`][rustdoc-chrono-datetime] |
| `date` | `date` | [`chrono::NaiveDate`][rustdoc-chrono-date] |
| `time` | `time` | [`chrono::NaiveTime`][rustdoc-chrono-time] |
| `duration` | `duration` | [`std::time::Duration`][rustdoc-duration] |
| `uuid` | `uuid` | [`uuid::Uuid`][rustdoc-uuid] |

The date-time type uses [`chrono::Utc`][rustdoc-chrono-utc] as its time zone. For these examples,
suppose your crate defines adapters named `DateTimeAdapter` and `DateAdapter`.
Each adapter must implement [`serde_with::SerializeAs`][rustdoc-serialize-as] or
[`serde_with::DeserializeAs`][rustdoc-deserialize-as] as needed by the generated field. To use both
adapters, repeat the flag:

```bash
cargo run -- generate types -i spec.json -o types.rs \
  -c date_time=crate::DateTimeAdapter \
  -c date=crate::DateAdapter
```

### Default (No Customization)

Without an override, the fields use their normal Serde implementations:

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Event {
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub scheduled_date: chrono::NaiveDate,
}
```

### With a Date-Time Adapter

If we pass only `-c date_time=crate::DateTimeAdapter`, the generator adds an
adapter to `created_at`:

```rust
#[serde_with::serde_as]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Event {
    #[serde_as(as = "crate::DateTimeAdapter")]
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub scheduled_date: chrono::NaiveDate,
}
```

Notice that `created_at` is still a [`chrono::DateTime<chrono::Utc>`][rustdoc-chrono-datetime]. The
`DateTimeAdapter` controls its conversion during serialization and
deserialization. The `scheduled_date` field is unchanged because we haven't
supplied a `date` override in this example.

### Handling Optional and Array Fields

The generator wraps the adapter in [`Option`][rustdoc-option] and [`Vec`][rustdoc-vec] to match optional and
array fields. For example, the same date-time override produces these
attributes on a schedule:

```rust
#[serde_with::serde_as]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Schedule {
    #[serde_as(as = "crate::DateTimeAdapter")]
    pub start: chrono::DateTime<chrono::Utc>,

    #[serde_as(as = "Option<crate::DateTimeAdapter>")]
    pub end: Option<chrono::DateTime<chrono::Utc>>,

    #[serde_as(as = "Vec<crate::DateTimeAdapter>")]
    pub milestones: Vec<chrono::DateTime<chrono::Utc>>,

    #[serde_as(as = "Option<Vec<crate::DateTimeAdapter>>")]
    pub optional_dates: Option<Vec<chrono::DateTime<chrono::Utc>>>,
}
```

Each attribute follows the shape of its field. You supply the adapter for one
date-time value; the generator adds the surrounding container adapters.

## Operation Filtering

You may only need a few operations from a large API. Use `--only` to select
those operations, or `--exclude` to leave specific operations out. You can use
one of these flags at a time.

```text
--only <id1,id2,...>
--exclude <id1,id2,...>
```

### `--only`

Pass the normalized `snake_case` operation IDs, separated by commas. Filtering
uses these IDs before any `--fn-name` overrides. For example, the petstore's
`listPets` and `createPets` operations use `list_pets` and `create_pets` in the
filter:

```bash
cargo run -- generate client-mod -i petstore.json -o output/ \
  --only list_pets,create_pets
```

The generated client keeps `list_pets()` and `create_pets()` and omits the
other operations. With `server-mod`, the same selection keeps those two trait
methods and their routes. Types needed by the selected methods are still
included, as we'll see in [Schema Dependency Resolution](#schema-dependency-resolution).

### `--exclude`

Use `--exclude` when you want most of an API but don't need a particular
operation. For example, to leave out the image upload operation, run:

```bash
cargo run -- generate client-mod -i petstore.json -o output/ \
  --exclude upload_pet_image
```

The client keeps the petstore's other operations, including `list_pets()`
and `create_pets()`, and omits `upload_pet_image()`. In `server-mod` mode, the
flag omits that operation's trait method and route.

### Schema Dependency Resolution

Filtering an operation also filters the types needed to use it. The generator
follows dependencies through the schemas so that a selected operation has the
types it needs:

1. It collects schemas used by the selected operations' parameters, request
   bodies, and responses.
2. It follows references from those schemas to other schemas.
3. It generates the resulting set of types.

For example, suppose `listPets` returns an array of `Pet` values and each `Pet`
contains a `Category`. Selecting `listPets` includes both `Pet` and `Category`,
even though the operation doesn't refer to `Category` directly.

## Function Name Overrides

Use `--fn-name` to choose the generated client or server method name for an
operation. The key is its `operationId` from the specification; the generator
also accepts the normalized operation ID shown by `oas3-gen list operations`.

```text
--fn-name <ID=NAME>
```

The generator converts your method name to `snake_case` and uses it to derive
the request type's name. For example, renaming `listPets` to `fetch_all_pets`
also produces `FetchAllPetsRequest`. Repeat the flag to rename more than one
operation:

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

## API Name Override

Use `--api-name` to name the client struct in `client` or `client-mod` mode,
or the server trait in `server-mod` mode. The generator converts the supplied
name to `PascalCase`.

```text
--api-name <NAME>
```

By default, the client name comes from the specification's `info.title`.
For example, `"Swagger Petstore"` becomes `SwaggerPetstoreClient`; an empty
title produces `ApiClient`. The default server trait name is `ApiServer`.
To choose `PetStoreClient` for the client, run:

```bash
cargo run -- generate client-mod -i petstore.json -o output/ --api-name PetStoreClient
```

The client struct and its implementation now use `PetStoreClient`. In server
mode, choose a trait name with the same flag:

```bash
cargo run -- generate server-mod -i petstore.json -o output/ --api-name PetStoreApi
```

The server trait is named `PetStoreApi`, and the generated router uses that
name in its trait bound. Changing the API name doesn't rename the operations;
use [`--fn-name`](#function-name-overrides) for those.

## Schema Filtering

By default, the generator includes schemas used by the selected operations
and the schemas they depend on. Pass `--all-schemas` when you also need types
that no operation references.

```text
--all-schemas
```

### Default (Reachability Filtering)

Suppose a specification defines `Pet`, `Category`, `Store`, and `Inventory`,
but its operations only use `Pet` and `Category`. Generate types with the
default options:

```bash
cargo run -- generate types -i spec.json -o types.rs
```

The output contains `Pet` and `Category`. The generator skips `Store` and
`Inventory` and reports them as orphaned schemas, meaning no selected operation
needs them.

### With `--all-schemas`

To include those unused schemas too, add the flag:

```bash
cargo run -- generate types -i spec.json -o types.rs --all-schemas
```

The output now contains all four types: `Pet`, `Category`, `Store`, and
`Inventory`.

### Combining with Operation Filtering

You can't combine `--all-schemas` with `--only` or `--exclude` in one command.
If you need both a complete set of types and a filtered client, generate two
outputs:

```bash
cargo run -- generate types -i spec.json -o types.rs --all-schemas
cargo run -- generate client -i spec.json -o client.rs --only list_pets
```

The first file contains all schemas. The second contains the filtered client
and its own required types; these commands don't make the client import types
from the first file.

## Header Emission

Header name constants use [`http::HeaderName`][rustdoc-http-header-name], so you can refer to headers
without repeating their string names. By default, the generator creates constants for request parameters and
[response headers](#response-headers) used by the selected operations. Use
`--all-headers` to include header parameters from `components/parameters` even
when no operation references them.

Headers that the `http` crate already names are the exception. The
[`http::header`][rustdoc-http-header-constants] module declares constants for
standard headers such as `Content-Type`, `Location`, and `Authorization`. The
generator doesn't declare its own constant for any of them; generated code
refers to `http`'s constant, such as `http::header::LOCATION`, instead.

OpenAPI says to ignore header parameters named `Accept`, `Content-Type`, or
`Authorization`, so the generator leaves them out of request types. The
specification describes those headers elsewhere: media types cover `Accept` and
`Content-Type`, and security schemes cover `Authorization`. To send credentials
in `Authorization`, declare a [security scheme](#security-schemes).

```text
--all-headers
```

### Default (Operation-Referenced Headers Only)

Suppose an operation uses `x-api-version`, and the specification also defines
an unused `x-api-key` header in `components/parameters`. Start with the default
options:

```bash
cargo run -- generate types -i spec.json -o types.rs
```

The output contains a constant for the header used by the operation:

```rust
pub const X_API_VERSION: http::HeaderName = http::HeaderName::from_static("x-api-version");
```

`x-api-key` is not emitted because no operation uses it.

### With `--all-headers`

Now include the unused component header:

```bash
cargo run -- generate types -i spec.json -o types.rs --all-headers
```

Both names have constants in the output:

```rust
pub const X_API_VERSION: http::HeaderName = http::HeaderName::from_static("x-api-version");
pub const X_API_KEY: http::HeaderName = http::HeaderName::from_static("x-api-key");
```

## Builder Generation

Use `--enable-builders` when you want to construct generated values through
setter methods. The generator adds [`bon::Builder`][rustdoc-bon-builder] derives to schema structs
and `#[builder]` constructors to request structs.

```text
--enable-builders
```

Builders are disabled by default. The [Builder Pattern](./builders.md) chapter
walks through using them and explains how request builders run validation.

### Default (Builders Disabled)

Without the flag, generated structs have no `bon` attributes. This excerpt
shows a schema type and a request type:

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Pet {
    pub id: i64,
    pub name: String,
}

pub struct CreatePetRequest {
    pub path: CreatePetRequestPath,
}
```

### With `--enable-builders`

With the flag, the schema gets a derive and the request gets a constructor
annotated for `bon`:

```rust
#[derive(Debug, Clone, Serialize, Deserialize, bon::Builder)]
pub struct Pet {
    pub id: i64,
    pub name: String,
}

pub struct CreatePetRequest {
    pub path: CreatePetRequestPath,
}

#[bon::bon]
impl CreatePetRequest {
    #[builder]
    pub fn new(/* params */) -> anyhow::Result<Self> {
        /* ... */
    }
}
```

## Ordering and Collections

By default, the generator preserves declaration order from the OpenAPI
document for schemas, fields, variants, operations, and header constants.
You can change enum ordering separately with
[`--enum-layout`](#enum-layout).

The generator also uses collection types that preserve insertion order at
runtime. Maps described by `additionalProperties` use
[`indexmap::IndexMap<String, T>`][rustdoc-indexmap-map], and arrays with `uniqueItems: true` use
[`indexmap::IndexSet<T>`][rustdoc-indexmap-set]. If you include these types in an existing crate, add
`indexmap` with its `serde` feature to your `Cargo.toml`. A generated workspace
manifest includes that dependency when needed.

Use this flag to choose standard library collections instead:

```text
--no-ordered-collections
```

Set `--no-ordered-collections` to opt out of the `indexmap` runtime types. Map
schemas then resolve to [`std::collections::HashMap<String, T>`][rustdoc-hashmap], and
`uniqueItems` arrays resolve to [`Vec<T>`][rustdoc-vec], the same type as a normal array, so
uniqueness is no longer expressed at the type level. Use this flag when your
code cannot depend on `indexmap`, or when you do not need JSON key and element
order to survive a deserialize-then-serialize round trip.

This flag does not change the declaration order of items in the generated
source, such as struct fields, enum variants, and operation methods.

## Documentation Formatting

Descriptions and summaries in the OpenAPI document become documentation
comments in the generated code. Use `--doc-format` to format longer comments
with the external [mdformat](https://github.com/executablebooks/mdformat) tool.
The generator sends text longer than 100 bytes to `mdformat` with a line-wrap
setting of 100 characters.

```text
--doc-format
```

Install `mdformat` and make sure it's available on your `PATH` before using
the flag:

```bash
pip install mdformat
```

### Default (Formatting Disabled)

Without the flag, the generator preserves documentation text while converting
escaped newlines to line breaks. A long description can therefore remain on
one line:

```rust
/// A long description that may contain very long lines that extend well beyond typical line widths because the OpenAPI spec author did not wrap them.
pub struct Widget {
    pub id: i64,
}
```

### With `--doc-format`

```bash
cargo run -- generate types -i spec.json -o types.rs --doc-format
```

The same description is wrapped into multiple lines:

```rust
/// A long description that may contain very long lines that extend well beyond typical line widths
/// because the OpenAPI spec author did not wrap them.
pub struct Widget {
    pub id: i64,
}
```

## Flag Summary

This table collects the options used across the generation chapters. Defaults
apply when you omit the corresponding option. Run `oas3-gen generate --help`
to see the command-line help, including input, output, and terminal options.

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
| `--no-ordered-collections` | `false` | Emit [`HashMap`][rustdoc-hashmap]/[`Vec`][rustdoc-vec] instead of `indexmap` collection types |
| `--doc-format` | `false` | Format documentation comments with `mdformat` |
| `--only` | *(none)* | Include only specified operations |
| `--exclude` | *(none)* | Exclude specified operations |
| `--all-schemas` | `false` | Generate all schemas regardless of usage |

To work through constructing the generated values, continue to the
[Builder Pattern](./builders.md) chapter.

[rustdoc-anyhow-result]: https://docs.rs/anyhow/1.0.104/anyhow/type.Result.html
[rustdoc-bon-builder]: https://docs.rs/bon/3.10.1/bon/derive.Builder.html
[rustdoc-chrono-date]: https://docs.rs/chrono/0.4.45/chrono/struct.NaiveDate.html
[rustdoc-chrono-datetime]: https://docs.rs/chrono/0.4.45/chrono/struct.DateTime.html
[rustdoc-chrono-time]: https://docs.rs/chrono/0.4.45/chrono/struct.NaiveTime.html
[rustdoc-chrono-utc]: https://docs.rs/chrono/0.4.45/chrono/struct.Utc.html
[rustdoc-debug]: https://doc.rust-lang.org/std/fmt/trait.Debug.html
[rustdoc-default]: https://doc.rust-lang.org/std/default/trait.Default.html
[rustdoc-deserialize-as]: https://docs.rs/serde_with/3.24.0/serde_with/trait.DeserializeAs.html
[rustdoc-display]: https://doc.rust-lang.org/std/fmt/trait.Display.html
[rustdoc-duration]: https://doc.rust-lang.org/std/time/struct.Duration.html
[rustdoc-eq]: https://doc.rust-lang.org/std/cmp/trait.Eq.html
[rustdoc-expose-secret]: https://docs.rs/secrecy/0.10.3/secrecy/trait.ExposeSecret.html
[rustdoc-f64]: https://doc.rust-lang.org/std/primitive.f64.html
[rustdoc-from-str]: https://doc.rust-lang.org/std/str/trait.FromStr.html
[rustdoc-hash]: https://doc.rust-lang.org/std/hash/trait.Hash.html
[rustdoc-hashmap]: https://doc.rust-lang.org/std/collections/struct.HashMap.html
[rustdoc-http-header-constants]: https://docs.rs/http/1.5.0/http/header/index.html#constants
[rustdoc-http-header-map]: https://docs.rs/http/1.5.0/http/header/struct.HeaderMap.html
[rustdoc-http-header-name]: https://docs.rs/http/1.5.0/http/header/struct.HeaderName.html
[rustdoc-http-status]: https://docs.rs/http/1.5.0/http/status/struct.StatusCode.html
[rustdoc-i64]: https://doc.rust-lang.org/std/primitive.i64.html
[rustdoc-indexmap-map]: https://docs.rs/indexmap/2.14.2/indexmap/map/struct.IndexMap.html
[rustdoc-indexmap-set]: https://docs.rs/indexmap/2.14.2/indexmap/set/struct.IndexSet.html
[rustdoc-option]: https://doc.rust-lang.org/std/option/enum.Option.html
[rustdoc-partial-eq]: https://doc.rust-lang.org/std/cmp/trait.PartialEq.html
[rustdoc-secret-string]: https://docs.rs/secrecy/0.10.3/secrecy/type.SecretString.html
[rustdoc-serde-as]: https://docs.rs/serde_with/3.24.0/serde_with/attr.serde_as.html
[rustdoc-serde-de-error]: https://docs.rs/serde/1.0.229/serde/de/trait.Error.html
[rustdoc-serde-deserialize]: https://docs.rs/serde/1.0.229/serde/trait.Deserialize.html
[rustdoc-serde-deserializer]: https://docs.rs/serde/1.0.229/serde/trait.Deserializer.html
[rustdoc-serde-json-value]: https://docs.rs/serde_json/1.0.151/serde_json/enum.Value.html
[rustdoc-serde-serialize]: https://docs.rs/serde/1.0.229/serde/trait.Serialize.html
[rustdoc-serde-serializer]: https://docs.rs/serde/1.0.229/serde/trait.Serializer.html
[rustdoc-serialize-as]: https://docs.rs/serde_with/3.24.0/serde_with/trait.SerializeAs.html
[rustdoc-string]: https://doc.rust-lang.org/std/string/struct.String.html
[rustdoc-u64]: https://doc.rust-lang.org/std/primitive.u64.html
[rustdoc-unit]: https://doc.rust-lang.org/std/primitive.unit.html
[rustdoc-uuid]: https://docs.rs/uuid/1.26.1/uuid/struct.Uuid.html
[rustdoc-validate]: https://docs.rs/validator/0.21.0/validator/trait.Validate.html
[rustdoc-vec]: https://doc.rust-lang.org/std/vec/struct.Vec.html

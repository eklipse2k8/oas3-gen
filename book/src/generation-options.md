# Generation Options

Generation options choose the output layout, select operations, and control the
generated API's names and visibility. Each section here explains what an option does
and links to the walkthrough that shows it in use. The [Flag Summary](#flag-summary)
lists every option with its default.

Commands run from a checkout of this repository. If you installed the tool, replace
`cargo run --` with `oas3-gen`. Use the path to your own document in place of
`spec.json` or `petstore.json`. Unless stated otherwise, examples refer to
`fixtures/petstore.json`; Rust excerpts may omit imports, attributes, and unrelated
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
- [Raw Response Bodies](#raw-response-bodies)
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

| Mode | Output | Learn More |
|------|--------|------------|
| `types` (default) | Type definitions in one file | [Schemas and Types](./schema-types.md) |
| `client` | Types and a `reqwest` client in one file | [Choosing the Output Layout](./client-generation.md#choosing-the-output-layout) |
| `client-mod` | `types.rs`, `client.rs`, and `mod.rs` in a directory | [Client Generation](./client-generation.md) |
| `server-mod` | `types.rs`, `server.rs`, and `mod.rs` in a directory | [Server Generation](./server-generation.md) |

In every mode, a type's derives follow how the API uses it. The petstore's `Pet` appears
only in responses, so it derives [`Deserialize`][rustdoc-serde-deserialize] but not
[`Serialize`][rustdoc-serde-serialize]. `NewPet`, which the API only accepts, derives
`Serialize` and [`Validate`][rustdoc-validate] instead. Server output reverses those
directions.

## Workspace Crate Output

Use `--workspace` when you want the output to be its own crate, with a `Cargo.toml` and
a `src/` directory. You can build that crate standalone or add it to a Cargo workspace.
The flag is available with `client-mod` and `server-mod`; using it with `types` or
`client` produces a generation error.

```text
-w, --workspace
```

The crate's `src/lib.rs` takes the place of `mod.rs` as the library root, and the type
and client or server files contain the same code as module output. To see a generated
crate and add it to a workspace, follow
[Generating a Separate Crate](./client-generation.md#generating-a-separate-crate) for a
client or
[the server version](./server-generation.md#generating-a-separate-crate) for a server.

### Crate Name

The package takes its name from the final component of the output directory, so
`-o petstore-api` produces `name = "petstore-api"` and a `petstore_api` library target.
Names are transliterated to ASCII and converted to kebab-case: `-o "output/Pet Store"`
yields `pet-store`, `-o café-api` yields `cafe-api`, and `-o petstore_v2` yields
`petstore-v2`. The library target of `petstore-v2` is still `petstore_v2`, because
Cargo maps dashes to underscores.

Generation fails when no valid name can be derived, for example when the directory name
is empty after sanitization or starts with a digit.

### Generated Manifest

The manifest declares a `[package]` and the dependencies its generated code uses. Its
description comes from the document's `info.title`. The generator rewrites `Cargo.toml`
on every run and adds a banner identifying it as generated output, so changes you make
to it don't survive regeneration.

The package version starts at `0.0.0`. Pass `--module-version` to set a version for the
generated crate:

```bash
cargo run -- generate client-mod -i petstore.json -o petstore-api --workspace --module-version 1.0.0
```

The `--module-version` flag requires `--workspace`, because that's the mode that
creates the manifest.

### Dependencies

Each generator release contains the dependency version requirements it writes to the
manifest. It doesn't read them from your project's environment. The requirement for
`oas3-gen-support` uses the generator's version. Cargo resolves these requirements when
you build the generated crate. If you include module output in an existing crate
instead, add the same dependencies to that crate's manifest.

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
uses `indexmap`. The feature checks the length of an `IndexSet` or `IndexMap` field,
such as the `maxItems` limit on the `tags` of `NewPet`.

## Operation Filtering

You may only need a few operations from a large API. Use `--only` to select those
operations, or `--exclude` to leave specific operations out. You can use one of these
flags at a time.

```text
--only <id1,id2,...>
--exclude <id1,id2,...>
```

Both flags take the normalized `snake_case` operation IDs that `oas3-gen list operations`
shows, separated by commas. Filtering uses these IDs before any
[`--fn-name`](#function-name-overrides) overrides. For example,
`--only list_pets,create_pet` keeps the petstore's `listPets` and `createPet`
operations, and `--exclude upload_pet_image` keeps every operation except
`uploadPetImage`.

A client omits the methods you filter out, and a server omits their trait methods and
routes. In a server, a callback follows the operation that declares it unless you name
the callback's own ID. See
[Choosing Operations and Names](./client-generation.md#choosing-operations-and-names)
for a filtered client, and
[the server version](./server-generation.md#choosing-operations-and-names) for
callbacks.

### Schema Dependency Resolution

Filtering an operation also filters the types needed to use it. The generator collects
the schemas used by the selected operations' parameters, request bodies, and responses,
then follows references from those schemas to other schemas. For example, the
petstore's `listPets` returns `Pets`, an array of `Pet` values. Selecting only
`list_pets` also generates the schemas `Pet` refers to, such as `Species` and
`PetStatus`, though the operation doesn't refer to them directly.

## Schema Filtering

By default, the generator includes schemas used by the selected operations and the
schemas they depend on. Pass `--all-schemas` when you also need types that no operation
references.

```text
--all-schemas
```

Suppose a specification defines `Pet`, `Category`, `Store`, and `Inventory`, but its
operations only use `Pet` and `Category`. By default, the output contains `Pet` and
`Category`, and with `--verbose` the generator reports how many orphaned schemas it
skipped. With `--all-schemas`, the output contains all four types.

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
`PascalCase`.

```text
--api-name <NAME>
```

By default, the client name comes from the specification's `info.title`. For example,
the petstore's title, `"Petstore"`, becomes `PetstoreClient`; an empty title produces
`ApiClient`. The default server trait name is `ApiServer`. Changing the API name doesn't
rename the operations; use [`--fn-name`](#function-name-overrides) for those. See
[naming the client](./client-generation.md#choosing-operations-and-names) and
[naming the server trait](./server-generation.md#choosing-operations-and-names) for
examples.

## Function Name Overrides

Use `--fn-name` to choose the generated client or server method name for an operation.
The key is its `operationId` from the specification; the generator also accepts the
normalized operation ID shown by `oas3-gen list operations`. Repeat the flag to rename
more than one operation.

```text
--fn-name <ID=NAME>
```

The generator converts your method name to `snake_case` and uses it to derive the
request type's name. For example, `--fn-name listPets=fetch_all_pets` produces the
method `fetch_all_pets` and the request type `FetchAllPetsRequest`. Use the new names
when calling the client or implementing the server trait, as
[Choosing Operations and Names](./client-generation.md#choosing-operations-and-names)
shows.

## Visibility

Use `--visibility` to choose where generated items can be accessed. The default,
`public`, makes them available to other crates. Choose `crate` when the generated API
is an internal part of your application.

```text
-C, --visibility <LEVEL>
```

| Value | Modifier | Use Case |
|-------|----------|----------|
| `public` (default) | `pub` | Library distribution |
| `crate` | `pub(crate)` | Internal crate types |
| `file` | `pub(super)` | Items used only inside the generated module |

The modifier applies to generated types, their fields, and their methods, such as the
[helper methods](./enum-types.md#helper-methods) on unions. With `--visibility crate`,
for example, the petstore's `Pet` struct and each of its fields are `pub(crate)`.

In module output, `pub(super)` lets the generated files use each other's items:
`client.rs` or `server.rs` can reach the types in `types.rs`. The module's `mod.rs` or
`lib.rs` doesn't re-export anything, so the items stay inside the generated module.
Keep the default `public` for a [workspace crate](#workspace-crate-output) that other
crates depend on, because `crate` and `file` restrict the re-exports in `lib.rs`.

## Header Emission

Header name constants use [`http::HeaderName`][rustdoc-http-header-name], so you can
refer to headers without repeating their string names. By default, the generator
creates constants for request parameters and
[response headers](./http.md#response-headers) used by the selected operations. Use
`--all-headers` to include header parameters from `components/parameters` even when no
operation references them.

```text
--all-headers
```

For example, the petstore's `showPetById` operation uses an `x-api-version` header, so
the output contains an `X_API_VERSION` constant. The document also declares an
`x-debug-trace` header parameter in `components/parameters` that no operation uses. With
`--all-headers`, the output gains a constant for it:

```rust
pub const X_DEBUG_TRACE: http::HeaderName = http::HeaderName::from_static(
    "x-debug-trace",
);
```

Headers that the `http` crate already names are the exception. The
[`http::header`][rustdoc-http-header-constants] module declares constants for standard
headers such as `Content-Type`, `Location`, and `Authorization`. The generator doesn't
declare its own constant for any of them; generated code refers to `http`'s constant,
such as `http::header::LOCATION`, instead.

OpenAPI says to ignore header parameters named `Accept`, `Content-Type`, or
`Authorization`, so the generator leaves them out of request types. Media types cover
`Accept` and `Content-Type`, and security schemes cover `Authorization`. For example,
the petstore's `listAuditEvents` declares a required `Authorization` header parameter,
yet its request struct has no header field. To send credentials in `Authorization`,
declare a [security scheme](./http.md#security-schemes), as that operation also does.

## Builder Generation

Use `--enable-builders` when you want to construct generated values through setter
methods. The generator adds [`bon::Builder`][rustdoc-bon-builder] derives to schema
structs and `#[builder]` constructors to request structs. A request constructor
assembles the nested parameter structs and validates the request before returning it.

```text
--enable-builders
```

Builders are disabled by default, and enabling them adds a `bon` dependency. The
[Builder Pattern](./builders.md) chapter shows
[what changes in the generated code](./builders.md#what-changes-in-the-generated-code),
how to [add `bon` to your project](./builders.md#adding-bon-to-your-project), and how
to use each kind of builder.

## Raw Response Bodies

Use `--raw-body` when a client downloads bodies you don't want to hold in memory all at
once, such as large files. By default, the client reads each binary response body into a
[`Vec<u8>`][rustdoc-vec] before it returns. With the flag, the client returns the body
unread as a [`reqwest::Body`][rustdoc-reqwest-body], and your code reads it as it arrives.

```text
--raw-body
```

For example, the petstore client's `get_pet_photo()` returns
`ApiResponse<reqwest::Body, Problem>` instead of `ApiResponse<Vec<u8>, Problem>`. The
flag doesn't change request bodies or `server-mod` output.
[Streaming Binary Bodies](./client-generation.md#streaming-binary-bodies) lists the
response bodies it affects and shows how to read one as a stream.

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

Without the flag, the generator preserves documentation text while converting escaped
newlines to line breaks, so a long description can remain on one line. With the flag,
the petstore's `CollarTelemetry` description is wrapped into multiple lines:

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
you omit the corresponding option, and the last column links to each option's
explanation. Run `oas3-gen generate --help` to see the command-line help, including
input, output, and terminal options.

| Flag | Default | Description | Details |
|------|---------|-------------|---------|
| `mode` | `types` | Generation mode: `types`, `client`, `client-mod`, `server-mod` | [Generation Modes](#generation-modes) |
| `-w, --workspace` | `false` | Emit a workspace-compatible `Cargo.toml` with sources in `src/`; requires `client-mod` or `server-mod` | [Workspace Crate Output](#workspace-crate-output) |
| `--module-version` | `0.0.0` | Version written to the generated `Cargo.toml` `[package]` table; requires `--workspace` | [Generated Manifest](#generated-manifest) |
| `-C, --visibility` | `public` | Item visibility: `public`, `crate`, `file` | [Visibility](#visibility) |
| `--enum-mode` | `merge` | Enum duplicate handling: `merge`, `preserve`, `relaxed` | [Enum Mode](./enum-types.md#enum-mode) |
| `--enum-layout` | `spec` | Variant ordering: `spec`, `sorted` | [Enum Layout](./enum-types.md#enum-layout) |
| `--no-helpers` | `false` | Disable enum constructor helpers | [Helper Methods](./enum-types.md#helper-methods) |
| `--odata-support` | `false` | Make `@odata.*` fields optional | [OData Support](./schema-types.md#odata-support) |
| `-c, --customize` | *(none)* | Custom `serde_with` adapter; repeatable | [Type Customization](./schema-types.md#type-customization) |
| `--fn-name` | *(none)* | Custom function name per operation (`ID=NAME`); repeatable | [Function Name Overrides](#function-name-overrides) |
| `--api-name` | *(none)* | Name for the generated client struct or server trait | [API Name Override](#api-name-override) |
| `--all-headers` | `false` | Emit header constants for all component-level headers | [Header Emission](#header-emission) |
| `--enable-builders` | `false` | Enable `bon` builder derives and methods | [Builder Generation](#builder-generation) |
| `--raw-body` | `false` | Return client binary response bodies as an unread `reqwest::Body` instead of `Vec<u8>` | [Raw Response Bodies](#raw-response-bodies) |
| `--no-ordered-collections` | `false` | Emit [`HashMap`][rustdoc-hashmap]/[`Vec`][rustdoc-vec] instead of `indexmap` collection types | [Ordering and Collections](./schema-types.md#ordering-and-collections) |
| `--doc-format` | `false` | Format documentation comments with `mdformat` | [Documentation Formatting](#documentation-formatting) |
| `--only` | *(none)* | Include only specified operations | [Operation Filtering](#operation-filtering) |
| `--exclude` | *(none)* | Exclude specified operations | [Operation Filtering](#operation-filtering) |
| `--all-schemas` | `false` | Generate all schemas regardless of usage | [Schema Filtering](#schema-filtering) |

[rustdoc-anyhow-result]: https://docs.rs/anyhow/1.0.104/anyhow/type.Result.html
[rustdoc-bon-builder]: https://docs.rs/bon/3.10.2/bon/derive.Builder.html
[rustdoc-futures-boxstream]: https://docs.rs/futures/0.3.34/futures/stream/type.BoxStream.html
[rustdoc-hashmap]: https://doc.rust-lang.org/std/collections/struct.HashMap.html
[rustdoc-http-header-constants]: https://docs.rs/http/1.5.0/http/header/index.html#constants
[rustdoc-http-header-map]: https://docs.rs/http/1.5.0/http/header/struct.HeaderMap.html
[rustdoc-http-header-name]: https://docs.rs/http/1.5.0/http/header/struct.HeaderName.html
[rustdoc-indexmap-map]: https://docs.rs/indexmap/2.14.2/indexmap/map/struct.IndexMap.html
[rustdoc-indexmap-set]: https://docs.rs/indexmap/2.14.2/indexmap/set/struct.IndexSet.html
[rustdoc-reqwest-body]: https://docs.rs/reqwest/0.13.5/reqwest/struct.Body.html
[rustdoc-serde-deserialize]: https://docs.rs/serde/1.0.229/serde/trait.Deserialize.html
[rustdoc-serde-json-value]: https://docs.rs/serde_json/1.0.154/serde_json/enum.Value.html
[rustdoc-serde-serialize]: https://docs.rs/serde/1.0.229/serde/trait.Serialize.html
[rustdoc-validate]: https://docs.rs/validator/0.21.0/validator/trait.Validate.html
[rustdoc-vec]: https://doc.rust-lang.org/std/vec/struct.Vec.html

# Client Generation

Use client generation when you want to call an API described by an OpenAPI
document. The generator creates a `reqwest` client, request types, and response
types. In this chapter, we'll generate a client, call an operation, and configure
API keys for operations that need them.

The commands run from a checkout of this repository and use its example
specifications. If you installed the tool, replace `cargo run --` with
`oas3-gen` and supply the path to your document. Rust excerpts assume you've
imported the generated types. Put snippets that use `?` in a function returning
[`anyhow::Result`][rustdoc-anyhow-result], and use an async function for calls that contain `.await`.

## Choosing the Output Layout

The `client` mode puts the client and its types in one file:

```bash
cargo run -- generate client \
  -i crates/oas3-gen/fixtures/petstore.json -o client.rs \
  --api-name PetStoreClient --enable-builders
```

Here, `--api-name` names the client `PetStoreClient`, and `--enable-builders`
adds the request builders we'll use below. Without a name override, the
petstore document's title produces `SwaggerPetstoreClient`.

Use `client-mod` if you'd like to keep the client and types in separate files:

```bash
cargo run -- generate client-mod \
  -i crates/oas3-gen/fixtures/petstore.json -o src/api/ \
  --api-name PetStoreClient --enable-builders
```

That command produces a module directory:

```text
src/api/
├── mod.rs
├── types.rs
└── client.rs
```

The `mod.rs` file declares both modules and re-exports their items:

```rust
mod types;
mod client;

pub use types::*;
pub use client::*;
```

To include this directory in your application, declare `mod api;` in your crate
root. You'll also need the dependencies referenced by the generated code. If
you'd prefer a separate crate with its own manifest, add `--workspace` and use
a crate directory as the output path. See
[Workspace Crate Output](./code-generation.md#workspace-crate-output) for the
layout and dependency rules.

## Creating a Client

The client stores a [`reqwest::Client`][rustdoc-reqwest-client] and a [`reqwest::Url`][rustdoc-reqwest-url] for the base URL.
You can call `new()` to use the first URL in the document's `servers` list. To choose a different
endpoint, call `with_base_url()`:

```rust
let client = PetStoreClient::with_base_url("https://api.example.com/v1")?;
```

This constructor returns a [`Result`][rustdoc-result] because parsing the URL or building the
HTTP client can fail. Replace the example URL with the API endpoint you want
to call.

If your application already has a configured `reqwest::Client`, pass it to
`PetStoreClient::with_client(base_url, http_client)`. For clients that use
cookie API keys, you'll also need to connect the generated cookie store, as
shown in [Using a Custom HTTP Client](#using-a-custom-http-client).

## Making a Request

Each operation has a request type. Path parameters, query parameters, and
headers are grouped into nested structs. With builders enabled, setters
assemble those structs for you. Let's request a pet by its ID:

```rust
let request = ShowPetByIdRequest::builder()
    .pet_id("pet-123".to_string())
    .x_api_version("2024-01-01".to_string())
    .build()?;

let response = client.show_pet_by_id(request).await?;
```

The builder checks the request's generated validation constraints. The client
method also validates the request before constructing and sending the HTTP
request. If validation, transport, or response decoding fails, the method
returns an error through its outer [`anyhow::Result`][rustdoc-anyhow-result].

You can also construct requests with struct literals. The
[Builder Pattern](./builders.md) chapter compares both forms and explains
which checks happen during compilation and which happen at runtime.

## Reading Responses

A decoded HTTP response is represented by `ApiResponse`. Its variants identify
HTTP statuses, while its type parameters hold the operation's success and
failure payloads. An API error response can therefore be a successfully decoded
`ApiResponse` value, even though the HTTP status indicates failure.

For `show_pet_by_id`, the success payload contains both a pet and response
headers. We can match the `response` from the previous example:

```rust
match response {
    ApiResponse::Ok(WithHeaders { headers, body: pet }) => {
        println!("Pet: {}", pet.name);
        println!("Cache status: {:?}", headers.x_cache);
    }
    ApiResponse::Unknown(status, error) => {
        eprintln!("{status}: {}", error.message);
    }
    other => anyhow::bail!("unexpected response: {other:?}"),
}
```

The `Ok` arm gives us the body and headers separately. In this specification,
`Unknown` represents the operation's `default` response. The wildcard arm
covers the remaining variants because operations share one response enum,
including variants that this operation doesn't return.

An optional response header becomes `None` when it's absent or can't be
parsed. For a list header, one invalid element makes the whole value `None`.
A missing or invalid required header causes decoding to return an error. See
[Responses](./code-generation.md#responses) for how the generator chooses
payload types, combines headers, and represents undeclared statuses.

### Decoding a Response from Your Own Transport

Client request types also provide a `parse_response()` associated function.
If you send the HTTP request yourself, you can pass the resulting
[`reqwest::Response`][rustdoc-reqwest-response] to the same decoder the generated client uses:

```rust
let response = ShowPetByIdRequest::parse_response(http_response).await?;
```

Here, `http_response` is the response you've already received. The decoder
reads declared headers before decoding the body and returns the operation's
`ApiResponse` type.

## Sending API Keys

For operations with `apiKey` security schemes, the client provides methods to
set credentials. See [API Key Security](./code-generation.md#api-key-security)
for how schemes and requirements are declared.

The petstore example doesn't declare API keys, so we'll use the Key Vault
fixture for the rest of this chapter. Generate it as a separate module:

```bash
cargo run -- generate client-mod \
  -i crates/oas3-gen/fixtures/api_key_security.json -o src/vault/ \
  --enable-builders
```

The document's title produces `KeyVaultClient`. Its schemes accept a header
key named `ApiKeyAuth`, a query key named `QueryKey`, and a cookie key named
`SessionCookie`. To configure the header key, read it from your application's
environment and pass it to the generated setter:

```rust
let api_key = std::env::var("API_KEY")?;
let client = KeyVaultClient::with_base_url("https://vault.example.com/v1")?
    .with_api_key_auth(api_key);
```

The client stores header and query keys as [`secrecy::SecretString`][rustdoc-secret-string] values
wrapped in [`Option`][rustdoc-option]. It gets one field and setter for each scheme used by the selected
operations. Names come from the scheme: `ApiKeyAuth` becomes `api_key_auth`
and `with_api_key_auth()`. If a name collides with another field, including
`client` or `base_url`, the generator adds a numeric suffix.

Each client method attaches the header and query keys you've configured that
its operation accepts. It doesn't enforce security requirements before sending
a request. If you leave a required key unset, the request goes out without it.

A generated server rejects missing required credentials with a plain-text
`401` response. If the operation declares a JSON body for `401`, the generated
client can't decode that plain-text body and returns a decoding error. Set the
required credentials before calling the operation; a service can separately
return a declared JSON `401` when it receives a key that isn't valid.

### Cookie Keys

Cookie keys use a shared [`reqwest_cookie_store::CookieStoreMutex`][rustdoc-cookie-store] in the
client's `cookies` field. The `new()` and `with_base_url()` constructors attach that store to the
underlying [`reqwest::Client`][rustdoc-reqwest-client]. To add the Key Vault session cookie, call its
setter:

```rust
let session_key = std::env::var("SESSION_KEY")?;
let client = KeyVaultClient::with_base_url("https://vault.example.com/v1")?
    .with_session_cookie(session_key);
```

The setter inserts a cookie for the base URL. Reqwest then sends it on requests
that match the cookie's scope, including requests to operations that don't
declare that scheme. The store also retains cookies received from the server.
Clones of the generated client share the store, so setting a cookie key on a
clone changes the store used by the original client too.

### Using a Custom HTTP Client

`with_client()` accepts an HTTP client that's already built, so it can't attach
the generated cookie store to it. To configure your own client with cookie
support, build it with the generated store and assign it to `client`:

```rust
use std::sync::Arc;

let mut api = KeyVaultClient::with_base_url("https://vault.example.com/v1")?;
api.client = reqwest::Client::builder()
    .cookie_provider(Arc::clone(&api.cookies))
    .build()?;

let api = api.with_session_cookie(std::env::var("SESSION_KEY")?);
```

The [`Arc`][rustdoc-arc] lets `api` and its HTTP client share the same store. Add any other
configuration to the [`reqwest::ClientBuilder`][rustdoc-reqwest-client-builder] before calling `build()`.

### Keeping Keys Out of Logs

Header and query keys use [`SecretString`][rustdoc-secret-string], whose debug output hides the value.
The generated client's debug output also omits the cookie store. Formatting
the generated client with `{:?}` therefore doesn't print these stored API keys.
See [Keeping Keys Out of Logs](./code-generation.md#keeping-keys-out-of-logs)
for the secret type's behavior when you explicitly read a key.

## Choosing Operations and Names

Use [`--only` or `--exclude`](./code-generation.md#operation-filtering) to select
operations. Use [`--fn-name`](./code-generation.md#function-name-overrides) to
rename methods and their request types, or
[`--api-name`](./code-generation.md#api-name-override) to name the client itself.
The [shared reference](./code-generation.md#flag-summary) also covers visibility,
enums, type adapters, and collection choices.

[rustdoc-anyhow-result]: https://docs.rs/anyhow/1.0.104/anyhow/type.Result.html
[rustdoc-arc]: https://doc.rust-lang.org/std/sync/struct.Arc.html
[rustdoc-cookie-store]: https://docs.rs/reqwest_cookie_store/0.10.0/reqwest_cookie_store/struct.CookieStoreMutex.html
[rustdoc-option]: https://doc.rust-lang.org/std/option/enum.Option.html
[rustdoc-reqwest-client]: https://docs.rs/reqwest/0.13.5/reqwest/struct.Client.html
[rustdoc-reqwest-client-builder]: https://docs.rs/reqwest/0.13.5/reqwest/struct.ClientBuilder.html
[rustdoc-reqwest-response]: https://docs.rs/reqwest/0.13.5/reqwest/struct.Response.html
[rustdoc-reqwest-url]: https://docs.rs/reqwest/0.13.5/reqwest/struct.Url.html
[rustdoc-result]: https://doc.rust-lang.org/std/result/enum.Result.html
[rustdoc-secret-string]: https://docs.rs/secrecy/0.10.3/secrecy/type.SecretString.html

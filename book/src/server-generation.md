# Server Generation

Use server generation when you want to implement an API with Axum. The
generator creates a trait for your service, request and response types, and a
router that connects HTTP requests to your implementation. In this chapter,
we'll implement an operation and then look at how API keys reach your service.

The commands run from a checkout of this repository. If you installed the
tool, replace `cargo run --` with `oas3-gen` and supply the path to your OpenAPI
document. Rust excerpts assume you've imported the generated types. They show
the relevant part of a service, not a complete application with a listening
socket and runtime.

## Generating a Server Module

The `server-mod` mode creates a directory containing the types, server trait,
and router. Let's generate the petstore's `listPets` operation:

```bash
cargo run -- generate server-mod \
  -i crates/oas3-gen/fixtures/petstore.json -o src/api/ \
  --only list_pets
```

The `--only` flag takes the normalized operation ID, `list_pets`, and keeps
this example to one operation. Omit it to generate all the document's
operations. The output directory contains:

```text
src/api/
├── mod.rs
├── types.rs
└── server.rs
```

The `mod.rs` file declares the modules and re-exports their items. Add
`mod api;` to your crate root to include this directory, and add the dependencies
referenced by the generated code to your manifest.

You can also generate a separate crate by adding `--workspace` and choosing
a crate directory as the output path. The generator then creates a
`Cargo.toml` and uses `src/lib.rs` as the module root. See
[Workspace Crate Output](./code-generation.md#workspace-crate-output) for details.

## Implementing the Service Trait

The generated `ApiServer` trait has one method per selected operation. For
our filtered petstore, it contains `list_pets()`:

```rust
pub trait ApiServer: Send + Sync {
    fn list_pets(
        &self,
        request: ListPetsRequest,
    ) -> impl std::future::Future<
        Output = anyhow::Result<ApiResponse<WithHeaders<XNextHeaders, Pets>, Error>>,
    > + Send;
}
```

The method returns a [`std::future::Future`][rustdoc-future] whose output is an [`anyhow::Result`][rustdoc-anyhow-result].
Your async implementation receives a request struct and returns a response
enum. For example, this service returns an empty collection of pets with no
next-page header:

```rust
#[derive(Clone)]
struct PetService;

impl ApiServer for PetService {
    async fn list_pets(
        &self,
        _request: ListPetsRequest,
    ) -> anyhow::Result<ApiResponse<WithHeaders<XNextHeaders, Pets>, Error>> {
        Ok(ApiResponse::Ok(WithHeaders {
            headers: XNextHeaders { x_next: None },
            body: Vec::new(),
        }))
    }
}
```

The outer `Ok` is the method's successful [`Result`][rustdoc-result]. The inner
`ApiResponse::Ok` selects HTTP status `200`. `WithHeaders` holds the response
headers and body together. Replace the empty [`Vec`][rustdoc-vec] with the pets your service
retrieves.

The trait requires [`Send`][rustdoc-send] + [`Sync`][rustdoc-sync], and the router also requires the service to
implement [`Clone`][rustdoc-clone]. The example derives `Clone` because its service has no state.
Choose how to share any application state when you add it to your service.

## Connecting the Router

Pass your service to the generated `router()` function:

```rust
let app = api::router(PetService);
```

Here, `api` is the module generated above, and `app` is an [`axum::Router`][rustdoc-axum-router] you
can use in your Axum application. For the selected petstore operation, the
generated function connects the route and stores your service as router state:

```rust
pub fn router<S>(service: S) -> Router
where
    S: ApiServer + Clone + Send + Sync + 'static,
{
    Router::new()
        .route("/{api_version}/pets", get(list_pets::<S>))
        .with_state(service)
}
```

This is an excerpt from `server.rs`; it omits the generated handler and imports.
The handler extracts the path, query, and header values, assembles a
`ListPetsRequest`, and calls your `list_pets()` method. For operations with API
keys, it extracts credentials first, as described in
[Receiving API Keys](#receiving-api-keys).

## Returning Responses

Your method returns the same response shapes described in
[Responses](./code-generation.md#responses). The generated handler converts
an `ApiResponse` variant into an HTTP status, body, and any declared headers.
An ordinary body is serialized as JSON; a [`()`][rustdoc-unit] or `None` body produces a
response without a body. `Other(status, body)`, when present, carries a raw
byte body and its status.

Use a declared response variant when the result is part of your API's contract,
such as an authorization failure. Returning `Err` from the service method
causes the handler to send status `500`. A response header value that can't be
represented as an HTTP header also produces `500`. Missing or malformed
required request headers produce `400` before your method runs.

## Receiving API Keys

For an operation that accepts API keys, the generated request has a
`credentials` field. Axum extracts its value before calling your service.
The [shared security rules](./code-generation.md#api-key-security) explain how
the document declares schemes and combines requirements.

The petstore has no API key schemes. To examine generated credentials, use
the Key Vault fixture instead:

```bash
cargo run -- generate server-mod \
  -i crates/oas3-gen/fixtures/api_key_security.json -o src/vault/ \
  --enable-builders
```

The rest of this chapter uses types from that module. The fixture's
`listSecrets` operation requires a header key named `ApiKeyAuth`. Its
credentials and request structs include these fields:

```rust
/// API keys accepted by the operation.
#[derive(Debug, Clone, oas3_gen_support::Default)]
pub struct ApiKeyAuthCredentials {
    /// The `ApiKeyAuth` key from the `X-Api-Key` header.
    pub api_key_auth: secrecy::SecretString,
}

pub struct ListSecretsRequest {
    pub query: ListSecretsRequestQuery,
    pub credentials: ApiKeyAuthCredentials,
}
```

A key required by every alternative uses [`SecretString`][rustdoc-secret-string]. A key that can be
omitted uses [`Option<SecretString>`][rustdoc-option]. For example, `createSecret` accepts either
a query key or a session cookie, so its credentials struct has two optional
fields:

```rust
/// API keys accepted by the operation.
#[derive(Debug, Clone, oas3_gen_support::Default)]
pub struct QueryKeyAndSessionCookieCredentials {
    pub query_key: Option<secrecy::SecretString>,
    pub session_cookie: Option<secrecy::SecretString>,
}
```

Neither field is required on every request, but the generated extractor
requires at least one of them to be present. Optional fields therefore don't
necessarily mean an operation allows anonymous access.

### Extracting Credentials

Each credentials struct implements Axum's [`FromRequestParts`][rustdoc-axum-from-request-parts]. An extractor
reads header, query, or cookie values before the handler reads the other
parameters or the body. Cookie keys are read with [`axum_extra::extract::CookieJar`][rustdoc-axum-cookie-jar].

When a required key is missing, or the supplied keys don't complete any
accepted alternative, Axum returns a plain-text `401` without reading the
body or calling your service. For the required header key above, the response
message is:

```text
missing API key in the `X-Api-Key` header
```

This response comes from the extractor, so it doesn't use the JSON error schema
that your operation might declare for `401`. The
[client chapter](./client-generation.md#sending-api-keys) explains how that
affects response decoding.

If a requirement includes a scheme the generator doesn't support, such as a
bearer token, it can't perform the complete presence check. All API key fields
for that operation become optional. Your application must enforce those
requirements; see [Requirements](./code-generation.md#requirements).

### Reading and Checking Keys

Passing extraction means that the required values were supplied. It doesn't
mean those values are valid credentials. Your service performs that check.
Inside the `list_secrets()` implementation, bring the [`secrecy::ExposeSecret`][rustdoc-expose-secret]
trait into scope and read the header key like this:

```rust
use secrecy::ExposeSecret;

let key: &str = request.credentials.api_key_auth.expose_secret();
```

Use `key` in your authorization check. If the key isn't recognized, you can
return the operation's declared `401` response from that method:

```rust
return Ok(ApiResponse::Unauthorized(Error {
    code: "invalid_api_key".to_string(),
    message: "The API key is not recognized.".to_string(),
}));
```

Here, `Error` is the generated Key Vault error type. The handler serializes it
as JSON, so a generated client can decode it as `ApiResponse::Unauthorized`.

Credentials structs don't derive [`PartialEq`][rustdoc-partial-eq] because [`SecretString`][rustdoc-secret-string] doesn't
implement it. Access a value through `expose_secret()` when you need to check
it. The secret type's debug output hides its value, but the exposed [`&str`][rustdoc-str] is
ordinary text. See [Keeping Keys Out of Logs](./code-generation.md#keeping-keys-out-of-logs).

### Shared Types and Field Names

Operations with the same security requirements share a credentials struct,
including when they list alternatives in a different order. Names come from
the schemes, such as `ApiKeyAuthCredentials` or
`QueryKeyAndSessionCookieCredentials`. A name that's already in use gets a
numeric suffix.

The request's `credentials` field also gets a suffix when that name conflicts
with an operation parameter. For example, it can become `credentials_2`.
Use the generated field name when reading the keys in your implementation.

### Constructing Requests in Tests

When builders are enabled, a server request builder accepts the whole
credentials struct. For example, a test can construct a `ListSecretsRequest`
without going through HTTP extraction:

```rust
let credentials = ApiKeyAuthCredentials {
    api_key_auth: secrecy::SecretString::from("example-test-key"),
};
let request = ListSecretsRequest::builder()
    .limit(10)
    .credentials(credentials)
    .build()?;
```

The builder assembles the query parameters and runs the request's generated
validation. It doesn't check whether the key is authorized. Calling a service
method with a constructed request also bypasses Axum's credential extractor,
so use HTTP requests when testing extraction and rejection behavior.

## Choosing Operations and Names

Use [`--only` or `--exclude`](./code-generation.md#operation-filtering) to select
which trait methods and routes to generate. Use
[`--fn-name`](./code-generation.md#function-name-overrides) to rename methods
and request types, or [`--api-name`](./code-generation.md#api-name-override) to
rename the service trait. The
[shared reference](./code-generation.md#flag-summary) covers the remaining
options, and [Builder Pattern](./builders.md) explains request construction.

[rustdoc-anyhow-result]: https://docs.rs/anyhow/1.0.104/anyhow/type.Result.html
[rustdoc-axum-cookie-jar]: https://docs.rs/axum-extra/0.12.6/axum_extra/extract/cookie/struct.CookieJar.html
[rustdoc-axum-from-request-parts]: https://docs.rs/axum/0.8.9/axum/extract/trait.FromRequestParts.html
[rustdoc-axum-router]: https://docs.rs/axum/0.8.9/axum/struct.Router.html
[rustdoc-clone]: https://doc.rust-lang.org/std/clone/trait.Clone.html
[rustdoc-expose-secret]: https://docs.rs/secrecy/0.10.3/secrecy/trait.ExposeSecret.html
[rustdoc-future]: https://doc.rust-lang.org/std/future/trait.Future.html
[rustdoc-option]: https://doc.rust-lang.org/std/option/enum.Option.html
[rustdoc-partial-eq]: https://doc.rust-lang.org/std/cmp/trait.PartialEq.html
[rustdoc-result]: https://doc.rust-lang.org/std/result/enum.Result.html
[rustdoc-secret-string]: https://docs.rs/secrecy/0.10.3/secrecy/type.SecretString.html
[rustdoc-send]: https://doc.rust-lang.org/std/marker/trait.Send.html
[rustdoc-str]: https://doc.rust-lang.org/std/primitive.str.html
[rustdoc-sync]: https://doc.rust-lang.org/std/marker/trait.Sync.html
[rustdoc-unit]: https://doc.rust-lang.org/std/primitive.unit.html
[rustdoc-vec]: https://doc.rust-lang.org/std/vec/struct.Vec.html

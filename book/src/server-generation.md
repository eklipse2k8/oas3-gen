# Server Generation

Server generation creates an Axum router, a service trait, and request and response
types. You implement the trait to handle each operation. We'll start with one petstore
operation, then follow a request through credential extraction, body decoding, and
response handling.

The commands run from a checkout of this repository and use the petstore fixture,
`fixtures/petstore.json` . If you installed the tool, replace `cargo run --` with
`oas3-gen` and supply the path to your OpenAPI document. Rust excerpts assume you've
imported the generated types. They show the relevant part of a service, not a complete
application with a listening socket and runtime.

- [Generating a Server Module](#generating-a-server-module)
- [Implementing the Service Trait](#implementing-the-service-trait)
- [Connecting the Router](#connecting-the-router)
- [Receiving Credentials](#receiving-credentials)
- [Receiving Cookie Parameters](#receiving-cookie-parameters)
- [Receiving Request Bodies](#receiving-request-bodies)
- [Returning Responses](#returning-responses)
- [Streaming Server-Sent Events](#streaming-server-sent-events)
- [Choosing Operations and Names](#choosing-operations-and-names)

## Generating a Server Module

The `server-mod` mode creates a directory containing the types, server trait, and
router. Let's generate the petstore's `listPets` operation:

```bash
cargo run -- generate server-mod \
  -i fixtures/petstore.json -o src/api/ \
  --only list_pets
```

The `--only` flag takes the normalized operation ID, `list_pets` , and keeps this
example to one operation. Omit it to generate all the document's operations. The output
directory contains:

```text
src/api/
├── mod.rs
├── types.rs
└── server.rs
```

The `mod.rs` file declares the modules and re-exports their items. Add `mod api;` to
your crate root to include this directory, and add the dependencies referenced by the
generated code to your manifest. [Dependencies](./generation-options.md#dependencies)
lists them.

### Generating a Separate Crate

You can also generate the server as its own crate by adding `--workspace` and choosing a
crate directory as the output path:

```bash
cargo run -- generate server-mod \
  -i fixtures/petstore.json -o petstore-server \
  --workspace
```

The generator creates a `Cargo.toml` and uses `src/lib.rs` as the library root:

```text
petstore-server/
├── Cargo.toml
└── src/
    ├── lib.rs
    ├── types.rs
    └── server.rs
```

The manifest's dependencies follow what the generated server uses. In place of a
client's `reqwest`, `reqwest_cookie_store`, and `serde_html_form`, it declares the Axum
crates for the router and extractors. The petstore server also declares `futures`
because it streams Server-Sent Events:

```toml
axum = { version = "0.8.5", features = ["multipart"] }
axum-extra = { version = "0.12", features = ["cookie", "query"] }
futures = "0.3"
```

[Dependencies](./generation-options.md#dependencies) explains when the generator declares
each crate. The generator rewrites `Cargo.toml` on every run, so keep your service
implementation in a separate crate that depends on this one.
[Generating a Separate Crate](./client-generation.md#generating-a-separate-crate) in
the client chapter shows a complete manifest and how to add the crate to a workspace.

## Implementing the Service Trait

The generated `ApiServer` trait has one method per selected operation. For our filtered
petstore, it contains `list_pets()` :

```rust
pub trait ApiServer: Send + Sync {
    /// List pets
    ///
    /// Lists pets one page at a time. Anonymous callers may list pets; an API key is optional.
    ///
    /// * Path: `GET /pets`
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
}
```

The method returns a [`std::future::Future`][rustdoc-future] whose output is an
[`anyhow::Result`][rustdoc-anyhow-result] . Your async implementation receives a request
struct and returns a response enum. The `200` response of `listPets` declares three
headers, `x-next` , `X-Total-Count` , and `X-Request-Id` , so the success payload pairs
the `Pets` body with a struct named after those headers. For example, this service
returns an empty page of pets with a total count of zero and no next-page link:

```rust
#[derive(Clone)]
struct PetService;

impl ApiServer for PetService {
    async fn list_pets(
        &self,
        _request: ListPetsRequest,
    ) -> anyhow::Result<
        ApiResponse<WithHeaders<XNextAndXTotalCountAndXRequestIdHeaders, Pets>, Error>,
    > {
        Ok(ApiResponse::Ok(WithHeaders {
            headers: XNextAndXTotalCountAndXRequestIdHeaders {
                x_next: None,
                x_total_count: Some(0),
                x_request_id: None,
            },
            body: vec![],
        }))
    }
}
```

The outer `Ok` is the method's successful
[`Result`][rustdoc-result]. The inner `ApiResponse::Ok` selects HTTP status `200`. `WithHeaders` holds the response headers and body together; [Response Headers](./http.md#response-headers)
explains how the generator names the header struct. Replace the empty
[`Vec`][rustdoc-vec] with the pets your service retrieves.

The service must implement [`Send`][rustdoc-send] , [`Sync`][rustdoc-sync] , and
[`Clone`][rustdoc-clone] to work with the router. This example has no state and derives
`Clone` . When you add state, decide which values each clone should own and which it
should share.

The generated handlers don't call `validate()` on a request. To check incoming values
against the document's constraints, call it in your service method with
[`validator::Validate`][rustdoc-validate] in scope. A trait method for an operation
marked `deprecated: true` has no `#[deprecated]` attribute.

## Connecting the Router

Pass your service to the generated `router()` function:

```rust
let app = api::router(PetService);
```

Here, `api` is the module generated above, and `app` is an
[`axum::Router`][rustdoc-axum-router] you can use in your Axum application. For the
selected petstore operation, the generated function connects the route and stores your
service as router state:

```rust
pub fn router<S>(service: S) -> Router
where
    S: ApiServer + Clone + Send + Sync + 'static,
{
    Router::new()
        .route("/pets", axum::routing::get(list_pets::<S>))
        .with_state(service)
}
```

This is an excerpt from `server.rs` ; it omits the generated handler and imports. The
handler extracts the credentials, query parameters, and headers, assembles a
`ListPetsRequest` , and calls your `list_pets()` method. Credential extraction is
described in [Receiving Credentials](#receiving-credentials) .

The handler reads query parameters with
[`axum_extra::extract::Query`][rustdoc-axum-extra-query] , which collects a repeated
name, such as `tags=calm&tags=shy` , into an array. With `--workspace` , the generated
manifest enables axum-extra's `query` feature. With module output, enable it in your own
`axum-extra` dependency.

The sections below use other petstore operations. To follow those examples, generate the
full module by running the command above without `--only` .

## Receiving Credentials

For an operation that accepts API keys or bearer tokens, the generated request has a
`credentials` field. Axum extracts its value before calling your service. The
[shared security rules](./http.md#security-schemes) explain how the document declares
schemes and combines requirements.

The petstore's top-level `security` requires a header key named `ApiKeyAuth` , and its
staff key operations use the other combinations. To examine their credentials, generate
those operations into a separate module:

```bash
cargo run -- generate server-mod \
  -i fixtures/petstore.json -o src/staff/ \
  --only list_api_keys,create_api_key,get_api_key,revoke_api_key,replace_api_key,list_audit_events \
  --enable-builders
```

The examples in this section use types from that module. The `listApiKeys` operation
inherits the top-level requirement. Its credentials and request structs include these
fields; the request excerpt omits its attributes:

```rust
/// Credentials an operation accepts, read from the request before the operation runs.
#[derive(Debug, Clone, oas3_gen_support::Default)]
pub struct ApiKeyAuthCredentials {
    /// Key issued in the Petstore partner dashboard.
    ///
    /// `ApiKeyAuth` API key, read from the `X-Api-Key` header.
    pub api_key_auth: secrecy::SecretString,
}

pub struct ListApiKeysRequest {
    pub query: ListApiKeysRequestQuery,
    pub credentials: ApiKeyAuthCredentials,
}
```

A key required by every alternative uses [`SecretString`][rustdoc-secret-string] . A key
that can be omitted uses [`Option<SecretString>`][rustdoc-option] . For example,
`createApiKey` accepts either a query key or a session cookie, so its credentials struct
has two optional fields:

```rust
/// Credentials an operation accepts, read from the request before the operation runs.
#[derive(Debug, Clone, oas3_gen_support::Default)]
pub struct QueryKeyAndSessionCookieCredentials {
    /// `QueryKey` API key, read from the `api_key` query parameter.
    pub query_key: Option<secrecy::SecretString>,
    /// `SessionCookie` API key, read from the `session` cookie.
    pub session_cookie: Option<secrecy::SecretString>,
}
```

The extractor requires at least one of these optional fields to be present. To allow a
request without credentials, the operation must list an empty alternative. The
`listPets` operation from the start of this chapter declares `[{}, {"ApiKeyAuth": []}]`
, so its credentials hold an `Option<SecretString>` , and a request without a key still
reaches your service.

### Extracting Credentials

Each credentials struct implements Axum's
[`FromRequestParts`][rustdoc-axum-from-request-parts] . An extractor reads header,
query, or cookie values before the handler reads the other parameters or the body.

Cookie keys are parsed from the `Cookie` header with axum-extra's cookie support. With
module output, enable the `cookie` feature of your `axum-extra` dependency. A cookie key
is percent-decoded like a cookie parameter, so a value sent as `abc%2Bdef` reaches your
service as `abc+def` . A value that isn't valid percent-encoded UTF-8, such as `abc%ff`
, counts as missing.

A bearer token comes from the `Authorization` header. The extractor accepts the `Bearer`
prefix in any case and stores the token that follows it. An `Authorization` header with
a different scheme, such as `Basic` , doesn't supply a bearer token. An `apiKey` scheme
named `Authorization` reads the whole header value instead, so when an operation accepts
both kinds, one header can fill both fields. For `listAuditEvents` , a request with
`Authorization: Bearer t1` sets `bearer_auth` to `t1` and `token_auth` to `Bearer t1` .

When a required key is missing, or the supplied keys don't complete any accepted
alternative, Axum returns a plain-text `401` without reading the body or calling your
service. For the required header key of `listApiKeys` , the response message is:

```text
missing API key in the `X-Api-Key` header
```

The `revokeApiKey` operation accepts a bearer token or the header key, so its
`ApiKeyAuthAndBearerAuthCredentials` struct has two optional fields. A request that
supplies neither receives this message:

```text
missing credentials for `ApiKeyAuth` or `BearerAuth`
```

This response comes from the extractor, so it doesn't use the JSON error schema that
your operation might declare for `401` . The
[client chapter](./client-generation.md#rejections-from-a-generated-server) explains how
that affects response decoding.

If a requirement includes a scheme the generator doesn't support, such as HTTP basic
authentication, it can't perform the complete presence check. All credential fields for
that operation become optional. For example, `replaceApiKey` accepts `BasicAuth` or
`ApiKeyAuth` , so a request without the header key still reaches your service. Your
application must enforce those requirements; see [Requirements](./http.md#requirements)
.

### Reading and Checking Keys

The extractor checks for required credentials. Your service checks whether the supplied
values are valid and authorize the request. Inside the `list_api_keys()` implementation,
bring the [`secrecy::ExposeSecret`][rustdoc-expose-secret] trait into scope and read the
header key like this:

```rust
use secrecy::ExposeSecret;

let key: &str = request.credentials.api_key_auth.expose_secret();
```

Use `key` in your authorization check. If the key isn't recognized, you can return the
operation's declared `401` response from that method:

```rust
return Ok(ApiResponse::Unauthorized(Error {
    code: 401,
    message: "The API key is not recognized.".to_string(),
}));
```

Here, `Error` is the petstore's generated error type. The handler serializes it as JSON,
so a generated client can decode it as `ApiResponse::Unauthorized` .

Credentials structs don't derive
[`PartialEq`][rustdoc-partial-eq] because [`SecretString`][rustdoc-secret-string] doesn't implement it. Access a value through `expose_secret()` when you need to check it. The secret type's debug output hides its value, but the exposed [`&str`][rustdoc-str] is ordinary text. See [Keeping Keys Out of Logs](./http.md#keeping-keys-out-of-logs)
.

### Shared Types and Field Names

Operations with the same security requirements share a credentials struct, including
when they list alternatives in a different order. Names come from the schemes, such as
`ApiKeyAuthCredentials` or `QueryKeyAndSessionCookieCredentials` . A name that's already
in use gets a numeric suffix. In the staff module, `replaceApiKey` , whose header key is
optional, gets `ApiKeyAuthCredentials2` .

The request's `credentials` field also gets a suffix when that name conflicts with an
operation parameter. For example, it can become `credentials_2` . Use the generated
field name when reading the keys in your implementation.

### Constructing Requests in Tests

When builders are enabled, a server request builder accepts the whole credentials
struct. For example, a test can construct a `ListApiKeysRequest` without going through
HTTP extraction:

```rust
let credentials = ApiKeyAuthCredentials {
    api_key_auth: secrecy::SecretString::from("example-test-key"),
};
let request = ListApiKeysRequest::builder()
    .limit(10)
    .credentials(credentials)
    .build()?;
```

The builder assembles the query parameters and runs the request's generated validation.
It doesn't check whether the key is authorized. Calling a service method with a
constructed request also bypasses Axum's credential extractor, so use HTTP requests when
testing extraction and rejection behavior.

## Receiving Cookie Parameters

An operation's cookie parameters arrive in the request's `cookie` field. The handler
parses them from the `Cookie` header and percent-decodes each value before calling your
service. A value that isn't valid percent-encoded UTF-8 counts as missing. An array
cookie with no items, sent as `name=` , arrives as an empty list. The petstore's
`getPetCard` operation declares two optional cookies, so your `get_pet_card()` method
reads `request.cookie.card_theme` and `request.cookie.recently_viewed` .

A missing required cookie or a value that doesn't parse as its type produces status
`400` with a plain-text reason. The handler doesn't call your method. The `conversation`
cookie that `listAssistantEvents` requires produces the first message when it's missing,
and a `cardTheme` outside its enum produces the second:

```text
Bad request: missing required cookie `conversation`
Bad request: invalid value for cookie `cardTheme`
```

With module output, enable the `cookie` feature of your `axum-extra` dependency.

## Receiving Request Bodies

When an operation declares a request body, the generated request struct has a `body`
field, and the handler fills it before calling your service. The field's type follows
the media type the operation declares:

| Declared media type | `body` field |
|---|---|
| `application/json`, or a `+json` type such as `application/merge-patch+json` | The generated body type, decoded from JSON |
| `application/x-www-form-urlencoded` | The generated body type, decoded from the form |
| `text/*`, such as `text/plain` | A [`String`][rustdoc-string] |
| A binary type, such as `image/png` | A [`Vec<u8>`][rustdoc-vec] |
| `multipart/form-data` | The generated body type; see [Receiving Multipart Bodies](#receiving-multipart-bodies) |

For example, `setPetNotes` declares a `text/plain` body, so the notes arrive as a
`String` in the `body` field of `SetPetNotesRequest` . `replacePetPhoto` declares an
`image/png` body, so its photo arrives as bytes. When a text or binary body is optional,
the field is an [`Option`][rustdoc-option] , and a request with an empty body gives you
`None` . An optional form body, such as the one `unsubscribeNewsletter` declares, is
`None` when the request has no `Content-Type` .

A JSON or form body that can't be decoded never reaches your method. If the body isn't
valid JSON, doesn't match the body type, or arrives with another `Content-Type` , the
handler responds with status `400` and a plain-text reason that starts with
`Bad request: ` , followed by Axum's description of the problem. A form body that
doesn't match its type is answered the same way.

### Receiving Multipart Bodies

When an operation's request body uses `multipart/form-data` , the generated handler
reads the parts back into the body struct before calling your service.
[Multipart Form Bodies](./http.md#multipart-form-bodies) describes which part each
property becomes.

The generated server extracts the body with Axum's
[`Multipart`][rustdoc-axum-multipart] extractor, which needs Axum 0.8.5 or later with the `multipart` feature. With `--workspace`, the generated manifest declares both. With module output, add the feature to your own `axum` dependency. Each handler contains a function that reads the parts. Text parts are converted by the rules in [Choosing the Part for Each Property](./http.md#choosing-the-part-for-each-property)
and collected into a JSON object. The function then deserializes the struct from that
object in one step, so renamed fields, defaults, and `#[serde_as]` adapters behave as
they do for JSON bodies. File parts skip that step: their bytes are assigned to the
struct afterward.

A missing required scalar part or a part whose text doesn't match its type produces
status `400` before your service runs. For example, a `count` part in the petstore's
`submitIntakeForm` body containing `three` produces ``Bad request: invalid multipart
part `count` : expected ident at line 1 column 2``. Arrays have different rules because
an empty array sends no parts. A missing required array becomes an empty array. An
optional array becomes `None` whether the sender omitted it or supplied an empty array.
Null array items are dropped.

When the multipart body itself is optional, as in the petstore's `uploadMedia`
operation, the request's `body` field is an [`Option`][rustdoc-option] . A request
without a `Content-Type` header has no body, and your service receives `None` . A
request that sends a body with another `Content-Type` , such as `application/json` ,
receives `400` with the reason `` Bad request: the body must be `multipart/form-data`
``.

Axum limits request bodies to 2 MB by default. A larger upload fails while the parts are
read, and the handler answers `400` . To accept larger files, add
[`DefaultBodyLimit`][rustdoc-axum-default-body-limit] as a layer on the router.

## Returning Responses

Your method returns the same response shapes described in
[Responses](./http.md#responses) . The generated handler converts an `ApiResponse`
variant into an HTTP status, body, and any declared headers. A [`()`][rustdoc-unit] or
`None` body produces a response without a body. `Other(status, body)` , when present,
carries a raw byte body and its status.

The handler sends each body in the media type that its status declares, so you return
Rust values and the handler chooses the encoding:

| Declared media type | Sent as |
|---|---|
| `application/json` | JSON, serialized with [`axum::Json`][rustdoc-axum-json] |
| A `+json` type, such as `application/problem+json` | JSON, labelled with the declared `Content-Type` |
| `text/*`, such as `text/plain` | Text, labelled with the declared `Content-Type`, when the body is a string, byte vector, number, boolean, value enum, or named string schema. Any other body is sent as JSON, labelled with the declared text type when the status declares no JSON type |
| A binary type, such as `image/png` or `application/octet-stream` | The raw bytes, labelled with the declared `Content-Type` and no `charset`, when the body is `Vec<u8>` or a string. Any other body is sent as JSON |
| `text/event-stream` | A stream of events; see [Streaming Server-Sent Events](#streaming-server-sent-events) |

A wildcard such as `image/*` or `text/*` names a range of types, so the handler labels
the body with a concrete one instead: `application/octet-stream` for binary and
`text/plain` for text. A `*/*` declares a JSON body, which is sent as `application/json`
. The handler sends Rust strings as UTF-8, so a `charset` other than UTF-8 is replaced
with `utf-8` . A binary label carries no `charset` .

The petstore's `updatePet` operation declares a `Pet` as `application/json` for `200` ,
and a `Problem` as `application/problem+json` for `404` and `422` . When the pet doesn't
exist, your `update_pet()` method can return:

```rust
return Ok(ApiResponse::NotFound(Problem {
    title: Some("Pet not found".to_string()),
    status: Some(404),
    ..Problem::default()
}));
```

The handler serializes the `Problem` as JSON and sends it with
`Content-Type: application/problem+json` . Likewise, `getPetCard` returns a `String`
that's sent as `text/plain` , and `getPetPhoto` returns a [`Vec<u8>`][rustdoc-vec]
that's sent as `image/png` .

You can also return a status that the operation doesn't declare, such as
`ApiResponse::Conflict` from `update_pet()` . The handler then uses the first media type
the operation declares for the same payload: `application/json` for a `Pet` and
`application/problem+json` for a `Problem` . A payload that combines several body types
into a union, such as `ErrorOrProblem` for `showPetById` , sends each member under the
media type it was declared with: `Error` as `application/json` and `Problem` as
`application/problem+json` . A status that offers both JSON and `text/event-stream`
works the same way, sending its stream member as events.

Use a declared response variant when the result is part of your API's contract, such as
an authorization failure. Returning `Err` from the service method causes the handler to
send status `500` . A response header value that can't be represented as an HTTP header
also produces `500` .

Some requests never reach your method. The handler answers these itself, with a
plain-text reason instead of a body from the document's error schemas:

- A missing or malformed required request header, a JSON or form body that
  doesn't match its type, and a `multipart/form-data` body that the handler
  can't read all produce `400`. The reason starts with `Bad request: `. See
  [Receiving Request Bodies](#receiving-request-bodies) and
  [Receiving Multipart Bodies](#receiving-multipart-bodies).
- A path or query value that doesn't match its type also produces `400`, from
  the Axum extractor that reads it.
- A missing required cookie parameter, or a cookie value that doesn't parse as
  its type, produces `400`. See
  [Receiving Cookie Parameters](#receiving-cookie-parameters).
- Missing credentials produce `401`, as described in
  [Extracting Credentials](#extracting-credentials).

Error responses returned by your service use the document's declared bodies.
[Rejections from a Generated Server](./client-generation.md#rejections-from-a-generated-server)
shows how a generated client reports the handler's rejections.

## Streaming Server-Sent Events

Some operations send a stream of events rather than one body. OpenAPI describes this
with a `text/event-stream` response, the media type of [Server-Sent Events][mdn-sse]
(SSE). The petstore's `streamPetVitals` operation uses one to send live smart-collar
readings:

```json
"responses": {
  "200": {
    "description": "A stream of vitals readings",
    "content": {
      "text/event-stream": {
        "schema": { "$ref": "#/components/schemas/VitalsReading" }
      }
    }
  }
}
```

The schema describes one event. The generated trait method returns a
[`BoxStream`][rustdoc-futures-boxstream] of `VitalsReading` values:

```rust
/// Stream live smart-collar vitals as server-sent events
///
/// * Path: `GET /pets/{petId}/vitals`
fn stream_pet_vitals(
    &self,
    request: StreamPetVitalsRequest,
) -> impl std::future::Future<
    Output = anyhow::Result<
        ApiResponse<futures::stream::BoxStream<'static, VitalsReading>, ()>,
    >,
> + Send;
```

`BoxStream` comes from the `futures` crate. When the generator writes a `Cargo.toml` ,
it declares `futures` , and your own crate needs it too so your service can build the
stream. When you already have every event, turn them into a stream with
[`stream::iter()`][rustdoc-futures-iter] and box it with
[`StreamExt::boxed()`][rustdoc-futures-boxed] . This body of `stream_pet_vitals()` sends
one reading and then ends the stream:

```rust
use futures::StreamExt;

let reading = VitalsReading {
    id: "reading-1".to_string(),
    heart_rate_bpm: 92,
    ..VitalsReading::default()
};
Ok(ApiResponse::Ok(futures::stream::iter([reading]).boxed()))
```

When events arrive over time, box any [`Stream`][rustdoc-futures-stream] that's
`Send + 'static` . For example, the receiving half of a
[`futures::channel::mpsc`][rustdoc-futures-mpsc] channel is a stream:

```rust
use futures::StreamExt;

let (sender, receiver) = futures::channel::mpsc::channel(16);
tokio::spawn(forward_readings(request.path.pet_id, sender));
Ok(ApiResponse::Ok(receiver.boxed()))
```

Here, `forward_readings` stands for your own async function that sends each new reading
through `sender` , and [`tokio::spawn`][rustdoc-tokio-spawn] runs it alongside the
response. The response stays open as long as the channel does. When `forward_readings`
returns and drops the sender, the stream ends, and so does the response.

The generated handler wraps the stream in an Axum [`Sse`][rustdoc-axum-sse] response.
Each item becomes the JSON `data` of one [`Event`][rustdoc-axum-sse-event] , in order:

```rust
Ok(ApiResponse::Ok(body)) => (
    http::StatusCode::OK,
    axum::response::sse::Sse::new(futures::StreamExt::map(body, |item| {
        axum::response::sse::Event::default().json_data(item)
    })),
)
    .into_response(),
```

This is an excerpt from the `stream_pet_vitals` handler in `server.rs` . An item that
can't be serialized ends the response.

### Sending Whole Frames

When the `text/event-stream` schema is a frame type, as described in
[Reading Whole Frames](./client-generation.md#reading-whole-frames) , your method
returns values of that type, and the handler sends each one as a whole event. The
petstore's `stream_assistant_message()` returns `AssistantEventFrame` values in the
`AssistantEventFrameStream` member of its payload union.

For each frame type, the generator implements
`TryFrom<AssistantEventFrame> for axum::response::sse::Event` in `types.rs` . The
conversion writes the item's `data` as JSON, or as text when the schema declares a plain
`string` , and sets the event's name, ID, and retry time from the `event` , `id` , and
`retry` properties that hold a value. A union of frame types, such as
`AssistantTokenFrame` , converts whichever member it holds, and sends a member that has
no `event` property under its discriminator value. The handler maps each item through
that conversion:

```rust
axum::response::sse::Sse::new(futures::StreamExt::chain(
    futures::StreamExt::map(body, axum::response::sse::Event::try_from),
    futures::stream::iter([Ok(axum::response::sse::Event::default().data("[DONE]"))]),
))
```

This is an excerpt from the `stream_assistant_message` handler. Because the operation
declares the sentinel `[DONE]` , through `x-speakeasy-sse-sentinel` , the handler sends
`data: [DONE]` after your last item. `x-sse-sentinel` works the same way. An `event` or
`id` whose text has a line break, an `id` with a NUL character, or a `retry` that isn't
a whole, non-negative number of milliseconds can't be sent, so the conversion returns an
error and the response ends.

## Choosing Operations and Names

In [Generating a Server Module](#generating-a-server-module), `--only list_pets` limited
the trait to one method. [`--only` and `--exclude`](./generation-options.md#operation-filtering)
select which trait methods and routes the generator creates. Both take normalized
`snake_case` operation IDs, so `--exclude upload_pet_image` keeps every operation except
the image upload.

A callback follows the operation that declares it. The petstore's `adoptPet` operation
declares a `statusChanged` callback, which the server generates as the
`adopt_pet_status_changed` trait method and route. `--only adopt_pet` keeps that
callback's route and trait method, and `--exclude adopt_pet` removes them.

You can also name a callback by its own normalized `snake_case` ID.
`--only adopt_pet_status_changed` keeps only the callback, and
`--exclude adopt_pet_status_changed` removes only the callback while `adopt_pet` stays.

The service trait is named `ApiServer` by default. To choose another name, pass
[`--api-name`](./generation-options.md#api-name-override):

```bash
cargo run -- generate server-mod \
  -i fixtures/petstore.json -o src/api/ \
  --api-name PetStoreApi
```

The trait is named `PetStoreApi`, and the generated router uses that name in its trait
bound. Changing the trait's name doesn't rename its methods. To rename a method, pass
[`--fn-name`](./generation-options.md#function-name-overrides) with the operation's
`operationId` and the new name. For example, `--fn-name listPets=fetch_all_pets` renames
`list_pets()` to `fetch_all_pets()` and its request type to `FetchAllPetsRequest`;
implement the trait with the new names.

The [Flag Summary](./generation-options.md#flag-summary) lists the remaining options,
and [Builder Pattern](./builders.md) explains request construction.

[mdn-sse]: https://developer.mozilla.org/en-US/docs/Web/API/Server-sent_events
[rustdoc-anyhow-result]: https://docs.rs/anyhow/1.0.104/anyhow/type.Result.html
[rustdoc-axum-default-body-limit]: https://docs.rs/axum/0.8.9/axum/extract/struct.DefaultBodyLimit.html
[rustdoc-axum-extra-query]: https://docs.rs/axum-extra/0.12.6/axum_extra/extract/struct.Query.html
[rustdoc-axum-from-request-parts]: https://docs.rs/axum/0.8.9/axum/extract/trait.FromRequestParts.html
[rustdoc-axum-json]: https://docs.rs/axum/0.8.9/axum/struct.Json.html
[rustdoc-axum-multipart]: https://docs.rs/axum/0.8.9/axum/extract/struct.Multipart.html
[rustdoc-axum-router]: https://docs.rs/axum/0.8.9/axum/struct.Router.html
[rustdoc-axum-sse]: https://docs.rs/axum/0.8.9/axum/response/sse/struct.Sse.html
[rustdoc-axum-sse-event]: https://docs.rs/axum/0.8.9/axum/response/sse/struct.Event.html
[rustdoc-clone]: https://doc.rust-lang.org/std/clone/trait.Clone.html
[rustdoc-expose-secret]: https://docs.rs/secrecy/0.10.3/secrecy/trait.ExposeSecret.html
[rustdoc-future]: https://doc.rust-lang.org/std/future/trait.Future.html
[rustdoc-futures-boxed]: https://docs.rs/futures/0.3.34/futures/stream/trait.StreamExt.html#method.boxed
[rustdoc-futures-boxstream]: https://docs.rs/futures/0.3.34/futures/stream/type.BoxStream.html
[rustdoc-futures-iter]: https://docs.rs/futures/0.3.34/futures/stream/fn.iter.html
[rustdoc-futures-mpsc]: https://docs.rs/futures/0.3.34/futures/channel/mpsc/index.html
[rustdoc-futures-stream]: https://docs.rs/futures/0.3.34/futures/stream/trait.Stream.html
[rustdoc-option]: https://doc.rust-lang.org/std/option/enum.Option.html
[rustdoc-partial-eq]: https://doc.rust-lang.org/std/cmp/trait.PartialEq.html
[rustdoc-result]: https://doc.rust-lang.org/std/result/enum.Result.html
[rustdoc-secret-string]: https://docs.rs/secrecy/0.10.3/secrecy/type.SecretString.html
[rustdoc-send]: https://doc.rust-lang.org/std/marker/trait.Send.html
[rustdoc-str]: https://doc.rust-lang.org/std/primitive.str.html
[rustdoc-string]: https://doc.rust-lang.org/std/string/struct.String.html
[rustdoc-sync]: https://doc.rust-lang.org/std/marker/trait.Sync.html
[rustdoc-tokio-spawn]: https://docs.rs/tokio/1.53.2/tokio/task/fn.spawn.html
[rustdoc-unit]: https://doc.rust-lang.org/std/primitive.unit.html
[rustdoc-validate]: https://docs.rs/validator/0.21.0/validator/trait.Validate.html
[rustdoc-vec]: https://doc.rust-lang.org/std/vec/struct.Vec.html

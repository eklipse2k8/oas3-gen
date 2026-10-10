# Client Generation

Use client generation when you want to call an API described by an OpenAPI
document. The generator creates a `reqwest` client, request types, and response
types. In this chapter, we'll generate a client, call an operation, read its
responses and event streams, send request bodies, and configure API keys and
bearer tokens for operations that need them.

The commands run from a checkout of this repository and use the petstore
fixture, `crates/oas3-gen/fixtures/petstore.json`. If you installed the tool,
replace `cargo run --` with `oas3-gen` and supply the path to your document.
Rust excerpts assume you've imported the generated types. Put snippets that use
`?` in a function returning [`anyhow::Result`][rustdoc-anyhow-result], and use
an async function for calls that contain `.await`.

## Choosing the Output Layout

The `client` mode puts the client and its types in one file:

```bash
cargo run -- generate client \
  -i crates/oas3-gen/fixtures/petstore.json -o client.rs \
  --enable-builders
```

Here, `--enable-builders` adds the request builders we'll use below. The
client's name comes from the document's title, `Petstore`, so the generated
client is `PetstoreClient`. To choose a different name, add
[`--api-name`](./code-generation.md#api-name-override).

Use `client-mod` if you'd like to keep the client and types in separate files:

```bash
cargo run -- generate client-mod \
  -i crates/oas3-gen/fixtures/petstore.json -o src/api/ \
  --enable-builders
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
You can call `new()` to use the first URL in the document's `servers` list,
`https://api.petstore.example/v3`. To choose a different endpoint, such as one
of the petstore's regional deployments, call `with_base_url()`:

```rust
let client = PetstoreClient::with_base_url("https://eu-central.api.petstore.example/v3")?;
```

This constructor returns a [`Result`][rustdoc-result] because parsing the URL or building the
HTTP client can fail. Replace the example URL with the API endpoint you want
to call.

If your application already has a configured `reqwest::Client`, pass it to
`PetstoreClient::with_client(base_url, http_client)`. For clients that use
cookie API keys, you'll also need to connect the generated cookie store, as
shown in [Using a Custom HTTP Client](#using-a-custom-http-client).

## Making a Request

Each operation has a request type. Path, query, header, and cookie parameters
are grouped into nested structs. With builders enabled, setters
assemble those structs for you. Let's request a pet by its ID:

```rust
let pet_id = uuid::Uuid::parse_str("0192f0c1-7a6e-7c4e-9d2a-5b1e8f3c2a10")?;
let request = ShowPetByIdRequest::builder()
    .pet_id(pet_id)
    .x_api_version("2026-10".to_string())
    .build()?;

let response = client.show_pet_by_id(request).await?;
```

The `petId` path parameter has `format: uuid`, so its setter takes a
[`uuid::Uuid`][rustdoc-uuid]. The optional `If-None-Match` header has no setter
call here, so the request leaves it out.

The builder checks the request's generated validation constraints. The client
method also validates the request before constructing and sending the HTTP
request. If validation, transport, or response decoding fails, the method
returns an error through its outer [`anyhow::Result`][rustdoc-anyhow-result].

You can also construct requests with struct literals. The
[Builder Pattern](./builders.md) chapter compares both forms and explains
which checks happen during compilation and which happen at runtime.

Every HTTP method that OpenAPI allows gets a client method, including `HEAD`,
`OPTIONS`, and `TRACE`. For example, the petstore's `describePets` operation is
`OPTIONS /pets`, and its `204` response lists the allowed methods in an `Allow`
header:

```rust
if let ApiResponse::NoContent(WithHeaders { headers, .. }) =
    client.describe_pets(DescribePetsRequest::default()).await?
{
    println!("Allowed: {}", headers.allow);
}
```

### Query Arrays

When a query parameter is an array, the client sends it according to the
parameter's `style` and `explode` fields. The petstore's `searchPets` operation
has both kinds. This excerpt shows two of its parameters:

```json
{
  "name": "colors",
  "in": "query",
  "style": "pipeDelimited",
  "explode": false,
  "schema": { "type": "array", "items": { "type": "string" } }
},
{
  "name": "tags",
  "in": "query",
  "schema": { "type": "array", "items": { "type": "string" } }
}
```

Let's search for pets with two tags and two coat colors:

```rust
let request = SearchPetsRequest::builder()
    .q("golden retriever".to_string())
    .tags(vec!["calm".to_string(), "shy".to_string()])
    .colors(vec!["gold".to_string(), "cream".to_string()])
    .build()?;

let response = client.search_pets(request).await?;
```

The client sends this query string:

```text
q=golden+retriever&colors=gold%7Ccream&tags=calm&tags=shy
```

The `tags` parameter doesn't set `style` or `explode`, so it uses OpenAPI's
default for query parameters, `form` with `explode: true`. The client sends one
`tags=` pair per item. The `colors` parameter sets `explode: false`, so its
items are joined with the `pipeDelimited` separator into one value, `gold|cream`,
which is percent-encoded as `gold%7Ccream`.

The client writes the query struct onto the request URL with
[`serde_html_form`][rustdoc-serde-html-form], and a parameter left as `None`
sends nothing. Each value must be a scalar or an array of scalars. A JSON
object or an array of arrays in the untyped `filter` parameter makes the method
return an error before it sends the request. The client doesn't apply
`allowReserved`, which the `q` parameter sets, so reserved characters such as
`/` are percent-encoded.

### Cookie Parameters

Cookie parameters have setters like any other parameter. The petstore's
`getPetCard` operation accepts a `cardTheme` cookie and a `recentlyViewed`
list of pet IDs. Here, `first_viewed` and `second_viewed` are two more
[`uuid::Uuid`][rustdoc-uuid] values:

```rust
let request = GetPetCardRequest::builder()
    .pet_id(pet_id)
    .card_theme(GetPetCardRequestCookieCardTheme::Dark)
    .recently_viewed(vec![first_viewed, second_viewed])
    .build()?;

let response = client.get_pet_card(request).await?;
```

The client sends both cookies in one `Cookie` header:

```text
cardTheme=dark; recentlyViewed=0192f0c1-7a6e-7c4e-9d2a-5b1e8f3c2a11%2C0192f0c1-7a6e-7c4e-9d2a-5b1e8f3c2a12
```

The array's items are joined with commas, and the [`cookie`][rustdoc-cookie]
crate percent-encodes each value, commas included. An optional cookie you
don't set is left out. When the client's cookie store holds cookies for the
URL, such as a [cookie key](#cookie-keys), they follow the operation's cookie
parameters in the same header, as in `cardTheme=dark; session=...`. A stored
cookie is left out when you send a parameter of the same name, so the header
never carries two values for one name. A stored cookie of a parameter you don't
set still goes out.

## Reading Responses

A decoded HTTP response is represented by `ApiResponse`. Its variants identify
HTTP statuses, while its type parameters hold the operation's success and
failure payloads. An API error response can therefore be a successfully decoded
`ApiResponse` value, even though the HTTP status indicates failure.

For `show_pet_by_id`, the success payload contains both a pet and response
headers. Its failures use two schemas: the `404` response returns a `Problem`,
while the `4XX` range and the `default` response return an `Error`. The
generator combines them in the payload union `ErrorOrProblem`. We can match the
`response` from the previous example:

```rust
match response {
    ApiResponse::Ok(WithHeaders { headers, body: pet }) => {
        println!("Pet: {}", pet.name);
        println!("Cache status: {:?}", headers.x_cache);
    }
    ApiResponse::NotModified => println!("Pet unchanged"),
    ApiResponse::NotFound(ErrorOrProblem::Problem(problem)) => {
        eprintln!("Not found: {:?}", problem.detail);
    }
    ApiResponse::ClientError(status, ErrorOrProblem::Error(error))
    | ApiResponse::Unknown(status, ErrorOrProblem::Error(error)) => {
        eprintln!("{status}: {}", error.message);
    }
    other => anyhow::bail!("unexpected response: {other:?}"),
}
```

The `Ok` arm gives us the body and headers separately. `NotModified` is the
`304` response, which has no body. `ClientError` represents the `4XX` range and
`Unknown` the `default` response; both carry the status that arrived. The
wildcard arm covers the remaining variants because operations share one
response enum, including variants that this operation doesn't return.

The client decodes each body in the media type its status declares. For
example, `get_pet_card()` returns its `text/plain` kennel card as
`ApiResponse<String, Problem>`, and `get_pet_photo()` returns its `image/png`
photo as `ApiResponse<Vec<u8>, Problem>`. See
[Response Media Types](./code-generation.md#response-media-types).

An optional response header becomes `None` when it's absent or can't be
parsed. For a list header, one invalid element makes the whole value `None`.
A missing or invalid required header causes decoding to return an error. See
[Responses](./code-generation.md#responses) for how the generator chooses
payload types, combines headers, and represents undeclared statuses.

### Missing and Null Fields

A struct whose schema gives any property a `default` or a `const` has
`#[serde(default)]`, so a missing field takes its value from the struct's
`Default` implementation, as [Field Defaults](./code-generation.md#field-defaults)
describes. The client also reads `null` in such a struct's required fields.
Each required field without a `default` of its own gets a
`serde_with::DefaultOnNull` adapter, which reads `null` as the field type's
default:

```rust
#[serde_with::serde_as]
#[serde(default)]
pub struct Order {
    /* ... */
    #[serde_as(as = "serde_with::DefaultOnNull")]
    #[serde(rename = "petId")]
    pub pet_id: uuid::Uuid,
    /* ... */
}
```

A response with `"petId": null` gives `pet_id` the nil UUID, as a response
without `petId` does. A field with another adapter keeps it inside, as in
`serde_with::DefaultOnNull<serde_with::base64::Base64>`. A field with its own
`default` or `const` still rejects `null`.

### Constant Properties

A property with a `const`, or with an `enum` that lists a single value, has that
one value as its default. The client reads the property from the response like any
other field, so it keeps the value the server sends. The optional `object` property
of `ChatPreference` becomes:

```rust
pub struct ChatPreference {
    /* ... */
    #[default(Some("chat.preference".to_string()))]
    pub object: Option<String>,
    /* ... */
}
```

A response that omits the property takes the constant from `#[default(...)]`, like any
other spec default. A response with a different value keeps that value rather than
replacing it with the constant.

### Rejections from a Generated Server

A generated server answers some requests before its service runs. Missing
credentials receive `401`, and malformed headers or bodies receive `400`. Each
of these responses carries a plain-text reason rather than a body from the
document's error schemas; see
[Returning Responses](./server-generation.md#returning-responses). The client
decodes such a response by its status, like any other:

- If the operation declares a body for that status, directly, through a range
  such as `4XX`, or through `default`, the client tries to decode the plain
  text as that body. Decoding fails, so the method returns an error through its
  outer `anyhow::Result`.
- If the operation doesn't declare the status and has no `default` response,
  the method returns `ApiResponse::Other` with the status and the plain-text
  bytes.

For example, calling `list_api_keys()` without setting the `ApiKeyAuth` key
sends a request that a generated server rejects with `401`. Because
`listApiKeys` declares an `Error` body for `401`, the method returns a decoding
error rather than `ApiResponse::Unauthorized`. Set the required credentials
before calling an operation. The service's own `401` responses, such as one for
a key it doesn't recognize, use the declared body and decode normally.

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

## Reading Server-Sent Events

Some operations send a stream of events rather than one body. OpenAPI
describes this with a `text/event-stream` response, the media type of
[Server-Sent Events][mdn-sse] (SSE), and the response's schema describes one
event. The petstore's `streamPetVitals` operation streams `VitalsReading`
events, so its client method returns
`ApiResponse<oas3_gen_support::EventStream<oas3_gen_support::SseFrame<VitalsReading>>, ()>`.

`EventStream` is a [`Stream`][rustdoc-futures-stream] whose items are
`Result<SseFrame<VitalsReading>, DiagnosticsError>`. Each `SseFrame` is one
event as the client read it: `data()` returns its `data` decoded from JSON,
`event()` its event name, `id()` the last event ID the stream set, and
`retry()` the reconnection time the stream last asked for. A frame is
read-only, and only the stream creates one. Let's print readings as they
arrive:

```rust
use futures::StreamExt;

let request = StreamPetVitalsRequest::builder().pet_id(pet_id).build()?;

let mut readings = match client.stream_pet_vitals(request).await? {
    ApiResponse::Ok(readings) => readings,
    other => anyhow::bail!("unexpected response: {other:?}"),
};

while let Some(reading) = readings.next().await {
    match reading {
        Ok(frame) => println!("{} bpm", frame.data().heart_rate_bpm),
        Err(e) => eprintln!("skipped an event: {e}"),
    }
}
```

The `stream_pet_vitals()` method returns once the response headers arrive.
The readings are decoded as the server sends them, and the loop ends when the
server closes the stream. [`StreamExt::next()`][rustdoc-futures-stream-ext]
comes from the `futures` crate, which the generated client manifest doesn't
declare, so add `futures` to your own dependencies. When you only need the
data, `SseFrame::into_data()` gives it to you without the rest of the frame.

An event whose data doesn't match `VitalsReading` yields a
`DiagnosticsError::DeserializationError` whose `path` names the field under
`data` that failed, such as `data.heartRateBpm`, and the stream continues with
the next event. An event without data is skipped. If reading the response body
fails, the stream yields `DiagnosticsError::BodyReadError` and then ends.

The client reads the stream the way the [HTML standard][whatwg-sse] tells
browsers to. Lines can end with LF, CR, or CRLF, comment lines and unknown
fields are ignored, a leading byte order mark is dropped, and invalid UTF-8
becomes U+FFFD. An event that the server cuts off before its closing blank line
is never yielded.

### Reading Whole Frames

Some documents describe the whole frame of an event instead of its data. When a
`text/event-stream` schema is an object whose properties all come from `data`,
`event`, `id`, and `retry`, with `data` among them, the generator treats it as
a *frame type*. Speakeasy uses this convention, and OpenAPI 3.2 adopts it for
event streams. The petstore's `streamAssistantMessage` streams a frame type:

```json
"AssistantEventFrame": {
  "type": "object",
  "required": ["data", "event"],
  "properties": {
    "data": { "$ref": "#/components/schemas/AssistantEvent" },
    "event": { "type": "string", "enum": ["message_start", "message_stop", "ping"] },
    "id": { "type": "string" },
    "retry": { "type": "integer", "minimum": 0 }
  }
}
```

This is an excerpt; the petstore lists more event names. A frame type already
describes an `SseFrame`, so the client generates it as an alias of the frame
of its `data` and `event` types:

```rust
pub type AssistantEventFrame = oas3_gen_support::SseFrame<Box<AssistantEvent>, AssistantEventFrameEvent>;
```

Each part of the frame decodes like this:

- `data` decodes as its own schema. A plain `string` keeps the frame's text as
  it is. A string with `contentMediaType: application/json` and a
  `contentSchema` decodes as that schema. Anything else, such as the
  `AssistantEvent` reference here, decodes from JSON.
- `event` decodes into the type the schema declares, such as the
  `AssistantEventFrameEvent` enum, and an event name the type doesn't accept
  is a `DeserializationError` whose `path` is `event`. A frame without an
  `event:` line is a `message` event, exactly like one that sends
  `event: message`, and a `message` is `None` when the `event` property is
  optional. An `event` declared as a string or a `const` stays a `String`,
  which is the frame's default event type.
- `id()` and `retry()` come from the stream, whatever types the `id` and
  `retry` properties declare. A frame without an `id:` line keeps the last ID
  the stream set, including one sent in a frame without data, as browsers
  report it. `retry()` works the same way for the reconnection time.

A `text/event-stream` schema can also be a `oneOf` of frame types that each fix
`event` to a constant. The petstore's `streamAssistantTokens` streams
`AssistantTokenFrame`, a `oneOf` of `TokenFrame` and `UsageFrame` whose events
are `token` and `usage`. The client generates an enum of the frames the members
alias, `SseFrame<String>` and `SseFrame<Usage>`, and picks the member from the
frame's event name. Let's print the tokens as they arrive and then the usage:

```rust
use futures::StreamExt;

let mut frames = match client.stream_assistant_tokens(StreamAssistantTokensRequest {}).await? {
    ApiResponse::Ok(frames) => frames,
    other => anyhow::bail!("unexpected response: {other:?}"),
};

while let Some(frame) = frames.next().await {
    match frame? {
        AssistantTokenFrame::Token(token) => print!("{}", token.data()),
        AssistantTokenFrame::Usage(usage) => println!("\n{} tokens", usage.data().output_tokens),
    }
}
```

A frame whose event name no member declares yields a `DeserializationError`
whose `path` is `event`, and the stream continues with the next frame. A frame
without an `event:` line goes to the member whose event is `message`, if there
is one.

A frame type can also be a JSON body or a property of another schema. Your code
needs to build and serialize the type there, so the client keeps a frame struct
that anything besides a stream uses as a struct, and its streams read
`SseFrame<..>` of its `data` and `event` types in its place. A union of frame
types that another schema holds streams each event's data as a JSON payload
instead.

`streamAssistantMessage` answers `200` with either a `ContentResponse` document
or the stream, so its payload is the union
`ContentResponseOrAssistantEventFrameStream`. The stream member is named after
its item type with a `Stream` suffix, and the response's `Content-Type` decides
which member you receive.

### Ending and Resuming a Stream

A `text/event-stream` media type can name the data that ends the stream with
the `x-sse-sentinel` or `x-speakeasy-sse-sentinel` extension. When a media type
declares both, `x-sse-sentinel` wins. `streamAssistantMessage` declares
`x-speakeasy-sse-sentinel: "[DONE]"`, so its stream returns `None` when an
event's data is `[DONE]`, and `streamAssistantTokens` declares
`x-sse-sentinel: "[END]"`. The sentinel itself is never decoded, so it doesn't
produce an error item. Without a sentinel, the stream ends when the server
closes it.

To resume an interrupted stream, send `EventStream::last_event_id()` back to
the server, for example in a `Last-Event-ID` header, after waiting for
`EventStream::retry()`. Unlike a frame's `id()` and `retry()`, these include
an ID or reconnection time the server sent in a frame without data after the
last frame you read.

## Sending Request Bodies

When an operation declares a request body, its request type has a `body`
field, and the builder has a `body()` setter. The client sends the body in the
media type the operation declares and sets `Content-Type` to match:

| Declared media type | How the client sends the body |
|---|---|
| `application/json` | As JSON, with `Content-Type: application/json` |
| A `+json` type, such as `application/merge-patch+json` | As JSON, with the declared `Content-Type` |
| `application/x-www-form-urlencoded` | As form fields |
| `text/*`, such as `text/plain` | As text, with the declared `Content-Type` |
| A binary type, such as `image/png` | As raw bytes, with the declared `Content-Type` |
| A wildcard, such as `*/*` or `image/*` | In the body's own kind: JSON as `application/json`, text as `text/plain`, bytes as `application/octet-stream` |
| `multipart/form-data` | As parts; see [Sending Multipart Bodies](#sending-multipart-bodies) |

A `charset` parameter other than UTF-8 on a text or JSON body is replaced with
`charset=utf-8`, because the client sends that text as UTF-8. A binary body
carries no charset, so its `charset` parameter is dropped.

For example, the petstore's `updatePet` operation takes a JSON merge patch.
Its `update_pet()` method sets the declared media type and then serializes the
body:

```rust
let mut req_builder = self
    .client
    .patch(url)
    .header(reqwest::header::CONTENT_TYPE, "application/merge-patch+json")
    .json(&request.body);
```

This is an excerpt from the generated `client.rs`. The patch's optional fields are [`Option`][rustdoc-option] values, and a `None`
field is left out of the patch. As a result, the client can't send `null` to
clear a nullable field such as `nickname`.

A body whose schema is a primitive, such as a string, keeps that schema's
constraints. The petstore's `setPetNotes` operation declares a `text/plain`
body with `maxLength: 2000`, so the generated request validates the body like
any other field:

```rust
let request = SetPetNotesRequest::builder()
    .pet_id(pet_id)
    .body("Prefers the quiet kennel by the window.".to_string())
    .build()?;

let response = client.set_pet_notes(request).await?;
```

The client sends the notes as `text/plain`. Notes longer than 2,000 characters
make `build()` return a validation error, and the client method checks the
same constraint before sending.

## Sending Multipart Bodies

When an operation's request body uses `multipart/form-data`, the client method
sends the body struct as a form with one or more parts per property.
[Multipart Form Bodies](./code-generation.md#multipart-form-bodies) describes
which part each property becomes. This section shows the code that builds them.

The petstore's `submitIntakeForm` operation takes an `IntakeForm` body with
`name`, `file`, `attachments`, and `quality` fields, among others. Its
`submit_intake_form()` method moves each field into a
[`reqwest::multipart::Form`][rustdoc-reqwest-form]:

```rust
let body = request.body;
let mut form = reqwest::multipart::Form::new().percent_encode_noop();
form = form.text("name", body.name);
if let Some(value) = body.file {
    form = form
        .part(
            "file",
            reqwest::multipart::Part::bytes(value)
                .file_name("file")
                .mime_str("application/octet-stream")?,
        );
}
for value in body.attachments {
    form = form
        .part(
            "attachments",
            reqwest::multipart::Part::bytes(value)
                .file_name("attachments")
                .mime_str("application/octet-stream")?,
        );
}
if let Some(value) = body.quality {
    form = form.text("quality", value.to_string());
}
req_builder = req_builder.multipart(form);
```

This is an excerpt from the generated `client.rs`, trimmed to those four
fields. A binary field becomes a file part with a filename, which is how
servers such as FastAPI tell an uploaded file from a text field. The filename
is the property name because the specification doesn't name the file. An array
sends one part per item under the same name, and a `None` field sends no part
at all. The bytes move into the part rather than being copied. Part names are
sent as raw UTF-8, as browsers send them, so the form's `résumé` property keeps
its name.

When the multipart body itself is optional, as in the petstore's `uploadMedia`
operation, the request's `body` field is an [`Option`][rustdoc-option]. A
`None` body sends a request with no body and no `Content-Type`. A body whose
fields are all unset still sends an empty `multipart/form-data` body.

## Sending Credentials

For operations with `apiKey` or bearer security schemes, the client provides
methods to set credentials. See [Security Schemes](./code-generation.md#security-schemes)
for how schemes and requirements are declared.

The petstore's top-level `security` requires a header key named `ApiKeyAuth`,
so most operations accept it. Individual operations also accept a query key
named `QueryKey`, a cookie key named `SessionCookie`, a bearer token named
`BearerAuth`, and an `apiKey` scheme named `TokenAuth` that travels in the
`Authorization` header. To configure the header key, read it from your
application's environment and pass it to the generated setter:

```rust
let api_key = std::env::var("API_KEY")?;
let client = PetstoreClient::new().with_api_key_auth(api_key);
```

The client stores header keys as [`http::HeaderValue`][rustdoc-http-header-value]
values marked sensitive, and query keys and bearer tokens as
[`secrecy::SecretString`][rustdoc-secret-string] values, each wrapped in
[`Option`][rustdoc-option]. A header key setter leaves the key unset when it's
empty or isn't a valid header value. The client gets one field and setter for each
supported scheme used by the selected operations. Names come from the scheme:
`ApiKeyAuth` becomes `api_key_auth` and `with_api_key_auth()`. If a name
collides with another field, including `client` or `base_url`, the generator
adds a numeric suffix. Schemes the generator doesn't support, such as the
petstore's `BasicAuth` and `PetstoreOAuth`, don't get a setter.

Each client method passes the schemes its operation accepts to a private
`set_security()` helper, which attaches the header keys, query keys, and bearer
tokens you've configured among them. The client doesn't enforce security
requirements before sending a request. If you leave a required key unset, the request goes out without it,
and a generated server rejects it as described in
[Rejections from a Generated Server](#rejections-from-a-generated-server).

### Bearer Tokens

A bearer scheme gets a field and setter, like a header key. The petstore's
`BearerAuth` scheme produces `with_bearer_auth()`:

```rust
let token = std::env::var("STAFF_TOKEN")?;
let client = PetstoreClient::new().with_bearer_auth(token);
```

Pass the token by itself, without the `Bearer` prefix. For an operation that
accepts the scheme, such as `revokeApiKey`, the client method calls
[`reqwest::RequestBuilder::bearer_auth()`][rustdoc-reqwest-bearer-auth]. That
method sends the token in an `Authorization: Bearer <token>` header and marks
the header value as sensitive.

A request carries one `Authorization` header. When an operation accepts more
than one credential that travels in it, such as a bearer token and an `apiKey`
scheme named `Authorization`, the client sends only one of them: of the
credentials you've configured, the scheme declared last wins. The petstore's
`listAuditEvents` accepts `BearerAuth` or `TokenAuth`, and `TokenAuth` is
declared later, so a client with both set sends the `TokenAuth` key.

### Cookie Keys

Cookie keys use a shared [`reqwest_cookie_store::CookieStoreMutex`][rustdoc-cookie-store] in the
client's `cookies` field. The `new()` and `with_base_url()` constructors attach that store to the
underlying [`reqwest::Client`][rustdoc-reqwest-client]. To add the petstore's session cookie, call its
setter:

```rust
let session_key = std::env::var("SESSION_KEY")?;
let client = PetstoreClient::new().with_session_cookie(session_key);
```

The setter inserts a cookie named `session` for the base URL and sends your key
as it is, without percent-encoding, so a key with `/`, `=`, `:`, or `@` reaches
the server unchanged. A generated server percent-decodes cookie values, so a
key containing an escape such as `%41` arrives there as `A`. A key with a space
or a semicolon can't be sent in a `Cookie` header.
Reqwest then sends the cookie on requests that match its scope, including
requests to operations that don't declare that scheme. The store also retains
cookies received from the server. Clones of the generated client share the
store, so setting a cookie key on a clone changes the store used by the
original client too.

### Using a Custom HTTP Client

`with_client()` accepts an HTTP client that's already built, so it can't attach
the generated cookie store to it. To configure your own client with cookie
support, build it with the generated store and assign it to `client`:

```rust
use std::sync::Arc;

let mut api = PetstoreClient::with_base_url("https://api.petstore.example/v3")?;
api.client = reqwest::Client::builder()
    .cookie_provider(Arc::clone(&api.cookies))
    .build()?;

let api = api.with_session_cookie(std::env::var("SESSION_KEY")?);
```

The [`Arc`][rustdoc-arc] lets `api` and its HTTP client share the same store. Add any other
configuration to the [`reqwest::ClientBuilder`][rustdoc-reqwest-client-builder] before calling `build()`.

### Keeping Keys Out of Logs

Header keys are sensitive `HeaderValue`s, whose debug output is `Sensitive`.
Query keys and bearer tokens use [`SecretString`][rustdoc-secret-string], whose
debug output hides the value. The generated client's debug output also omits the
cookie store. Formatting the generated client with `{:?}` therefore doesn't
print these stored credentials.
See [Keeping Keys Out of Logs](./code-generation.md#keeping-keys-out-of-logs)
for the secret type's behavior when you explicitly read a key.

## Deprecated Operations

When an operation sets `deprecated: true`, its client method carries Rust's
[`#[deprecated]`][rust-deprecated] attribute, so code that calls it gets a
compiler warning. The petstore's `listCats` operation has been superseded by
`listPets`. This excerpt shows the start of its generated method:

```rust
/// List cats
///
/// Superseded by `listPets` with `x-only: cat`.
///
/// * Path: `GET /cats`
#[deprecated]
pub async fn list_cats(
    &self,
    request: ListCatsRequest,
) -> anyhow::Result<ApiResponse<WithHeaders<XNextHeaders, Cats>, Error>> {
```

The method still sends the request. The operation's description, copied into
the method's documentation, usually tells you what to use instead. To keep
calling the method while you migrate, add `#[allow(deprecated)]` to the calling
function.

## Choosing Operations and Names

Use [`--only` or `--exclude`](./code-generation.md#operation-filtering) to select
operations. Use [`--fn-name`](./code-generation.md#function-name-overrides) to
rename methods and their request types, or
[`--api-name`](./code-generation.md#api-name-override) to name the client itself.
The [shared reference](./code-generation.md#flag-summary) also covers visibility,
enums, type adapters, and collection choices.

[mdn-sse]: https://developer.mozilla.org/en-US/docs/Web/API/Server-sent_events
[rust-deprecated]: https://doc.rust-lang.org/reference/attributes/diagnostics.html#the-deprecated-attribute
[rustdoc-anyhow-result]: https://docs.rs/anyhow/1.0.104/anyhow/type.Result.html
[rustdoc-arc]: https://doc.rust-lang.org/std/sync/struct.Arc.html
[rustdoc-cookie]: https://docs.rs/cookie/0.18.2/cookie/
[rustdoc-cookie-store]: https://docs.rs/reqwest_cookie_store/0.10.0/reqwest_cookie_store/struct.CookieStoreMutex.html
[rustdoc-futures-stream]: https://docs.rs/futures/0.3.34/futures/stream/trait.Stream.html
[rustdoc-futures-stream-ext]: https://docs.rs/futures/0.3.34/futures/stream/trait.StreamExt.html#method.next
[rustdoc-http-header-value]: https://docs.rs/http/1.5.0/http/header/struct.HeaderValue.html
[rustdoc-option]: https://doc.rust-lang.org/std/option/enum.Option.html
[rustdoc-reqwest-bearer-auth]: https://docs.rs/reqwest/0.13.5/reqwest/struct.RequestBuilder.html#method.bearer_auth
[rustdoc-reqwest-client]: https://docs.rs/reqwest/0.13.5/reqwest/struct.Client.html
[rustdoc-reqwest-client-builder]: https://docs.rs/reqwest/0.13.5/reqwest/struct.ClientBuilder.html
[rustdoc-reqwest-form]: https://docs.rs/reqwest/0.13.5/reqwest/multipart/struct.Form.html
[rustdoc-reqwest-response]: https://docs.rs/reqwest/0.13.5/reqwest/struct.Response.html
[rustdoc-reqwest-url]: https://docs.rs/reqwest/0.13.5/reqwest/struct.Url.html
[rustdoc-result]: https://doc.rust-lang.org/std/result/enum.Result.html
[rustdoc-secret-string]: https://docs.rs/secrecy/0.10.3/secrecy/type.SecretString.html
[rustdoc-serde-html-form]: https://docs.rs/serde_html_form/0.4.1/serde_html_form/
[rustdoc-uuid]: https://docs.rs/uuid/1.27.0/uuid/struct.Uuid.html
[whatwg-sse]: https://html.spec.whatwg.org/multipage/server-sent-events.html#event-stream-interpretation
